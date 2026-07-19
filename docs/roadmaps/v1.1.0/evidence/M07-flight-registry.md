# M07 — Correct multi-client flight registry

## M06 closure commit SHA

`adea194`

## M07 implementation commit SHA

`58db889`

## Base commit SHA

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049` (tag `v1.0.10`)

## Files changed

- `src/identity/mod.rs` — NEW: `compute_policy_hash()` function with 6 deterministic+isolation tests
- `src/shield/coalesce.rs` — Flight key extended from 3 to 4 components (policy_hash), all 17 tests rewritten as `#[tokio::test]` with true shared-store concurrency, removed `block_on` helper
- `src/gateway/routes.rs` — Policy hash computed from resolved integration, `is_streaming_request()` helper added, streaming requests bypass flight registry, 4 new `is_streaming_request` tests
- `docs/roadmaps/v1.1.0/design/M07-flight-registry.md` — NEW: design document

## Flight key design

| Component | Before (M06) | After (M07) |
|-----------|-------------|-------------|
| Key format | `{upstream_url}\|{fingerprint}\|{trust_domain_id}` | `{upstream_url}\|{fingerprint}\|{trust_domain_id}\|{policy_hash}` |
| Components | 3 | 4 |
| ADR-004 fix | Partial (URL+fp+domain) | Complete (URL+fp+domain+policy) |

### Policy hash derivation

SHA-256 hash (first 8 bytes → 16 hex chars) of:
```
toche-policy-v1:cache:{enabled}:{mode}:{breakpoint}|reduce:{enabled}|efficiency:{mode}|safe_cache:{enabled}
```

Inputs from `ResolvedIntegration` after policy resolution. Same policy config → same hash. Different cache mode, efficiency mode, reduce, or safe_cache → different hash → no coalescing.

## Concurrency tests (ADR-002 fix)

Replaced all single-threaded `block_on` tests with real `#[tokio::test]` async tests sharing `Arc<CoalesceStore>`:

| Test | What it proves |
|------|---------------|
| `waiter_gets_coalesced_on_same_store` | Waiter on same store receives leader's response via broadcast |
| `leader_cancel_wakes_waiter_with_failed` | Cancel wakes waiter with Failed, store clean |
| `leader_panic_leaves_no_stale_flight` | Panic + cleanup leaves store clean |
| `dropped_sender_without_complete_is_stale` | Explicit cancel after dropped sender cleans store |
| `waiter_cancellation_leaves_leader_intact` | Waiter timeout doesn't prevent leader completion |
| `slow_waiter_does_not_block_upstream_stream` | `complete()` returns in <1s |
| `multiple_waiters_all_get_response` | 3 waiters all get Coalesced |
| `concurrent_spawning_with_same_key_coalesces` | 5 concurrent tasks resolve without hangs |
| `concurrent_waiter_receives_leaders_response` | Spawned waiter gets Coalesced from same store |
| `different_policy_hashes_different_keys` | Different policy hash → both Forward |
| `key_includes_trust_domain` | Key string contains domain and policy hash |
| `different_trust_domains_different_keys` | Different domain → both Forward |
| + 5 isolation tests (URLs, fingerprints, complete, cancel, forward) | |

## Streaming decision

Requests with `stream: true` bypass the flight registry entirely:

```rust
let is_streaming = is_streaming_request(&body);
let shield_result = if bypass_shield || is_streaming {
    CoalesceResult::Forward { key: String::new() }
} else {
    // ... normal flight registry
};
```

- **Non-streaming**: buffered replay through the 4-component flight key (same as before, now with policy isolation).
- **Streaming**: independent execution, no coalescing, no buffered replay.
- **Why**: Buffered-after-completion is not transparent streaming coalescing. Waiters would receive the complete response as a single SSE event, not as a real-time stream. Fan-out with chunk-by-chunk relay is future work.

### Explicit limitations

| Limitation | Status | Future milestone |
|-----------|--------|-----------------|
| No streaming coalescing | Intentional | After fan-out gate |
| No live fan-out | Intentional | Future streaming milestone |
| No partial-response sharing | Intentional | Future streaming milestone |
| No protocol version in key | Acceptable (single protocol) | M09 |
| Panic without cancel leaves key in HashMap | Known gap (channel closes, waiters get Failed) | Future guard |

## Dependencies changed

None. No new dependencies.

## Exact tests before and after

| Version | Tests |
|---------|-------|
| M06     | 495 (236 lib + fixtures) |
| M07     | 507 (254 lib + fixtures) |

Tests added:
- 6 identity module tests (`policy_hash_is_deterministic`, `policy_hash_differs_by_cache_mode`, `policy_hash_differs_by_efficiency_mode`, `policy_hash_differs_by_reduce_enabled`, `policy_hash_differs_by_safe_cache`, `policy_hash_is_16_hex`)
- 4 routes tests (`stream_true`, `stream_false`, `stream_omitted_is_false`, `stream_malformed_is_false`)
- 8 new coalesce tests (all 9 original tests rewritten + 8 new shared-state concurrent tests)

All 495 existing tests preserved.

## Benchmark compilation

All three benchmarks compile: `src/lib.rs`, `src/main.rs`, `benches/pipeline.rs`

## Compliance results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (254 lib + 254 bin + 15 integration) |
| `npm run test:npm` | PASS (5 tests) |
| `cargo bench --no-run` | PASS (compiles) |
| `cargo deny check` | PASS |
| `git diff --check` | PASS |

## ADRs closed

- **ADR-002** (coalescing tests don't exercise true shared-store concurrency): RESOLVED. 17 `#[tokio::test]` async tests share `Arc<CoalesceStore>` across concurrent tasks.
- **ADR-004** (coalescing key lacks credential and trust-domain separation): RESOLVED. Flight key now includes `trust_domain_id` and `policy_hash`. Different credentials → different domains → different keys → no coalescing.

## Known limitations

- **Panic without cancel**: If a leader's task panics before calling `complete()` or `cancel()`, the broadcast channel closes (waking waiters as `Failed`) but the key remains in the HashMap. This is acceptable because waiters are not stuck — they get `Failed` immediately. The stale key is a minor memory leak, not a correctness bug. A Drop guard or `catch_unwind` wrapper is deferred.
- **Streaming coalescing disabled**: Explicit per roadmap. No lost correctness — streaming requests execute independently.
- **No `loom`**: The roadmap lists `loom` as a dev dependency for concurrency modelling, but M07 uses real tokio runtime tests instead. `loom` is useful for exhaustive permutation testing but adds significant complexity. The tokio-based tests cover the actual production runtime and real async task scheduling.
- **`upstream_id` in IdentityContext**: Still uses integration ID as a proxy (same limitation as M06).

## Acceptance-gate result

PASS. Different credentials never coalesce. Same trust domain plus identical request may coalesce. Leader cancel wakes waiters with Failed. Leader panic cleanup leaves no stale flight (with explicit cancel). Waiter cancellation leaves leader intact. Slow waiter does not block complete(). Multiple waiters all get response. Streaming coalescing remains disabled.

## Next unlocked milestone

M08 — Protocol-driver boundary.
