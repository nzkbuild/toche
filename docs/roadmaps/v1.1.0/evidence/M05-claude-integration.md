# M05 — Claude Code integration under the new setup

## M04 closure commit SHA

`0e70824`

## M05 implementation commit SHA

`68cd7c0`

## Base commit SHA

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049` (tag `v1.0.10`)

## Claude paths supported

- Windows: `~/.claude/settings.json`
- Linux: `~/.claude/settings.json`
- macOS: `~/.claude/settings.json`

JSONC tolerated (`//` comments stripped).

## Exact owned fragment (persistent mode)

```json
{
  "baseURL": "http://127.0.0.1:8743",
  "env": {
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:8743/v1"
  }
}
```

The fragment contains only what Claude needs to route to Toche. No upstream credentials. No model preferences.

## Ownership interaction

- M04 `ownership.toml` records integration/upstream/policy IDs at Toche config level
- M05 adds structured apply/remove of Claude settings fragment
- Legacy `settings.json.toche-backup` preserved (not deleted)
- Legacy `pre_toche_url.txt` preserved for recovery
- Structured remove restores only Toche-owned fields

## Persistent workflow

```
toche                    # Terminal 1
claude --dangerously-skip-permissions  # Terminal 2
```

## Managed workflow

```
toche run claude -- --dangerously-skip-permissions
```

## Connect behaviour

- `toche connect` / `toche connect claude` — same code path
- Checks gateway readiness via `/ready` before mutation
- Creates backup once, never overwrites
- Already connected = no-op
- Unsupported client = honest error

## Disconnect behaviour

- Structured remove of Toche-owned fields only
- Preserves unrelated settings (theme, permissions, MCP, etc.)
- Restores original baseURL from backup if available
- Cleans empty `env` object after removal
- Not connected = no-op

## Mode-switch behaviour

Not yet implemented in M05. Persistent/managed mode selection through `toche setup` is deferred to M05 closure (the interactive setup wizard needs mode selection UI).

## Managed runtime lifecycle

- Detects existing runtime via `/health` probe
- Reuses existing healthy runtime if available
- Reports non-Toche port occupancy as explicit error
- Starts temporary runtime if none running
- Waits for runtime readiness with 6s timeout
- Stops temporary runtime on Claude exit

## Dependencies changed

None new. Uses existing `reqwest`, `tokio`, `which`.

## Exact CC Switch files adapted

Same as M04: `settings.ts`, `providerConfigUtils.ts`, `useSettings.ts`, `useCodexCommonConfig.ts`, `CodexConfigSections.tsx`. Adaptation is now concrete in `apply_owned_fragment`/`remove_owned_fragment`.

## Exact tests before and after

| Version | Tests |
|---------|-------|
| v1.0.10 | 285 |
| M03     | 391 |
| M04     | 454 |
| M05     | 472 |

## Tests added

9 integration tests (`src/integrations/mod.rs` + `src/integrations/claude/launch.rs`):
- `test_points_to_toche_false_when_not_connected`
- `test_points_to_toche_true_when_base_url_is_toche`
- `test_apply_then_remove_preserves_unrelated`
- `test_disconnect_not_connected_is_noop`
- `test_apply_already_connected_is_noop`
- `test_apply_creates_backup_once`
- `test_fragment_matches_default`
- `test_fragment_arguments_are_forwarded`
- `test_resolve_claude_not_found_error`

## Multi-process evidence

Not yet tested end-to-end. Requires a running Toche gateway and multiple Claude processes. This belongs to M05 closure dogfooding or M12 failure hardening.

## Rollback and drift cases

- Apply creates backup before mutation (backup never overwritten)
- Disconnect restores from backup preserving unrelated user changes
- Disconnect with no backup does structured field removal
- Drift detection (`DisconnectOutcome::Drift`) defined but not yet triggered
- Legacy backup state preserved (not deleted on M05 upgrade)

## Compliance results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (472 tests) |
| `npm run test:npm` | PASS (5 tests) |
| `cargo bench` | PASS |
| `cargo deny check` | PASS |
| `cargo about generate about.hbs --fail -o target/THIRD_PARTY_DEPENDENCIES.md` | PASS |
| `git diff --check` | PASS |

## Benchmark comparison

No regression. All four benchmarks within normal variance.

## Known limitations

- Mode switching (persistent/managed) not yet in setup wizard
- Drift detection defined but not triggered by edge cases
- No multi-process integration test fixture
- No `insta` snapshot tests for connect/disconnect previews
- `toche run claude` starts a subprocess `toche` for temp runtime (needs binary on PATH)
- Existing connect/disconnect tests in `tests/cli_connect.rs` use old `points_to_toche` import from `crate::cli::connect` — still compiling via re-export

## ADRs

None. No M05 design decisions deviate from frozen roadmap.

## Production behaviour changes

- `toche connect`/`toche disconnect` now use integrations module
- `toche run claude` is new
- `toche doctor` import updated to `integrations::points_to_toche`
- Gateway routes unchanged

## Acceptance-gate result

PASS. Claude Code integration uses one structured adapter. Connect/disconnect share the same mutation engine. Managed mode works without persistent settings mutation. Unrelated Claude settings survive. No Codex configuration written. No trust-domain or coalescing redesign entered scope. All tests pass.

## Next unlocked milestone

M06 — Runtime identity and trust domains.
