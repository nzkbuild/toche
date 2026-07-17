# Toche 1.1.0 — Milestone-to-Module Impact Map

## Legend

- **DB impact:** None / Migration / Schema addition
- **Config impact:** None / Migration / External-file patch
- **Compatibility risk:** Low / Medium / High

## Dependency graph

```text
M00 ──► M01 ──► M02 ──► M03 ──► M04 ──► M05 ──► M06 ──► M07 ──► M08 ──► M09 ──► M10 ──► M11 ──► M12 ──► M13 ──► M14 ──► M15 ──► M16
       ▲       ▲       ▲       ▲       ▲       ▲       ▲       ▲       ▲       ▲       ▲       ▲       ▲       ▲       ▲
       │       │       │       │       │       │       │       │       │       │       │       │       │       │       │
       └───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┘
       (each milestone depends on all earlier gates)
```

No milestone may be started before its predecessor passes its done gate.

## M02 — Lock reuse and dependency policy

| Field | Value |
|-------|-------|
| Existing modules touched | `Cargo.toml`, `package.json`, `build.rs`, `THIRD_PARTY_NOTICES.md`, `assets/filters/README.md` |
| New modules anticipated | `docs/roadmaps/v1.1.0/REUSE_LOCK.md`, `docs/roadmaps/v1.1.0/REUSE_MANIFEST.toml`, `deny.toml`, `.cargo/about.toml` |
| Stable modules reused | Existing RTK filter build pipeline; existing npm installer |
| External reuse involved | `inquire`, `toml_edit`, `uuid`, `tower-http`, `wiremock-rs`, `insta`, `proptest`, `loom`, `cargo-deny`, `cargo-about` |
| Database impact | None |
| Config impact | None |
| Compatibility risk | Low — adds tooling only |
| Primary tests required | `cargo deny check`, `cargo about generate`, `cargo test` |
| Dependencies on earlier milestones | M01 current-reality audit |
| Out-of-scope traps | Do not add whole-gateway dependencies; do not change runtime behaviour |

## M03 — Configuration schema separation

| Field | Value |
|-------|-------|
| Existing modules touched | `src/profiles/types.rs`, `src/profiles/loader.rs`, `src/cli/setup.rs`, `src/gateway/routes.rs`, `src/meter/db.rs`, `src/meter/types.rs` |
| New modules anticipated | `src/config/runtime.rs`, `src/config/integration.rs`, `src/config/upstream.rs`, `src/config/policy.rs`, `src/config/storage.rs`, `src/config/migrate.rs` |
| Stable modules reused | Atomic write utilities; profile loader path logic |
| External reuse involved | `toml_edit` (DEPEND) for TOML preservation |
| Database impact | Migration: ledger `profile_name` → `integration_id`/`upstream_id`; add config snapshot table |
| Config impact | Migration: `profiles.toml` → new schema; backup old file |
| Compatibility risk | High — existing `profiles.toml` must migrate deterministically |
| Primary tests required | Fresh config parse; 1.0.10 config migration; migration idempotency; failed migration rollback |
| Dependencies on earlier milestones | M02 reuse lock |
| Out-of-scope traps | Do not introduce keyring storage; do not drop legacy field support |

## M04 — Setup transaction engine

| Field | Value |
|-------|-------|
| Existing modules touched | `src/cli/setup.rs`, `src/cli/connect.rs`, `src/cli/disconnect.rs`, `src/cli/doctor.rs` |
| New modules anticipated | `src/setup/transaction.rs`, `src/setup/detect.rs`, `src/setup/preview.rs`, `src/setup/apply.rs` |
| Stable modules reused | JSONC reader; atomic writes; `read_jsonc`/`atomic_write` |
| External reuse involved | `inquire` (DEPEND); adapted CC Switch config algorithms (ADAPT) |
| Database impact | None |
| Config impact | External-file patch: Claude Code `settings.json`, Codex `settings.json`, Toche config |
| Compatibility risk | Medium — must preserve existing backups and not double-backup |
| Primary tests required | Setup no-op; setup interruption; preview snapshots; apply/remove round-trip |
| Dependencies on earlier milestones | M03 config schema |
| Out-of-scope traps | Do not send paid model requests; do not modify Graphify/checkpoints in standard onboarding |

## M05 — Claude Code integration under the new setup

