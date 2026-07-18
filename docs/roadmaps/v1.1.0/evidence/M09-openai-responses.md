# M09 — OpenAI Responses Ingress Evidence

## M08 closure commit SHA

`a68238b`

## M09 implementation commit SHA

`<to-be-filled-after-commit>`

## Base commit SHA

`1cea733` (branch `feat/1.1.0-multi-client-runtime`)

## Decision

**Route-based protocol selection.** No protocol field in config. Each route handler statically knows its protocol:

| Route | Protocol | Pipeline |
|-------|----------|----------|
| `/v1/messages` | `AnthropicProtocol` | Full (shield, safe cache, reduce, efficiency, cache inject, ledger) |
| `/v1/responses` | `OpenAiResponsesProtocol` | Pass-through only (identity, timing, ledger, no optimizations) |

## Files changed

- `src/protocol/openai_responses/mod.rs` — NEW: `OpenAiResponsesProtocol` implementation with 16 tests
- `src/protocol/mod.rs` — register `pub mod openai_responses`
- `src/gateway/routes.rs` — add `responses` handler (pass-through pipeline: identity, forward, timing, ledger)
- `src/gateway/server.rs` — register `/v1/responses` POST route
- `docs/roadmaps/v1.1.0/design/M09-openai-responses.md` — NEW: design document

## Trait implementation

| Method | Behavior | Rationale |
|--------|----------|-----------|
| `extract_model` | Parse `"model"` from JSON body | Same JSON field name |
| `path` | Return `"/v1/responses"` | OpenAI Responses endpoint |
| `fingerprint` | Delegate to `shield::fingerprint::compute` | Same normalization + SHA-256 |
| `parse_response_headers` | Return `ResponseHeaders::default()` (zeros) | OpenAI does not use Anthropic cache headers |
| `inject_cache_control` | Return body unchanged | No cache breakpoint injection for pass-through |
| `is_streaming` | Check `"stream": true` in JSON body | Same boolean field |

## responses handler vs messages handler

| Feature | messages | responses |
|---------|----------|-----------|
| Identity context | Full | Full |
| Timing | RequestTimer | RequestTimer |
| Header forwarding | Full | Full |
| Streaming detection | Yes | Yes |
| Shield/flight coalescing | Yes | No |
| Safe cache check | Yes | No |
| Reduction | Yes | No |
| Efficiency injection | Yes | No |
| Cache breakpoint injection | Yes | No |
| Safe cache storage | Yes | No |
| Upstream forwarding | Yes | Yes |
| Ledger recording | Yes | Yes |
| Continuity observer | Yes | Yes |

## Dependencies changed

None. No new dependencies.

## Exact tests

| Suite | Passed | Failed |
|-------|--------|--------|
| lib | 274 | 0 |
| main | 274 | 0 |
| other | 69 | 0 |
| **Total** | **617** | **0** |

New tests in `src/protocol/openai_responses/mod.rs`:
- 5 `extract_model` tests (present, custom prefix, missing, empty body, malformed)
- 1 `path` test
- 2 `fingerprint` tests (delegation, determinism)
- 2 `parse_response_headers` tests (always zero, empty headers)
- 1 `inject_cache_control` test (returns body unchanged)
- 4 `is_streaming` tests (true, false, omitted, malformed)
- 1 integration test via main binary (responses handler compiles and is routable)

All existing tests preserved. Pipeline behaviour unchanged for `/v1/messages`.

## Benchmark compilation

All three benchmarks compile: `src/lib.rs`, `src/main.rs`, `benches/pipeline.rs`

## Compliance results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (617 passed, 0 failed) |
| `cargo bench --no-run` | PASS |
| `cargo deny check` | PASS |
| `git diff --check` | PASS |

## Known limitations

- **responses handler duplicates ~80 lines from messages handler**: Identity context construction, header forwarding, upstream call, and ledger recording are copy-pasted. Future refactoring (M10 or later) should extract shared helpers.
- **No protocol field on config yet**: The `responses` handler uses the default integration's Anthropic upstream URL with `/v1/responses` appended. A proper OpenAI upstream configuration (M10 Codex integration) will need its own integration/upstream entry.
- **Streaming passthrough collects full response before SSE**: Same behavior as the messages handler. Live streaming fan-out is deferred per M07 streaming decision.
- **No OpenAI-specific usage token extraction**: The handler records `cache_read_tokens: 0, cache_create_tokens: 0`. The OpenAI Responses API reports usage differently (not in Anthropic cache headers). Token usage from OpenAI responses is not yet extracted — `estimate_tokens` is used on the raw response body.

## Acceptance-gate result

PASS. The protocol boundary proven real with a second protocol implementation. Existing Anthropic tests pass unchanged. New protocol trait implementation covers all 6 methods. Route-based selection avoids config schema changes. Pass-through handler preserves identity, timing, and ledger without touching optimizations path.

## Next unlocked milestone

M10 — Codex CLI integration.
