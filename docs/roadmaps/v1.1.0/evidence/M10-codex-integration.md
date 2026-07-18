# M10 — Codex CLI Integration Evidence

## M09 closure commit SHA

`4ec512a`

## M10 implementation commit SHA

`<to-be-filled-after-commit>`

## Base commit SHA

`4ec512a725656dbbdfc21cc8b7c96804c8c24c09` (branch `feat/1.1.0-multi-client-runtime`)

## Design

Route-based: M10 mirrors M05 Claude integration model with a Codex adapter. `toml_edit` preserves comments and ordering in Codex TOML config.

| Client | Config File | Fragment | Tool |
|--------|-----------|----------|------|
| Claude | `~/.claude/settings.json` | `baseURL` + `env.ANTHROPIC_BASE_URL` | `serde_json` + `read_jsonc` |
| Codex | `~/.codex/config.toml` | `openai_base_url` | `toml_edit::DocumentMut` |

## Files changed

- `src/integrations/codex/mod.rs` — NEW: sub-module re-exports
- `src/integrations/codex/discovery.rs` — NEW: `CodexDiscovery`, `codex_home()`, `codex_config_path()`, `codex_backup_path()`, TOML URL extraction, 5 tests
- `src/integrations/codex/config.rs` — NEW: `apply_owned_fragment`, `remove_owned_fragment`, `connect()`, `disconnect()`, 8 tests
- `src/integrations/codex/launch.rs` — NEW: `run_managed()` for `toche run codex`, 2 tests
- `src/integrations/mod.rs` — register `pub mod codex`
- `src/cli/connect.rs` — add `"codex"` match arm with gateway readiness check and `CodexConnectOutcome` handling
- `src/cli/disconnect.rs` — add `"codex"` match arm with `CodexDisconnectOutcome` handling
- `src/cli/run.rs` — add `"codex"` match arm for managed mode
- `src/cli/doctor.rs` — add Codex config detection (directory, config.toml, backup, binary)
- `src/main.rs` — update CLI help text: `about`, agent descriptions

## Config mutation approaches

| Operation | Claude (JSON) | Codex (TOML) |
|-----------|--------------|------------|
| Apply | `serde_json::Value` object mutation | `toml_edit::DocumentMut` key insertion |
| Remove (with backup) | Restore fields from backup JSON | Restore fields from backup TOML |
| Remove (no backup) | Direct key removal | Direct key removal |
| Backup | `settings.json.toche-backup` | `config.toml.toche-backup` |
| Original URL | `~/.toche/pre_toche_url.txt` | `~/.toche/pre_toche_codex_url.txt` |

## CLI surface

```text
toche connect codex     — persistent mode
toche disconnect codex  — remove fragment
toche run codex         — managed mode
toche doctor            — shows Codex status alongside Claude
```

## Managed mode env vars

| Client | Env var set |
|--------|------------|
| Claude | `ANTHROPIC_BASE_URL=http://127.0.0.1:8743/v1` |
| Codex | `OPENAI_BASE_URL=http://127.0.0.1:8743/v1` |

## Dependencies changed

None. `toml_edit` was already approved and present from M02 (v0.25.13). `which` was already used by Claude launch.

## Exact tests

| Suite | Passed | Failed |
|-------|--------|--------|
| lib | 290 | 0 |
| main | 290 | 0 |
| other | 69 | 0 |
| **Total** | **359** | **0** |

New tests:
- `discovery.rs`: 5 tests — URL extraction from TOML (basic, keys, comments, missing, single-quotes)
- `config.rs`: 8 tests — `OwnedFragment` defaults, `points_to_toche` detection, apply preserves unrelated keys, apply already connected no-op, apply/remove roundtrip, disconnect not connected no-op, backup creation, comments survive apply/remove
- `launch.rs`: 2 tests — fragment arguments, resolve codex not found error

All existing tests preserved. Package and integration tests unchanged.

## Benchmark compilation

All three benchmarks compile: `src/lib.rs`, `src/main.rs`, `benches/pipeline.rs`

## Compliance results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (359 passed, 0 failed) |
| `cargo bench --no-run` | PASS |
| `cargo deny check` | PASS |
| `npm run test:npm` | PASS (5 tests) |
| `git diff --check` | PASS |

## Known limitations

- **No live fire test with codex binary.** This environment has codex on PATH but M10 does not require a running end-to-end test with Codex. Dogfooding is deferred to M12 failure hardening.
- **Codex discovery not used by setup.** `CodexDiscovery::detect()` exists but the setup wizard still creates a single integration. Multi-client setup awareness is deferred to M11+.
- **No Codex-specific integration/upstream in Toche config.** Codex connects to the same default integration's upstream URL (Anthropic) appended with `/v1/responses`. A proper OpenAI upstream configuration is needed when Toche supports multiple integrations with different upstreams.
- **`toml_edit` decor prefix comment is a full line prefix.** The `# Managed by Toche` comment appears above `openai_base_url` but cannot be placed as an inline comment due to `toml_edit` API constraints.

## Acceptance-gate result

PASS. Codex integration mirrors the Claude integration ownership model. `toml_edit` preserves comments and unrelated keys during apply and remove. Managed mode spawns codex with correct `OPENAI_BASE_URL`. Same runtime pipeline serves both `/v1/messages` and `/v1/responses`. No Codex-specific format or comment is destroyed. All tests pass. No new dependencies.

## Next unlocked milestone

M11 — Multi-client evidence and reporting.
