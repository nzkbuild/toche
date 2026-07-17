# M02 — Lock reuse and dependency policy

## Base commit SHA

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049`

## M01 commit SHA

`d78ae4fb7c853371bc9bab73cd71bc189152ed04`

## Files inspected

- `Cargo.toml`
- `Cargo.lock`
- `package.json`
- `build.rs`
- `THIRD_PARTY_NOTICES.md`
- `NOTICE`
- `assets/filters/README.md`
- `assets/filters/rtk/*.toml`
- `assets/filters/toche/*.toml`
- `.github/workflows/ci.yml`
- `.github/workflows/prepare-release.yml`

## Repositories and crate registries inspected

- https://github.com/rtk-ai/rtk (cloned, `develop` branch)
- https://github.com/ccusage/ccusage (cloned)
- https://github.com/farion1231/cc-switch (cloned)
- https://github.com/SaladDay/cc-switch-cli (cloned)
- https://github.com/openai/codex (cloned)
- https://github.com/Portkey-AI/models (cloned)
- https://github.com/BerriAI/litellm (cloned)
- crates.io metadata for: inquire, toml_edit, uuid, tower-http, wiremock, insta, proptest, loom, cargo-deny, cargo-about

## Pinned revisions

| Source | Revision |
|--------|----------|
| rtk-ai/rtk | `5d32d0736f686b69d1e8b9dc45c007d4eb77a0a2` |
| ccusage/ccusage | `7acee6c5853c26fe66fbe1453bd94c9376afec06` |
| farion1231/cc-switch | `edea624a27d6a94678e0a5c2ddaa674876d9d186` |
| SaladDay/cc-switch-cli | `bca8b9457ca0f6c7e67e4d52fa61e880cbdeeef3` (rejected) |
| openai/codex | `315195492c80fdade38e917c18f9584efd599304` |
| Portkey-AI/models | `72da8a5cbed3db8a7f845c5a64d720d2662ad165` |
| BerriAI/litellm | `4d339648981ceb8c45df3081b388680084a2206d` (rejected) |

## Verified licences

| Source | Licence | Verified via |
|--------|---------|--------------|
| rtk-ai/rtk | Apache-2.0 | cloned `LICENSE` |
| ccusage/ccusage | MIT | cloned `apps/ccusage/LICENSE` |
| farion1231/cc-switch | MIT | cloned `LICENSE` |
| SaladDay/cc-switch-cli | MIT | cloned `LICENSE` |
| openai/codex | Apache-2.0 | cloned `LICENSE` |
| Portkey-AI/models | MIT | cloned `LICENSE` |
| BerriAI/litellm | MIT (with enterprise caveat) | cloned `LICENSE` |

## Approved sources

- rtk-ai/rtk: VENDOR (data)
- ccusage/ccusage: REFERENCE
- Graphify-Labs/graphify: REFERENCE (existing)
- multica-ai/andrej-karpathy-skills: REFERENCE (existing)
- juliusbrussee/caveman: REFERENCE (existing)
- farion1231/cc-switch: ADAPT
- openai/codex: ADAPT / REFERENCE
- Portkey-AI/models: VENDOR (data)
- TensorZero: REFERENCE
- OpenTelemetry GenAI semantic conventions: REFERENCE
- Direct dependencies: inquire 0.9.4, toml_edit 0.25.13, uuid 1.24.0, tower-http 0.7.0, wiremock 0.6.5, insta 1.48.0, proptest 1.11.0, loom 0.7.2
- Tooling dependencies: cargo-deny 0.20.2, cargo-about 0.9.1

## Rejected sources

- SaladDay/cc-switch-cli: heavier Tauri/Rust fork; same original author and licence as primary source.
- BerriAI/litellm for pricing: monolithic file with enterprise licencing caveats; Portkey provides cleaner per-provider JSON.

## Conditional sources

None.

## Current reuse deficiencies corrected

- `THIRD_PARTY_NOTICES.md` listed RTK commit as `5d32d07`; corrected to full SHA `5d32d0736f686b69d1e8b9dc45c007d4eb77a0a2`.
- `Cargo.toml` missing `license` field; added `license = "Apache-2.0"`.

## Direct dependencies approved for later milestones

| Crate | Version | Features | Use |
|-------|---------|----------|-----|
| inquire | 0.9.4 | default | M04 guided setup |
| toml_edit | 0.25.13 | default | M03/M10 TOML preservation |
| uuid | 1.24.0 | v7, serde, rng | M06 runtime/request/session IDs |
| tower-http | 0.7.0 | request-id, trace, timeout | M06/M08 middleware |
| wiremock | 0.6.5 | default | M12 failure simulation (dev) |
| insta | 1.48.0 | json, redactions | M04/M11 snapshots (dev) |
| proptest | 1.11.0 | default | M08 property tests (dev) |
| loom | 0.7.2 | default | M07 concurrency tests (dev) |

## Adaptations approved

- farion1231/cc-switch: config path discovery, structured subset matching/merge/removal, preservation of unrelated configuration.
- openai/codex: OpenAI Responses request structures, SSE event lifecycle, provider configuration expectations, authentication/header behaviour, retry/protocol-error patterns.
- ccusage: grouping and reporting concepts only.

## Data and fixtures approved

- Portkey-AI/models pricing snapshot: vendored compact pricing data for M11.
- OpenAI Codex fixtures: minimal official Codex fixtures for non-streaming responses, SSE streaming, usage, tool calls, errors, malformed streams, unknown fields.
- Toche-owned fixtures for Anthropic edge cases where public protocol documentation is sufficient.

## Reference-only sources

- TensorZero
- OpenTelemetry GenAI semantic conventions
- 9Router, LiteLLM, Bifrost, Portkey Gateway, Envoy AI Gateway

## cargo-deny result

PASS with warnings:

- Warnings for missing clarification files on `hyper-rustls` and `rustls` (non-blocking; crates ship SPDX metadata).
- Warnings for unused licence allowances (non-blocking).
- Duplicate `hashbrown` versions noted (non-blocking; caused by `rusqlite` and `toml_edit` indexmap dependencies).

## cargo-about result

PASS. Generated dependency attribution report successfully with:

```shell
cargo about generate about.hbs --fail -o /tmp/THIRD_PARTY_DEPENDENCIES.md
```

## CI changes

- Added `cargo-deny` and `cargo-about` steps to `.github/workflows/ci.yml` quality job.
- Pinned versions by full semantic version in CI commands.

## ADRs added

None. M02 confirms roadmap assumptions and adds only factual corrections.

## Production runtime changes

Explicitly **none**.

## Validation results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (285 tests) |
| `npm run test:npm` | PASS (5 tests) |
| `cargo deny check` | PASS (with non-blocking warnings) |
| `cargo about generate about.hbs --fail -o /tmp/THIRD_PARTY_DEPENDENCIES.md` | PASS |
| `git diff --check` | PASS |

## Blockers

None.

## Acceptance-gate result

PASS. Every approved source has an exact revision or package version, a verified
licence, a single primary reuse action, and narrow adaptation boundaries. CI
enforces the approved licence policy. No production runtime behaviour changed.

## Next unlocked milestone

M03 — Configuration schema separation.
