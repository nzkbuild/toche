# M10 — Codex CLI Integration Design

## Frozen scope

Configure Codex to use Toche's `/v1/responses` ingress. Mirror M05 Claude integration ownership model using `toml_edit` for comment-preserving TOML mutation.

## Codex configuration reality

- Path: `~/.codex/config.toml` (or `$CODEX_HOME/config.toml`)
- Format: TOML with comments and arbitrary key ordering
- Key Toche owns: `openai_base_url`
- Backup: `config.toml.toche-backup`
- Saved original URL: `~/.toche/pre_toche_codex_url.txt`

## Toche-owned fragment (persistent mode)

Minimal fragment added to Codex config.toml:

```toml
# Managed by Toche — do not edit directly.
openai_base_url = "http://127.0.0.1:8743/v1"
```

No upstream credentials. No model preferences. No unrelated keys.

## Module structure

```text
src/integrations/
  mod.rs           — registered pub mod codex
  codex/
    mod.rs         — sub-module re-exports
    discovery.rs   — Codex installation + config discovery
    config.rs      — Owned fragment apply/remove/verify via toml_edit
    launch.rs      — Managed launch (toche run codex)
```

## Persistent workflow

```
1. Read Codex config.toml
2. Parse with toml_edit::DocumentMut (comments survive)
3. Check if already connected (openai_base_url contains 127.0.0.1:8743)
4. If connected -> no-op
5. Backup if no existing backup
6. Set openai_base_url = http://127.0.0.1:8743/v1
7. Save original upstream URL to ~/.toche/pre_toche_codex_url.txt
8. Atomic write
9. Update ownership record
```

## Managed workflow

```
1. Resolve Codex executable (which codex)
2. Build child environment with OPENAI_BASE_URL pointing to Toche
3. Detect existing runtime or start temporary
4. Spawn codex with forwarded args
5. Propagate exit code
6. Stop temporary runtime only if we started it
```

## Disconnect (ownership-aware)

```
1. Read current config.toml
2. Check if connected (points to Toche)
3. If not connected -> report and exit
4. If backup exists -> structured restore (preserving unrelated user changes)
5. If no backup -> structured remove openai_base_url
6. Preserve all other keys and comments
7. Atomic write
```

## CLI surface (M10)

```text
toche run codex [-- <codex args>]
toche connect codex
toche disconnect codex
toche doctor  (now shows Codex status)
```

## Design decisions

1. **`toml_edit` over raw string manipulation.** Codex config.toml may contain comments, inline comments, and user-specific formatting. `toml_edit::DocumentMut` preserves all of these when only `openai_base_url` is modified.

2. **Separate connect/disconnect outcome types.** Claude uses `ConnectOutcome`/`DisconnectOutcome` shared in `integrations/mod.rs`. Codex uses `CodexConnectOutcome`/`CodexDisconnectOutcome` because each adapter has different payload shapes. A shared trait would add coupling without benefit at this stage.

3. **`OPENAI_BASE_URL` env var for managed mode.** Codex reads `OPENAI_BASE_URL` from the environment. Managed mode sets it to route through Toche without modifying any files.

4. **No multi-client setup awareness yet.** Setup still creates a single integration. Future milestones (M11+) will make `toche setup` detect and configure multiple clients.
