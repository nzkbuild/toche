# M04 — Setup Transaction Engine Design

## Frozen scope

Turn `toche setup` into a rerunnable configuration reconciler with:
- Detect → Resolve → Ask → Preview → Validate → Apply → Re-read → Verify → Ownership lifecycle
- `--dry-run` with human and `--json` (schema-versioned machine-readable) output
- Atomic lock preventing concurrent setup runs
- Ownership record persistence
- Transaction rollback on failure
- Non-interactive validation mode for tests/CI
