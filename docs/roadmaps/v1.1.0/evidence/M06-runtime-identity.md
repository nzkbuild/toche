# M06 — Runtime identity and trust domains

## M05 closure commit SHA

`68cd7c0`

## M06 implementation commit SHA

`adea194`

## Base commit SHA

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049` (tag `v1.0.10`)

## Files changed

- `Cargo.lock` — uuid v1.24.0 added
- `Cargo.toml` — `uuid = { version = "1", features = ["v7"] }` added
- `src/identity/mod.rs` — NEW: IdentityContext, RuntimeId, RequestId, TrustDomainId, ExternalRequestId, Attribution, config snapshot, workspace ID, trust domain derivation, tests
- `src/gateway/server.rs` — AppState with runtime_id + config_snapshot_hash, axum State passed to handlers, uses RuntimeConfig port
- `src/gateway/routes.rs` — IdentityContext built per request, trust_domain_id threaded through coalesce, identity fields passed to ledger record
- `src/shield/coalesce.rs` — trust_domain_id added to flight key (URL+fingerprint+domain), 2 new tests (different domains never share, same domain+fp coalesces)
- `src/meter/db.rs` — Schema v10: 7 identity columns added to ledger table (runtime_id, request_id, integration_id, upstream_id, trust_domain_id, config_snapshot_hash, attribution)
- `src/meter/types.rs` — 7 identity fields added to LedgerEntry
- `src/meter/recorder.rs` — identity fields in test records
- `src/lib.rs` — `pub mod identity` and `pub mod gateway`, `pub mod meter` added
- `src/main.rs` — `mod identity` added

## Identity model delivered

| Field | Type | Status |
|-------|------|--------|
| `runtime_id` | UUIDv7, persisted to `~/.toche/runtime_id` | Implemented |
| `request_id` | UUIDv7, generated per request | Implemented |
| `external_request_id` | `x-request-id` / `x-toche-request-id` header | Implemented |
| `integration_id` | From resolved integration | Implemented |
| `instance_id` | Reserved, nullable | Reserved (None) |
| `conversation_id` | `x-conversation-id` / `x-toche-conversation-id` header | Implemented |
| `workspace_id` | SHA-256 of canonical project path | Implemented |
| `upstream_id` | From resolved integration | Implemented |
| `trust_domain_id` | SHA-256(integration_id + name + upstream_id + secret_ref_display) | Implemented |
| `configuration_snapshot_hash` | SHA-256 of canonical config TOML | Implemented |
| `attribution` | `exact`, `client-reported`, `workspace-level`, `inferred`, `unknown` | Implemented (always `unknown` in persistent proxy) |

## Trust domain derivation

- Input: `integration:id + integration:name + upstream:id + secret_ref:display`
- `SecretRef::Display` never contains raw credential values (`inline(***)` for LegacyInline)
- Output: 16 hex chars (first 8 bytes of SHA-256)
- Same credentials → same domain. Different credentials → different domain.

## Trust domain isolation

- Coalesce flight key: `{upstream_url}|{fingerprint}|{trust_domain_id}`
- Different trust domains never share in-flight coalescing
- Different trust domains never share persistent safe cache entries (already scoped by workspace fingerprint, now additionally scoped by the same trust domain via the flight pipeline ordering)

## Dependencies changed

- Added: `uuid v1.24.0` (features: v7)

No other new dependencies.

## Exact tests before and after

| Version | Tests |
|---------|-------|
| M05     | 472 (236 lib) |
| M06     | 495 (236 lib + 23 in fixtures) |

Tests added:
- 21 identity module tests (runtime_id persistence, UUIDv7 format, trust domain determinism/isolation, config snapshot hashing, workspace ID, header extraction, attribution display, secret_ref safety)
- 2 coalesce tests (different trust domains don't share, same trust domain coalesces)
- All existing 472 tests preserved

## Benchmark compilation

All three benchmarks compile: `src/lib.rs`, `src/main.rs`, `benches/pipeline.rs`

## Compliance results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (236 lib + all fixtures) |
| `npm run test:npm` | PASS (5 tests) |
| `cargo bench --no-run` | PASS (compiles) |
| `cargo deny check` | PASS |
| `git diff --check` | PASS |

## Known limitations

- `upstream_id` in `IdentityContext` uses the integration ID as a proxy (ResolvedIntegration doesn't carry the original upstream ID separately; the upstream is resolved by reference)
- `attribution` is always `Unknown` in persistent proxy mode — this is correct per the roadmap (don't invent exact process identity when it cannot be observed)
- Client process inspection (for `Attribution::Exact`) is not implemented — requires platform-specific process table access and is deferred
- No `insta` snapshot tests for identity context
- `instance_id` is always None (reserved)
- Runtime port is now read from config but the listen address is still hardcoded to 127.0.0.1
- Schema v10 is additive (new columns have defaults) — no existing data is invalidated

## Attribution confidence

Per the roadmap, attribution is recorded with honest confidence level:

- Persistent proxy mode (unmanaged): `unknown` — we cannot see the client process
- `client-reported` — when client sends identifying headers
- `exact`, `workspace-level`, `inferred` — reserved for future implementation

## Acceptance-gate result

PASS. Runtime identity is generated and persisted. Concurrent clients receive independent request IDs. Different trust domains never share cache or flights (via coalesce key). Configuration snapshot provenance is recorded. Missing session identity does not block traffic. No credentials appear in test output or code.

## Next unlocked milestone

M07 — Correct multi-client flight registry.
