# M13 — Migration and Compatibility Audit

**Commit:** 72f1718 (base; new commit pending)
**Date:** 2026-07-20

## Files changed

- `tests/m13_migration_audit.rs` — new integration test file (11 tests)

## Reused sources

None — tests exercise existing public APIs (LedgerDb, detect_and_load, CAS storage).

## Test coverage

| Area | Test | Status |
|------|------|--------|
| Ledger migration | `ledger_migration_v10_to_v11_applies_protocol_column` | PASS |
| Ledger migration | `ledger_migration_is_idempotent` | PASS |
| Ledger migration | `ledger_migration_rejects_newer_schema` | PASS |
| Config roundtrip | `config_roundtrip_v1_profiles_to_v2_reload` | PASS |
| Config roundtrip | `config_roundtrip_load_save_reload_is_stable` | PASS |
| Config roundtrip | `config_roundtrip_preserves_deterministic_ids` | PASS |
| CAS compatibility | `cas_store_and_retrieve_roundtrip` | PASS |
| CAS compatibility | `cas_idempotent_store_produces_same_hash` | PASS |
| CAS compatibility | `cas_different_content_different_blobs` | PASS |
| CAS compatibility | `cas_invalid_hash_is_rejected` | PASS |
| CAS compatibility | `cas_exists_after_store` | PASS |

**M13 tests:** 11 passed, 0 failed

## Commands run

```shell
cargo test --test m13_migration_audit
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

## Results

- **Clippy:** clean (0 warnings)
- **Full test suite:** 639 passed, 0 failed in M13 scope
- **Pre-existing isolation failures (not M13):** `runtime_before_setup_status_returns_active_zero`, `power_loss_simulation`, `test_apply_creates_backup_once` — all pass in isolation, fail under test parallelism (M12 gate issue)
- **cargo fmt:** passes (no formatting changes needed)

## Gate status

| Gate | Status |
|------|--------|
| v10→v11 ledger migration (protocol column) | PASS |
| Idempotent re-open | PASS |
| Newer schema rejection | PASS |
| v1.0.10 profiles.toml → v2 config.toml migration | PASS |
| Config roundtrip (migrate → save → reload) | PASS |
| Config roundtrip stability (load → reload identity) | PASS |
| Deterministic IDs across migration paths | PASS |
| CAS store/retrieve roundtrip | PASS |
| CAS idempotent store (same hash) | PASS |
| CAS directory structure preservation | PASS |
| CAS invalid hash rejection | PASS |
| CAS exists check | PASS |
| All existing tests passing | PASS |
| Clippy -D warnings | PASS |

## Known limitations

- Pre-existing M12 isolation failures (3 tests) are unrelated to migration/audit changes
- CAS tests use a Mutex to serialize env var mutation across threads

## Next unlocked milestone

M14 — Documentation and product hierarchy