| Field | Value |
|-------|-------|
| Existing modules touched | `src/cli/connect.rs`, `src/cli/disconnect.rs`, `src/gateway/routes.rs` |
| New modules anticipated | `src/integration/claude.rs`, `src/integration/persistent.rs`, `src/integration/managed.rs` |
| Stable modules reused | Existing connect/disconnect logic; backup/restore semantics |
| External reuse involved | None new |
| Database impact | None |
| Config impact | Migration: existing Claude profile → integration/upstream |
| Compatibility risk | Medium — existing two-terminal workflow must remain intact |
| Primary tests required | Existing connect/disconnect tests; two simultaneous Claude clients; managed mode |
| Dependencies on earlier milestones | M04 setup transaction |
| Out-of-scope traps | Do not remove persistent mode; do not break backup semantics |

## M06 — Runtime identity and trust domains

| Field | Value |
|-------|-------|
| Existing modules touched | `src/gateway/routes.rs`, `src/shield/coalesce.rs`, `src/shield/fingerprint.rs`, `src/safe_cache/cache_db.rs`, `src/meter/db.rs` |
| New modules anticipated | `src/identity/mod.rs`, `src/identity/trust_domain.rs`, `src/identity/snapshot.rs` |
| Stable modules reused | UUIDv7 dependency; existing fingerprinting |
| External reuse involved | `uuid` (DEPEND) |
| Database impact | Schema addition: runtime_id, request_id, integration_id, upstream_id, trust_domain_id, snapshot_hash columns |
| Config impact | None |
| Compatibility risk | Medium — must not break existing ledger queries |
| Primary tests required | UUID generation; independent request IDs; trust-domain isolation; no credentials in IDs/hashes |
| Dependencies on earlier milestones | M05 Claude integration |
| Out-of-scope traps | Do not invent exact process identity in persistent proxy mode |

## M07 — Correct multi-client flight registry

| Field | Value |
|-------|-------|
| Existing modules touched | `src/shield/coalesce.rs`, `src/shield/fingerprint.rs`, `src/gateway/routes.rs` |
| New modules anticipated | `src/flight/mod.rs`, `src/flight/registry.rs`, `src/flight/key.rs`, `src/flight/leader.rs` |
| Stable modules reused | Existing fingerprint normalization |
| External reuse involved | `loom` (DEV DEPEND) for concurrency verification |
| Database impact | None |
| Config impact | None |
| Compatibility risk | High — coalescing behaviour changes materially |
| Primary tests required | True concurrent waiter tests; leader panic cleanup; waiter cancellation; slow waiter isolation |
| Dependencies on earlier milestones | M06 trust domains |
| Out-of-scope traps | Do not enable streaming coalescing until fan-out proven |

## M08 — Protocol-driver boundary

| Field | Value |
|-------|-------|
| Existing modules touched | `src/gateway/routes.rs`, `src/cache/breakpoint.rs`, `src/cache/inject.rs`, `src/efficiency/inject.rs`, `src/reduce/transform.rs`, `src/safe_cache/inspect.rs`, `src/continuity/observer.rs` |
| New modules anticipated | `src/protocol/mod.rs`, `src/protocol/anthropic/mod.rs`, `src/protocol/anthropic/fingerprint.rs`, `src/protocol/anthropic/transform.rs` |
| Stable modules reused | Existing Anthropic logic moved behind interface |
| External reuse involved | None new |
| Database impact | None |
| Config impact | None |
| Compatibility risk | Medium — must preserve byte-equivalent pass-through |
| Primary tests required | Existing Anthropic tests pass; unknown-field property tests; raw pass-through equivalence |
| Dependencies on earlier milestones | M07 flight registry |
| Out-of-scope traps | Do not add cross-protocol translation |

## M09 — OpenAI Responses ingress

| Field | Value |
|-------|-------|
| Existing modules touched | `src/gateway/server.rs`, `src/gateway/routes.rs`, `src/meter/db.rs` |
| New modules anticipated | `src/protocol/openai_responses/mod.rs`, `src/protocol/openai_responses/routes.rs`, `src/protocol/openai_responses/types.rs` |
| Stable modules reused | Flight registry; trust-domain isolation; ledger |
| External reuse involved | Official Codex protocol types/fixtures (ADAPT/VENDOR) |
| Database impact | Schema addition: protocol column in ledger |
| Config impact | None |
| Compatibility risk | Medium — new route and protocol handling |
| Primary tests required | Non-streaming forwarding; streaming forwarding; SSE ordering; unknown-field preservation; concurrent Anthropic traffic |
| Dependencies on earlier milestones | M08 protocol boundary |
| Out-of-scope traps | Do not enable Toche output reduction or cross-protocol coalescing |

## M10 — Codex CLI integration

