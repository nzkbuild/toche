# M05 — Claude Code Integration Design

## Frozen scope

Make Claude Code the first fully managed client under schema-v2 config + M04 transaction engine.

## Claude settings reality (from v1.0.10)

- Path: `~/.claude/settings.json` (JSONC: `//` comments stripped on read)
- Fields Toche owns: `baseURL`, `env.ANTHROPIC_BASE_URL`
- Fields Toche preserves: themes, permissions, hooks, MCP config, model preferences, custom env, experimental settings
- Auth helper: `apiKeyHelper` (e.g. `env:ANTHROPIC_API_KEY`)
- Backup: `settings.json.toche-backup` (whole-file)
- Saved original URL: `~/.toche/pre_toche_url.txt`

## Toche-owned fragment (persistent mode)

Minimal fragment added to Claude settings:

```json
{
  "baseURL": "http://127.0.0.1:8743",
  "env": {
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:8743/v1"
  }
}
```

No upstream credentials. No model preferences. No unrelated fields.

## Module structure

```text
src/integrations/
  mod.rs           — IntegrationAdapter trait
  claude/
    mod.rs         — Claude adapter
    discovery.rs   — Claude installation + settings discovery
    config.rs      — Owned fragment apply/remove/verify
    launch.rs      — Managed launch (toche run claude)
```

## Persistent workflow

```
1. Read Claude settings.json (JSONC-tolerant)
2. Parse into Value
3. Check if already connected (baseURL points to Toche)
4. If connected and healthy -> no-op
5. Backup if no existing backup
6. Set baseURL = http://127.0.0.1:8743
7. Set env.ANTHROPIC_BASE_URL = http://127.0.0.1:8743/v1
8. Save original upstream URL
9. Atomic write
10. Re-read + verify
11. Update ownership record
```

## Managed workflow

```
1. Resolve Claude executable
2. Build child environment with Toche endpoint
3. Detect existing runtime or start temporary
4. Spawn child with forwarded args
5. Propagate exit code
6. Stop temporary runtime only if we started it
```

## Disconnect (ownership-aware)

```
1. Read current settings.json
2. Check if connected (baseURL points to Toche)
3. If not connected -> report and exit
4. If backup exists -> structured restore (preserving unrelated user changes)
5. If no backup -> structured remove owned fields
6. Clean empty env object
7. Atomic write
8. Re-read + verify absence
```

## CLI surface (M05)

```text
toche run claude [-- <claude args>]
toche connect [claude]
toche disconnect [claude]
toche setup  (now with persistent/managed mode selection)
```
