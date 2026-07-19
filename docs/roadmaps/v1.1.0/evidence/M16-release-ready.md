# M16 — Version and Release Preparation

**Date:** 2026-07-20
**Commit:** 0e8a39c57f99c771524caf800b94eb46299f409e
**Branch:** feat/1.1.0-multi-client-runtime

---

## Release readiness checklist

### 1. All M08–M16 milestones completed

| Milestone | Evidence | Status |
|-----------|----------|--------|
| M08 — Protocol-driver boundary | `evidence/M08-protocol-boundary.md` | PASS |
| M09 — OpenAI Responses ingress | `evidence/M09-openai-responses.md` | PASS |
| M10 — Codex CLI integration | `evidence/M10-codex-integration.md` | PASS |
| M11 — Multi-client evidence and reporting | (M12 + M13 acceptance matrix covers this) | PASS |
| M12 — Failure hardening | (M12 test shards, M15 audit confirms) | PASS |
| M13 — Migration and compatibility audit | `evidence/M13-migration-audit.md` | PASS |
| M14 — Documentation and product hierarchy | `evidence/M14-documentation.md` | PASS |
| M15 — Full release audit | `evidence/M15-release-audit.md` | PASS |

### 2. All tests passing

```
cargo test --all-features
  unit tests:   290 passed, 0 failed
  integration:  gateway_integration — 11 passed (1 flaky Windows race, passes retry)
                m12_failure_shard4 — 5 passed (1 flaky Windows race, passes retry)
                All other test targets: 47 passed
  Total: ~348 tests, all green on retry
```

Two flaky tests documented in M15 known limitations (Windows file-locking races). Both pass individually on retry.

### 3. Clippy clean

```
cargo clippy --all-targets --all-features -- -D warnings
  Finished `dev` profile — zero warnings
```

**Result: PASS**

### 4. Cargo audit clean

```
cargo audit
  Loaded 1166 security advisories
  Scanning Cargo.lock (291 crate dependencies)
  0 vulnerabilities found
```

**Result: PASS**

### 5. Cargo deny clean

```
cargo deny check
  advisories ok, bans ok, licenses ok, sources ok
```

**Result: PASS**

### 6. Docs complete

| Document | Exists | Status |
|----------|--------|--------|
| README.md | Yes | Installation, usage, multi-client workflow, client support |
| CHANGELOG.md | Yes | Updated v1.1.0 with release date and summary |
| CONTRIBUTING.md | Yes | Contribution guidelines |
| CODE_OF_CONDUCT.md | Yes | Code of conduct |
| ARCHITECTURE.md | Yes | Full pipeline and module documentation |
| THIRD_PARTY_NOTICES.md | Yes | Generatable via `cargo about` |

### 7. Version bumped

| File | From | To |
|------|------|----|
| Cargo.toml | 1.0.10 | 1.1.0 |
| package.json | 1.0.10 | 1.1.0 |
| Cargo.lock | — | Updated by `cargo check` |
| CHANGELOG.md | Unreleased | 2026-07-20 |

---

## Files changed in this milestone

- `Cargo.toml` — version bump 1.0.10 → 1.1.0
- `Cargo.lock` — updated by cargo
- `package.json` — version bump 1.0.10 → 1.1.0
- `CHANGELOG.md` — release date and final summary for v1.1.0
- `docs/releases/v1.1.0.md` — release notes for GitHub Release
- `docs/roadmaps/v1.1.0/evidence/M16-release-ready.md` — this file

---

## Acceptance gate

| Check | Status |
|-------|--------|
| M08–M16 milestones completed | PASS |
| All tests passing | PASS (2 flaky Windows races, retry-pass) |
| Clippy clean | PASS |
| Cargo audit clean | PASS |
| Docs complete | PASS |
| Version bumped | PASS |

---

## Final recommendation

**RELEASE READY.** All M16 checks pass. Proceed with the existing verified release workflow.
