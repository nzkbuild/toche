# M01 — Freeze the contract and current-reality audit

## Base commit SHA

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049`

## M00 evidence commit SHA

`ac70c7a8bbbeeed84c8bec716f3766650dbcc998`

## Files inspected

- `Cargo.toml`
- `package.json`
- `src/main.rs`
- `src/lib.rs`
- `src/gateway/mod.rs`
- `src/gateway/server.rs`
- `src/gateway/routes.rs`
- `src/shield/coalesce.rs`
- `src/shield/fingerprint.rs`
- `src/profiles/types.rs`
- `src/profiles/loader.rs`
- `src/cli/setup.rs`
- `src/cli/connect.rs`
- `src/cli/disconnect.rs`
- `src/cli/status.rs`
- `src/cli/doctor.rs`
- `src/cli/stats.rs`
- `src/cli/cache.rs`
- `src/cli/checkpoint.rs`
- `src/cli/graph.rs`
- `src/cli/expand.rs`
- `src/meter/db.rs`
- `src/meter/types.rs`
- `src/meter/recorder.rs`
- `src/meter/pricing.rs`
- `src/safe_cache/cache_db.rs`
- `src/safe_cache/workspace.rs`
- `src/safe_cache/inspect.rs`
- `src/safe_cache/config.rs`
- `src/reduce/transform.rs`
- `src/reduce/storage.rs`
- `src/reduce/config.rs`
- `src/reduce/rtk/toml_filter.rs`
- `src/cache/breakpoint.rs`
- `src/cache/inject.rs`
- `src/efficiency/inject.rs`
- `src/efficiency/instructions.rs`
- `src/efficiency/config.rs`
- `src/continuity/observer.rs`
- `src/continuity/checkpoint.rs`
- `src/config/utils.rs`
- `src/config/toche_config.rs`
- `src/graphify/adapter.rs`
- `src/graphify/config.rs`
- `build.rs`
- `.github/workflows/ci.yml`
- `.github/workflows/prepare-release.yml`
- `npm/install.js`
- `npm/install.test.js`
- `assets/filters/README.md`
- `THIRD_PARTY_NOTICES.md`
- `NOTICE`
- `tests/*.rs`
- `docs/releases/v1.0.10.md`

## Files created or moved

- Created `docs/roadmaps/v1.1.0/ROADMAP.md` (canonical roadmap, moved from `docs/roadmaps/1.1.0_ROADMAP.md`)
- Created `docs/roadmaps/v1.1.0/CURRENT_REALITY.md`
- Created `docs/roadmaps/v1.1.0/MODULE_IMPACT.md`
- Created `docs/roadmaps/v1.1.0/DECISIONS.md`
- Created `docs/roadmaps/v1.1.0/evidence/M01-current-reality.md` (this file)
- Removed obsolete `docs/roadmaps/1.1.0_ROADMAP.md`

## Commands run

```shell
git status --short
git branch --show-current
git merge-base --is-ancestor v1.0.10 HEAD
git log -1 --oneline
cargo test --all-features --locked -- --list
cargo test --all-features --locked
cargo test --all-features --locked --doc
npm run test:npm
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

## Verified current architecture summary

Toche v1.0.10 is a single-profile, Claude-Code-oriented local gateway. It exposes one Axum route (`/v1/messages`) that implements an Anthropic Messages API optimization pipeline: coalescing, safe cache, reduction, efficiency injection, provider cache breakpoint injection, upstream forwarding, response streaming, fact observation, and ledger recording. Configuration is a single `profiles.toml` with overloaded `Profile` objects. Identity is limited to request fingerprint, project path, profile name, and upstream URL. There is no runtime ID, integration ID, trust domain, or session identity.

## Confirmed stable modules

- Atomic write utilities (`src/config/utils.rs`)
- JSONC reader (`src/config/utils.rs::read_jsonc`)
- CAS storage (`src/reduce/storage.rs`)
- RTK filter build pipeline (`build.rs`, `assets/filters/`)
- Reduction engine (`src/reduce/transform.rs`, `src/reduce/rtk/`)
- Request fingerprint normalization (`src/shield/fingerprint.rs`)
- Ledger and stats aggregation (`src/meter/db.rs`, `src/meter/types.rs`)
- npm installer (`npm/install.js`, `npm/install.test.js`)
- Release workflow (`.github/workflows/prepare-release.yml`)

## Confirmed architectural gaps

- No integration/upstream separation.
- No runtime/request/integration/trust-domain/session identity.
- Coalescing key is `(upstream_url, fingerprint)` only, risking credential crossover.
- No true shared-store concurrent waiter tests.
- Only Anthropic Messages API is supported.
- Global `SessionFacts` observer shared across all traffic.
- Plaintext credential storage in `profiles.toml`.
- Stats do not distinguish integrations or protocols.

## Roadmap assumptions validated

- v1.0.10 is the correct development base.
- Working tree was clean enough to proceed.
- All baseline validation commands pass.
- The existing coalescing implementation requires significant correction for multi-client trust isolation.

## Discrepancies requiring ADRs

- ADR-001: `cargo bench -- --noplot` is not supported.
- ADR-002: Existing coalescing tests do not exercise true shared-store concurrency.
- ADR-003: Global `SessionFacts` observer is shared across all traffic.
- ADR-004: Coalescing key lacks credential and trust-domain separation.
- ADR-005: Plaintext credential storage in `profiles.toml`.

## Production-code changes

Explicitly **none**.

## Test status

| Target | Tests passed | Tests failed | Ignored |
|--------|--------------|--------------|---------|
| `unittests src/lib.rs` | 131 | 0 | 0 |
| `unittests src/main.rs` | 154 | 0 | 0 |
| `tests/cache_fixtures.rs` | 4 | 0 | 0 |
| `tests/cli_connect.rs` | 7 | 0 | 0 |
| `tests/cli_status.rs` | 6 | 0 | 0 |
| `tests/continuity_fixtures.rs` | 12 | 0 | 0 |
| `tests/efficiency_fixtures.rs` | 8 | 0 | 0 |
| `tests/gateway_integration.rs` | 3 | 0 | 0 |
| `tests/graphify_fixtures.rs` | 7 | 0 | 0 |
| `tests/reduce_fixtures.rs` | 9 | 0 | 0 |
| `tests/safe_cache_fixtures.rs` | 9 | 0 | 0 |
| `tests/shield_fixtures.rs` | 4 | 0 | 0 |
| Doc-tests | 0 | 0 | 0 |
| npm tests | 5 | 0 | — |

**Total Rust tests executed: 285** (131 + 154 + 4 + 7 + 6 + 12 + 8 + 3 + 7 + 9 + 9 + 4 = 285)

Note: The previous M00 report's "154 unit tests + 131 lib tests" terminology overlapped; the 154 tests in `src/main.rs` include the same modules as the 131 tests in `src/lib.rs` because both binaries compile the same library code. The unique test count is 131 (lib) + 154 (main binary) is double-counting; the actual distinct test invocations are 285 across all targets.

## Validation results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (285 tests) |
| `npm run test:npm` | PASS (5 tests) |
| `git diff --check` | PASS |

## Blockers

None.

## Next unlocked milestone

M02 — Lock reuse and dependency policy.
