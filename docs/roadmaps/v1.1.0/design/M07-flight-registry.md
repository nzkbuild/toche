# M07 Design — Correct Multi-Client Flight Registry

## Decision record

M07 addresses three architectural gaps identified in the M01 current-reality audit:

- **ADR-002**: Existing coalescing tests did not exercise true shared-store concurrency. The `concurrent_waiter_gets_coalesced` test spawned a waiter on a *fresh* `CoalesceStore`, so leader and waiter never shared the same store.
- **ADR-004**: The flight key was `{upstream_url}|{fingerprint}` — different credentials to the same URL with identical bodies would coalesce, violating trust isolation.
- **Streaming**: The roadmap explicitly states that `1.1.0` must not retain buffered-after-completion behaviour while claiming transparent streaming coalescing.

## Flight key design

The new flight key is a 4-component pipe-delimited string:

```
{upstream_url}|{request_fingerprint}|{trust_domain_id}|{policy_hash}
```

| Component | Source | Purpose |
|-----------|--------|---------|
| `upstream_url` | Resolved integration | Separate traffic to different providers |
| `request_fingerprint` | `shield::fingerprint::compute()` | Identify semantically equivalent requests |
| `trust_domain_id` | `identity::derive_trust_domain_id()` | Isolate different credential references |
| `policy_hash` | `identity::compute_policy_hash()` | Isolate requests with different optimization policies |

### Policy hash derivation

`compute_policy_hash` encodes the effective optimization policy as a SHA-256 hash (first 8 bytes → 16 hex chars):

```
toche-policy-v1:cache:{enabled}:{mode}:{breakpoint}|reduce:{enabled}|efficiency:{mode}|safe_cache:{enabled}
```

Inputs come from the `ResolvedIntegration` after policy resolution, not from raw TOML. This ensures that configuration changes (e.g., enabling reduction, switching from observe to auto cache mode) produce different policy hashes and prevent cross-policy coalescing.

### Trust domain component

The trust domain ID already incorporates integration identity, upstream identity, and credential reference (via `SecretRef::Display`, which never leaks raw credential values). Including it in the flight key ensures that two integrations using the same upstream URL with different API keys never share flights — even when the request bodies are identical.

## Concurrency model

### Leader/waiters pattern

The `CoalesceStore` uses a `std::sync::Mutex<HashMap<Key, broadcast::Sender>>`:

1. **Leader**: First request for a key inserts a `broadcast::Sender` and returns `Forward`.
2. **Waiters**: Subsequent requests find the existing sender, call `subscribe()`, and await `rx.recv()`.
3. **Completion**: Leader calls `complete()` → sender is removed from HashMap → `send(Some(response))` wakes all subscribers → they return `Coalesced`.
4. **Failure**: Leader calls `cancel()` → sender is removed → `send(None)` wakes all subscribers → they return `Failed`.

### RAII cleanup

- `complete()` and `cancel()` remove the key from the HashMap, preventing stale entries.
- If a leader panics without calling `complete()` or `cancel()`, the `broadcast::Sender` is dropped, closing the channel. Waiters receive `RecvError::Closed` → `Failed`. However, the key *remains* in the HashMap — this is a known limitation. A production guard (Drop impl or `catch_unwind`) would call `cancel()`. The tests document this gap explicitly.

### Mutex choice

`std::sync::Mutex` (not `tokio::sync::Mutex`) is used because:
- All lock durations are short (HashMap insert/remove).
- Poison recovery via `unwrap_or_else(|e| e.into_inner())` keeps the store available after a task panic.
- The mutex is never held across an `.await` point.

## Streaming decision

### Buffered-after-completion is not transparent coalescing

The current architecture buffers the entire upstream response body (`forward_to_upstream` collects all bytes_stream chunks into a `Vec<u8>`) before replaying to waiters. This is acceptable for non-streaming requests where the full response is available at once, but it is *not* transparent streaming coalescing for the following reasons:

1. **Latency**: All waiters must wait for the leader's complete response before receiving any data.
2. **Memory**: The full response is buffered in memory regardless of size.
3. **Semantics**: The downstream client does not experience streaming behaviour — it receives the response as a single SSE event.
4. **Mid-stream errors**: If the upstream stream fails after partial delivery, the buffered data is incomplete and would corrupt waiters that receive it.

### Implementation

Streaming coalescing is **disabled** in M07:

```rust
fn is_streaming_request(body: &str) -> bool { ... }

let shield_result = if bypass_shield || is_streaming {
    CoalesceResult::Forward { key: String::new() }  // bypass flight registry
};
```

- Requests with `stream: true` bypass the flight registry entirely and execute independently.
- Non-streaming requests continue using buffered replay through the new 4-component key.
- The `shield_key_for_complete` is `None` for streaming requests (empty key), so `complete()` is never called on the flight store for streaming responses.

### Future work

| Phase | What | Prerequisite |
|-------|------|-------------|
| 1 | Disable streaming coalescing | M07 (done) |
| 2 | Prove safe non-streaming coalescing | M07 (done) |
| 3 | Implement bounded live fan-out | Future milestone |
| 4 | Enable streaming coalescing | After fan-out acceptance gate |

Live fan-out requires replay of already-emitted events for late waiters, bounded memory, slow-consumer isolation, terminal event propagation, and no persistence of incomplete streams. None of this is implemented in M07.

## True shared-state concurrency tests

M07 replaces the single-threaded `block_on` tests with real `#[tokio::test]` async tests that share the same `Arc<CoalesceStore>` across concurrent tasks:

| Test | Property verified |
|------|------------------|
| `waiter_gets_coalesced_on_same_store` | Leader completes → waiter on same store receives `Coalesced` with correct body |
| `leader_cancel_wakes_waiter_with_failed` | Leader cancels → waiter receives `Failed` → store is clean |
| `leader_panic_leaves_no_stale_flight` | Panic simulation: cancel cleans the HashMap |
| `dropped_sender_without_complete_is_stale` | Bug scenario: explicit cancel after dropped sender cleans store |
| `waiter_cancellation_leaves_leader_intact` | Waiter timeout does not prevent leader from completing |
| `slow_waiter_does_not_block_upstream_stream` | `complete()` returns in <1s regardless of waiters |
| `multiple_waiters_all_get_response` | 3 waiters on same store all receive `Coalesced` |
| `concurrent_spawning_with_same_key_coalesces` | 5 concurrent tasks with same key: at least 1 leader, all resolve without hangs |
| `concurrent_waiter_receives_leaders_response` | Leader completes → spawned waiter on same store gets `Coalesced` |
| `different_policy_hashes_different_keys` | Same URL+fp+domain but different policy → both get `Forward` |

## Protocol version

The flight key does not currently include a protocol version component. Anthropic Messages API version is recorded in the `anthropic-version` header but not embedded in the key. When M09 adds OpenAI Responses, protocol-aware key scoping can be added trivially by prepending a protocol identifier.

## Limitations (intentional)

1. **No streaming coalescing**: Requests with `stream: true` bypass the flight registry and execute independently. See streaming decision above.
2. **No fan-out**: Multiple waiters receive the complete buffered response after the leader finishes. No chunk-by-chunk fan-out.
3. **No partial-response sharing**: No mechanism for waiters to join a stream mid-flight.
4. **No protocol version in key**: Not needed until M09.
5. **Panic without cancel**: If a leader panics without calling `cancel()`, the key remains in the HashMap (though the channel closes, waking waiters as `Failed`). A Drop guard or `catch_unwind` wrapper is deferred.

These limitations are documented here as *intentional scope boundaries*, not as defects.