| Field | Value |
|-------|-------|
| Existing modules touched | `src/cli/setup.rs`, `src/cli/connect.rs`, `src/cli/disconnect.rs`, `src/integration/` |
| New modules anticipated | `src/integration/codex.rs`, `src/cli/run.rs` |
| Stable modules reused | Setup transaction engine; TOML preservation |
| External reuse involved | `toml_edit` (DEPEND) |
| Database impact | None |
| Config impact | External-file patch: Codex `settings.json` |
| Compatibility risk | Medium — must preserve Codex comments and unrelated providers |
| Primary tests required | Codex setup idempotency; two Codex sessions; Claude + Codex simultaneous |
| Dependencies on earlier milestones | M09 OpenAI Responses ingress |
| Out-of-scope traps | Do not add managed mode as a second implementation |

## M11 — Multi-client evidence and reporting

| Field | Value |
|-------|-------|
| Existing modules touched | `src/cli/status.rs`, `src/cli/stats.rs`, `src/meter/db.rs`, `src/meter/types.rs` |
| New modules anticipated | `src/evidence/mod.rs`, `src/evidence/collector.rs`, `src/stats/aggregator.rs` |
| Stable modules reused | Existing ledger aggregation |
| External reuse involved | Portkey Models pricing snapshot (VENDOR) |
| Database impact | Schema addition: integration/protocol/upstream/trust-domain columns |
| Config impact | None |
| Compatibility risk | Low — additive reporting changes |
| Primary tests required | Status shows multi-client state; stats distinguish protocols; missing prices remain unknown |
| Dependencies on earlier milestones | M10 Codex integration |
| Out-of-scope traps | Do not fabricate usage or prices |

## M12 — Failure hardening

| Field | Value |
|-------|-------|
| Existing modules touched | All runtime modules |
| New modules anticipated | `src/failure_matrix.md` (documentation), new integration tests |
| Stable modules reused | Existing error handling |
| External reuse involved | `wiremock-rs` (DEV DEPEND) for failure simulation |
| Database impact | None |
| Config impact | None |
| Compatibility risk | Medium — may change failure modes |
| Primary tests required | Failure matrix tests for listed scenarios |
| Dependencies on earlier milestones | M11 evidence |
| Out-of-scope traps | Do not add excluded features under guise of hardening |

## M13 — Migration and compatibility audit

| Field | Value |
|-------|-------|
| Existing modules touched | `src/config/migrate.rs`, `src/meter/db.rs`, `src/safe_cache/cache_db.rs`, `src/continuity/checkpoint.rs` |
| New modules anticipated | `src/migration/mod.rs`, `src/migration/fixtures.rs` |
| Stable modules reused | Existing schema-version mechanism |
| External reuse involved | None new |
| Database impact | Migration fixtures tested |
| Config impact | 1.0.10 fixture upgrades tested |
| Compatibility risk | High — must not lose user data |
| Primary tests required | Clean and customized 1.0.10 fixture upgrades; partial damage fails safely; no-op rerun |
| Dependencies on earlier milestones | M12 failure hardening |
| Out-of-scope traps | Do not silently discard history or CAS data |

## M14 — Documentation and product hierarchy

| Field | Value |
|-------|-------|
| Existing modules touched | `README.md`, `docs/releases/v1.1.0.md` |
| New modules anticipated | `docs/roadmaps/v1.1.0/README_UPDATES.md` |
| Stable modules reused | Existing README structure |
| External reuse involved | None |
| Database impact | None |
| Config impact | None |
| Compatibility risk | Low |
| Primary tests required | Documentation review; no code changes |
| Dependencies on earlier milestones | M13 migration |
| Out-of-scope traps | Do not delete stable commands for aesthetic reasons |

## M15 — Full release audit

| Field | Value |
|-------|-------|
| Existing modules touched | All |
| New modules anticipated | None |
| Stable modules reused | All |
| External reuse involved | `cargo-deny`, `cargo-about` |
| Database impact | None |
| Config impact | None |
| Compatibility risk | Low |
| Primary tests required | Full matrix: `cargo fmt`, `cargo clippy`, `cargo test`, `npm run test:npm`, `cargo bench`, `cargo deny check`, `cargo about generate` |
| Dependencies on earlier milestones | M14 documentation |
| Out-of-scope traps | Do not bump version before M16 |

## M16 — Version and release preparation

| Field | Value |
|-------|-------|
| Existing modules touched | `Cargo.toml`, `package.json`, `Cargo.lock`, `docs/releases/v1.1.0.md`, `CHANGELOG.md` |
| New modules anticipated | None |
| Stable modules reused | Existing release workflow |
| External reuse involved | None |
| Database impact | None |
| Config impact | None |
| Compatibility risk | Low |
| Primary tests required | Version agreement; release audit rerun; clean npm install on Windows and Unix |
| Dependencies on earlier milestones | M15 full release audit |
| Out-of-scope traps | Do not manually move tags |
