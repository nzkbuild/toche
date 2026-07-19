# M08 — Protocol-driver boundary evidence

## M07 closure commit SHA

`58db889`

## M08 implementation commit SHA

`a68238b`

## Base commit SHA

`807e9ce` (branch `feat/1.1.0-multi-client-runtime`)

## Files changed

- `src/protocol/mod.rs` — NEW: `Protocol` trait + `ResponseHeaders` struct
- `src/protocol/anthropic/mod.rs` — NEW: `AnthropicProtocol` implementation with 16 tests
- `src/gateway/routes.rs` — wire trait calls, remove standalone `extract_model()` and `is_streaming_request()` and their 11 tests
- `docs/roadmaps/v1.1.0/design/M08-protocol-boundary.md` — NEW: design document

## Trait surface

| Method | Responsibility | Delegates to |
|--------|---------------|-------------|
| `extract_model` | Parse "model" from JSON | inline serde_json |
| `path` | Return "/v1/messages" | constant |
| `fingerprint` | Normalize + hash request body | `shield::fingerprint::compute` |
| `parse_response_headers` | Extract Anthropic cache header tokens | inline |
| `inject_cache_control` | Inject ephemeral cache breakpoints | `cache::inject::inject_cache_control` |
| `is_streaming` | Check for `"stream": true` | inline serde_json |

## Dependencies changed

None. No new dependencies.

## Exact tests before and after

| Version | Tests |
|---------|-------|
| M07     | 587 (259 lib + 259 main + 69 other) |
| M08     | 587 (259 lib + 259 main + 69 other) |

Tests moved:
- 7 `extract_model` tests: routes.rs → protocol/anthropic/mod.rs
- 4 `is_streaming_request` tests: routes.rs → protocol/anthropic/mod.rs
- 5 new protocol-specific tests: path, fingerprint delegation, header parsing (2), fingerprint determinism

All existing non-moved tests preserved. Pipeline behaviour unchanged.

## Benchmark compilation

All three benchmarks compile: `src/lib.rs`, `src/main.rs`, `benches/pipeline.rs`

## Compliance results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (587 passed, 0 failed) |
| `cargo bench --no-run` | PASS |
| `cargo deny check` | PASS |
| `git diff --check` | PASS |

## Known limitations

- **Protocol selection is hardcoded**: routes.rs always uses `AnthropicProtocol`. M09 will add runtime protocol selection based on integration configuration.
- **Efficiency injection is not protocol-routed**: The efficiency module's `inject_efficiency()` function appends to Anthropic `system` content blocks directly. This stays in routes.rs as pipeline logic per the thin-trait decision. M09 will need to handle protocol-aware efficiency or disable it for non-Anthropic protocols.

## Acceptance-gate result

PASS. Existing Anthropic tests pass unchanged. Protocol trait is thin and stateless. Unknown fields survive (no protocol-specific parsers drop unrecognized fields). fingerprint delegates to existing shield implementation. Pipeline (coalescing, safe cache, reduce, efficiency, ledger) stays in routes.rs.

## Next unlocked milestone

M09 — OpenAI Responses ingress.
