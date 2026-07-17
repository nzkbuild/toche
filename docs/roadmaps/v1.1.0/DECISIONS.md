# Toche 1.1.0 — Architecture Decision Register

## ADR-001 — Benchmark CLI does not accept `--noplot`

- **Status:** Accepted
- **Date:** 2026-07-17
- **Milestone:** M00
- **Discovered evidence:** Running `cargo bench -- --noplot` failed with `error: Unrecognized option: 'noplot'`. The `pipeline` benchmark uses Criterion's default CLI, which on this version does not expose `--noplot`.
- **Original roadmap assumption:** The M00 baseline command list did not mention `--noplot`; this flag was introduced by the audit team informally.
- **Proposed correction:** Run `cargo bench` without extra flags. Criterion emits a `Gnuplot not found, using plotters backend` warning but completes successfully.
- **Alternatives considered:**
  - Install gnuplot to suppress the warning — unnecessary for baseline validation.
  - Patch Criterion invocation — out of scope for M01.
- **Safety impact:** None.
- **Compatibility impact:** None.
- **Scope impact:** None; documentation only.
- **Recommendation:** Record this as a known baseline behaviour; do not add `--noplot` to future release audit commands.
- **Owner decision:** Accepted.

## ADR-002 — Existing coalescing tests do not exercise true shared-store concurrency

- **Status:** Accepted — gap to close in M07
- **Date:** 2026-07-17
- **Milestone:** M01
- **Discovered evidence:** `src/shield/coalesce.rs` test `concurrent_waiter_gets_coalesced` spawns a waiter on a *fresh* `CoalesceStore`, so leader and waiter never share the same store. No other test shares a store across runtimes or tasks.
- **Original roadmap assumption:** M07 assumes the current coalescing implementation has real concurrent-waiter coverage.
- **Proposed correction:** Treat M07 as requiring new true-concurrency tests, possibly with `loom`, and do not rely on existing tests for shared-store behaviour.
- **Alternatives considered:**
  - Refactor existing test to share store — insufficient; M07 needs comprehensive concurrency verification.
  - Accept current tests as adequate — rejected; they do not prove the property.
- **Safety impact:** Medium; undetected race conditions in coalescing could affect multi-client isolation.
- **Compatibility impact:** None for M01.
- **Scope impact:** Adds explicit concurrency-test work to M07.
- **Recommendation:** Add `loom` modelled tests and real async shared-store tests in M07.
- **Owner decision:** Accepted.

## ADR-003 — Global `SessionFacts` observer is shared across all traffic

- **Status:** Accepted — gap to close in M06/M07
- **Date:** 2026-07-17
- **Milestone:** M01
- **Discovered evidence:** `src/continuity/observer.rs` uses a single global `LazyLock<Mutex<SessionFacts>>`. In a multi-client runtime, facts from one client/session will leak into another's checkpoint.
- **Original roadmap assumption:** M06 requires per-integration/session identity and isolation.
- **Proposed correction:** Replace global observer with a runtime-scoped or request-context-scoped fact collector keyed by integration/session.
- **Alternatives considered:**
  - Keep global observer and filter by integration at drain time — more complex and error-prone.
- **Safety impact:** Medium; cross-client fact leakage violates trust isolation.
- **Compatibility impact:** Existing checkpoint tests must continue to pass.
- **Scope impact:** Adds identity plumbing to continuity module.
- **Recommendation:** Introduce request-scoped observer in M06.
- **Owner decision:** Accepted.

## ADR-004 — Coalescing key lacks credential and trust-domain separation

- **Status:** Accepted — gap to close in M07
- **Date:** 2026-07-17
- **Milestone:** M01
- **Discovered evidence:** `src/shield/coalesce.rs` key is `format!("{}|{}", upstream_url, fingerprint)`. Different credentials to the same URL with the same body will coalesce.
- **Original roadmap assumption:** M07 requires trust-safe concurrency.
- **Proposed correction:** Include `upstream_id` and `trust_domain_id` in the flight key.
- **Alternatives considered:**
  - Include raw credential hash — rejected; raw credentials must not appear in IDs/hashes.
- **Safety impact:** High; credential crossover is a security risk.
- **Compatibility impact:** Existing coalescing behaviour changes only for multi-credential scenarios.
- **Scope impact:** Core M07 work.
- **Recommendation:** Implement trust-domain-aware flight key in M07.
- **Owner decision:** Accepted.

## ADR-005 — Plaintext credential storage in `profiles.toml`

- **Status:** Accepted — deferred beyond 1.1.0
- **Date:** 2026-07-17
- **Milestone:** M03/M04
- **Discovered evidence:** `src/profiles/types.rs` stores `AuthMethod::ApiKey { key: String, ... }` and `BearerToken { token: String }`, serialized into TOML.
- **Original roadmap assumption:** M03 defers native keyring storage; supports environment variables, command/helper references, and legacy values during migration.
- **Proposed correction:** Keep plaintext during migration; introduce `SecretRef` abstraction that can resolve env vars and helper commands. Keyring remains deferred.
- **Alternatives considered:**
  - Require keyring in 1.1.0 — rejected; roadmap defers it.
- **Safety impact:** Medium; existing plaintext remains, but no new plaintext is introduced.
- **Compatibility impact:** Must preserve existing credentials through migration.
- **Scope impact:** Adds `SecretRef` design without keyring implementation.
- **Recommendation:** Implement `SecretRef` with env/command/legacy support in M03.
- **Owner decision:** Accepted.
