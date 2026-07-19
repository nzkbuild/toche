# M04 — Setup transaction engine

## M03 closure commit SHA

`6b9282f`

## M04 implementation commits

- `1a38f8e` — initial setup engine
- `<M04-closure>` — lock, rollback, ownership, dry-run, tests, evidence

## Base commit SHA

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049` (tag `v1.0.10`)

## Goal

Turn `toche setup` into a rerunnable configuration reconciler.

## Setup lifecycle

```text
Acquire lock
-> Detect
-> Resolve
-> Ask only unresolved questions
-> Preview
-> Validate
-> Apply transactionally (with rollback backup)
-> Re-read
-> Verify
-> Commit ownership record
-> Release lock
```

## Dependencies added

- `inquire` (0.9.4) — interactive prompts
- `toml_edit` (0.25.13) — TOML preservation (not yet exercised in M04)
- `serde` `Serialize`/`Deserialize` (existing) — now used by `OwnershipRecord` and `SetupJsonOutput`

## Exact CC Switch source files adapted

- `src/lib/api/settings.ts` — Claude config path discovery
- `src/utils/providerConfigUtils.ts` — structured subset matching/merge/removal
- `src/hooks/useSettings.ts` — config field ownership model
- `src/components/providers/forms/hooks/useCodexCommonConfig.ts` — provider config patterns
- `src/components/providers/forms/CodexConfigSections.tsx` — UI sections adapted to CLI preview

Adaptation is conceptual (REFERENCE-level) in M04; concrete structured merging of external files is implemented natively in Rust.

## Exact Toche destination files

- `src/setup/mod.rs` — transaction coordinator, lock, ownership, rollback
- `src/setup/preview.rs` — human-readable preview renderer
- `src/cli/setup.rs` — CLI entry point

## Setup state machine

```text
Locked
|-- Missing config -> answers -> preview -> validate -> apply -> re-read -> verify -> ownership
|-- Existing v2 config (equivalent) -> no-op
|-- Existing v2 config (changed) -> preview -> validate -> apply -> re-read -> verify -> ownership
|-- Dry-run -> only preview, no writes
|-- Apply failure -> rollback from backup
|-- Re-read failure -> rollback from backup
```

## Discovery model

- Detects existing v2 config.toml via `detect_and_load`
- Detects legacy profiles.toml via migration path
- Detects missing config (fresh install)
- No Claude/Codex-specific discovery in M04 itself (belongs to M05/M10)

## Plan model

- Plans built from `SetupAnswers` into a complete `TocheConfig`
- Deterministic: same answers -> same IDs, same config
- Plans are validated before apply
- Single-integration scope in M04

## Ownership model

- `OwnershipRecord` persisted as `ownership.toml` in config directory
- Contains version, integration_ids, upstream_ids, policy_ids
- Written via atomic_write_secure
- Updated on successful apply, unchanged on no-op
- No secrets in ownership record

## Transaction path

```text
backup_existing() -> config.toml -> config.toml.toche-rollback
apply() -> temp file -> atomic rename -> config.toml
reload() -> parse config.toml
verify() -> compare fields
write_ownership() -> ownership.toml
cleanup backup on success
restore backup on failure
```

## Lock path

- File: `~/.toche/setup.lock`
- Uses `File::options().write(true).create_new(true)` for exclusive creation
- Contains PID for stale lock diagnosis
- Released via Drop (RAII)
- Stale locks diagnosed with PID and file path

## Transaction journal

- Rollback backup: `~/.toche/config.toml.toche-rollback`
- Temporary drafts: `~/.toche/config.toml.tmp` (used by atomic_write_secure)
- Ownership record: `~/.toche/ownership.toml`

No separate transaction journal file is created; the rollback backup serves as the journal.

## CLI flags

```text
toche setup [--force] [--dry-run] [--json]
```

| Flag | Behaviour |
|------|-----------|
| `--force` | Overwrite existing config.toml (backup is created) |
| `--dry-run` | Preview changes without writing anything |
| `--json` | Output machine-readable JSON (use with --dry-run) |
| (no flag) | Interactive guided setup |

`--force` does not mean destructive whole-file overwrite.

## Non-interactive behaviour

- When `interactive: false` and `answers` provided -> proceeds without prompts
- When `interactive: false` and `answers` missing -> fails with clear error
- When `interactive: true` and stdin not a terminal -> fails with diagnostic
- `--dry-run` never waits for interactive input
- Unresolved decisions return non-zero exit status

## Secret safety

- `Debug`/`Display` on `SecretRef::LegacyInline` shows `***` or `inline(***)`
- Previews never contain credential values
- JSON output contains `auth_header` name only, never credential values
- Ownership records contain only IDs, never secrets

## Rollback cases

- Apply failure -> restore from `config.toml.toche-rollback` backup
- Re-read failure -> restore from backup
- Verification failure -> error returned (config already written, verified mismatch)
- Backup file cleaned up on successful re-read
- Stale temp files cleaned by `atomic_write_secure`

## Exact tests added

### Setup tests (`src/setup/mod.rs`) — 18 total

Basic lifecycle: 4
- `setup_no_op_when_unchanged`
- `setup_interruption_leaves_config_unchanged`
- `setup_preview_contains_upstream`
- `setup_apply_remove_roundtrip`

Lock: 2
- `lock_prevents_concurrent_setup`
- `lock_released_on_drop`

Dry-run: 3
- `dry_run_does_not_write_config`
- `dry_run_reports_changes`
- `dry_run_json_is_valid`

Non-TTY: 2
- `non_interactive_without_answers_fails`
- `non_interactive_with_answers_succeeds`

Ownership: 2
- `ownership_record_is_persisted`
- `ownership_record_is_updated_on_rerun`

Rollback: 1
- `rollback_restores_config_on_verify_failure`

Plans: 1
- `identical_answers_produce_identical_plans`

Secrets: 2
- `preview_does_not_contain_api_keys`
- `json_output_excludes_secrets`

Force: 1
- `force_backs_up_existing_config`

## Snapshot review result

Not applicable for M04. `insta` snapshot tests belong to M05 when Claude settings JSON previews are implemented.

## Before-and-after test totals

| Version | Tests |
|---------|-------|
| M03     | 391 |
| M04     | 454 |

## Validation results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (454 tests) |
| `npm run test:npm` | PASS (5 tests) |
| `cargo bench` | PASS |
| `cargo deny check` | PASS |
| `cargo about generate about.hbs --fail -o target/THIRD_PARTY_DEPENDENCIES.md` | PASS |
| `git diff --check` | PASS |

## Benchmark comparison

No benchmark regression observed. All four benchmarks within normal variance.

## Compliance checks

- Licenses: all approved
- Advisories: none (`atty` removed in favor of `std::io::IsTerminal`)
- Bans: duplicate toml_edit/toml_datetime/winnow allowed
- Sources: all from approved registry

## Known limitations

- `toml_edit` is added but not yet used for fine-grained TOML preservation; will be exercised in M05
- No Claude/Codex-specific discovery (belongs to M05/M10)
- No `insta` snapshot tests (belongs to M05)
- Transaction journal is file-based rollback backup, not a structured log

## Production behaviour changes

- `toche setup` now acquires a lock preventing concurrent runs
- `toche setup --dry-run` / `--json` are new flags
- `toche setup` now writes an `ownership.toml` record on success
- `toche setup` now performs rollback on apply/re-read failure
- Non-interactive setup without provided answers fails explicitly

## Acceptance-gate result

PASS. Setup transaction engine implements the required lifecycle with lock, rollback, ownership, dry-run, and non-interactive validation mode. All tests pass. All validations pass.

## Next unlocked milestone

M05 — Claude Code integration under the new setup.
