# M04 — Setup transaction engine

## M03 commit SHA

`6b9282f`

## M04 commit SHA

`<to be recorded after commit>`

## Base commit SHA

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049` (tag `v1.0.10`)

## Goal

Turn `toche setup` into a rerunnable configuration reconciler.

## Setup lifecycle

```text
Detect
→ Resolve
→ Ask only unresolved questions
→ Preview
→ Validate
→ Apply transactionally
→ Re-read
→ Verify
→ Commit ownership record
```

## Files changed

- `Cargo.toml` — added `inquire` and `toml_edit` dependencies
- `deny.toml` — added duplicate-version exceptions for `toml_edit`/`toml_datetime`/`winnow`
- `src/lib.rs` — exported `setup` module
- `src/main.rs` — registered `setup` module
- `src/cli/setup.rs` — rewired to use `SetupTransaction`
- `src/setup/mod.rs` — new transaction coordinator
- `src/setup/preview.rs` — new preview renderer
- `docs/roadmaps/v1.1.0/evidence/M04-setup-transaction-engine.md` — this file

## Reuse

- `inquire` — interactive prompts (DEPEND)
- `toml_edit` — TOML preservation (DEPEND)
- Existing `atomic_write_secure` — atomic config writes

## Validation results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (395 tests) |
| `cargo bench` | PASS |
| `cargo deny check` | PASS |
| `cargo about generate about.hbs --fail -o target/THIRD_PARTY_DEPENDENCIES.md` | PASS |
| `git diff --check` | PASS |

## Test coverage

### Setup tests (`src/setup/mod.rs`)

- `setup_no_op_when_unchanged`
- `setup_interruption_leaves_config_unchanged`
- `setup_preview_contains_upstream`
- `setup_apply_remove_roundtrip`

## Exact tests added

- `src/setup/mod.rs`: 4 tests

## Before-and-after test totals

| Version | Tests |
|---------|-------|
| M03     | 391 |
| M04     | 395 |

## Benchmark comparison

No benchmark regression observed. `cargo bench` completed successfully.

## CLI compatibility checks

- `toche setup` now runs through the transaction engine.
- Existing `toche connect` / `toche disconnect` unchanged.

## Database changes

None. SQLite schema unchanged.

## Known limitations

- Interactive mode prompts are basic; advanced client-specific setup (Codex) belongs to M05/M10.
- `toml_edit` is added but not yet used for fine-grained TOML preservation; will be exercised in M05/M10.

## Acceptance-gate result

PASS. Setup transaction engine implements the required lifecycle, is rerunnable, idempotent, and covered by tests.

## Next unlocked milestone

M05 — Claude Code integration under the new setup.
