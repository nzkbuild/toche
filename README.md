<p align="center">
  <img src="assets/branding/toche-logo.png" alt="Toche" width="200">
</p>

# Toche

**Local context-efficiency gateway for Claude Code.**

Toche sits between Claude Code and the Anthropic API, making every request cheaper
without sacrificing quality. It fingerprints requests to prevent duplicate API calls,
manages prompt caching automatically, reduces tool output before it consumes your
context window, tunes model behaviour per profile, stores safe responses for cross-session
replay — all on your machine, with zero external dependencies at runtime.

---

## Highlights

- **Deduplication** — SHA-256 canonical request fingerprinting + single-flight
  coalescing means concurrent identical requests hit the API once and share the
  response.
- **Persistent safe cache** — Safe responses (text-only, no tool_use) are stored
  in SQLite + content-addressed storage for cross-session replay. Workspace
  fingerprinting prevents replay across different code states.
- **Automatic prompt caching** — Breakpoint detection and `cache_control` injection
  without manual config. Observe mode logs what *would* be cached before you
  commit to it.
- **Tool output reduction** — 63 built-in filters from RTK pattern-match command
  output (Cargo, Git, npm/pnpm, pytest, ruff, go, docker, etc.) and strip noise
  before it hits Claude's context window. Original content stored in CAS for
  recovery via `toche expand <hash>`.
- **Efficiency profiles** — `concise` (terse, no filler) or `careful` (explicit
  assumptions, surgical changes) system-prompt injection per profile.
- **Session continuity** — Checkpoint/goal system preserves working state across
  Claude Code sessions.
- **Usage ledger** — SQLite ledger with per-request token counts, resolved pricing,
  cache-hit breakdown, coalescing stats, reduction savings, and 90-day retention.
  `toche stats` gives human or JSON output.

## Quick start

```bash
# Build
cargo build --release

# Import your existing Claude Code config
./target/release/toche setup

# Point Claude Code at Toche (verify with doctor)
./target/release/toche connect
./target/release/toche doctor

# Run the gateway
./target/release/toche
```

Then use Claude Code normally. Toche proxies requests transparently.

## CLI Reference

| Command | What it does |
|---------|--------------|
| `toche` (no args) | Start the gateway on `127.0.0.1:8743` |
| `toche setup` | Generate `profiles.toml` from Claude Code config |
| `toche setup --force` | Regenerate, backing up existing `profiles.toml` |
| `toche connect` | Route Claude Code through Toche |
| `toche disconnect` | Restore Claude Code to direct upstream |
| `toche doctor` | Show config status and integration health |
| `toche status` | Show gateway status |
| `toche stats` | Usage and cost breakdown (human-readable) |
| `toche stats --json` | Machine-readable output |
| `toche stats --entries 100` | Last N entries |
| `toche expand <hash>` | Restore original tool output from reduction hash |
| `toche cache inspect` | List persistent safe cache entries |
| `toche cache clear` | Clear cache for current project |
| `toche cache clear --all` | Clear all cache entries |
| `toche cache why <fingerprint>` | Explain cache decision for a fingerprint |
| `toche checkpoint save` | Save a session checkpoint |
| `toche checkpoint list` | List saved checkpoints |
| `toche checkpoint show` | Show latest checkpoint |
| `toche checkpoint delete <id>` | Delete a checkpoint |
| `toche graph query <question>` | Query the knowledge graph |
| `toche graph status` | Show graph node/edge counts |
| `toche graph extract` | Rebuild the knowledge graph |

## How it works

Every Anthropic API request flows through a pipeline:

```
fingerprint → shield → safe_cache → reduce → efficiency → cache → forward → ledger
```

| Stage | What it does |
|-------|--------------|
| **Fingerprint** | SHA-256 over the canonical request body (model, messages, tools, temperature — with cache_control and stream stripped). |
| **Shield** | Single-flight coalescing by `{upstream_url}\|{fingerprint}`. First caller forwards; concurrent callers share the response. |
| **Safe Cache** | Persistent cross-session cache keyed by `(project_path, fingerprint)`. Workspace fingerprint prevents replay across different code states. Unsafe responses (containing tool_use blocks) are rejected. |
| **Reduce** | 63-filter RTK pipeline pattern-matches tool command names (including commands resolved from Bash `input.command`), strips noise, stores original in CAS. |
| **Efficiency** | Appends `concise` or `careful` system prompt instruction block when configured. |
| **Cache** | Injects `cache_control` breakpoints into system prompt and consecutive non-tool message runs (Standard) or system prompt only (SystemOnly). |
| **Forward** | Proxies the (possibly transformed) request to the upstream API, parses cache-hit response headers. |
| **Ledger** | Fire-and-forget SQLite recording: model, tokens, cache hits, coalescing, reduction savings, latency, cost estimate. |

## Bypass Headers

Set any header to `true` (case-insensitive) to skip that pipeline stage per-request.
The umbrella header overrides all individual bypasses.

| Header | Skips |
|--------|-------|
| `x-toche-bypass` | All stages (raw forward) |
| `x-toche-bypass-shield` | Request coalescing |
| `x-toche-bypass-safe-cache` | Persistent cache lookup and store |
| `x-toche-bypass-reduce` | Tool output reduction |
| `x-toche-bypass-efficiency` | Instruction injection |
| `x-toche-bypass-cache` | Ephemeral prompt cache injection |

## Configuration

Toche profiles live in `~/.toche/profiles.toml`. Example:

```toml
default = "default"

[[profiles]]
name = "default"
upstream_url = "https://api.anthropic.com"
auth_method = { type = "api_key", header_name = "x-api-key", key = "sk-ant-..." }

[profiles.cache]
enabled = true
mode = "auto"
breakpoint = "standard"

[profiles.reduce]
enabled = true

[profiles.efficiency]
mode = "concise"

[profiles.safe_cache]
enabled = true
ttl_days = 30
max_entry_bytes = 1048576

[profiles.graphify]
enabled = false
```

`toche setup` generates this from your existing Claude Code configuration.

## Troubleshooting

**Gateway won't start:**
- Check port 8743 is free: nothing else should be listening there
- Verify `~/.toche/profiles.toml` exists and is valid TOML: `toche doctor`
- Run with debug logging: `RUST_LOG=toche=debug toche`

**Cannot connect:**
- Ensure the gateway is running first (run `toche` in one terminal)
- Run `toche connect` in another terminal after the gateway is listening

**Stats show nothing:**
- The ledger records only requests that go through the gateway. Run `toche connect`
  first, then use Claude Code normally.

**Cache entries not appearing:**
- Safe cache only caches text-only responses (no tool_use blocks). Use `toche cache why <fingerprint>`
  to see why a response was rejected.

**API errors after disconnect:**
- Run `toche doctor` to check routing state. If `env.ANTHROPIC_BASE_URL` still points
  to Toche but the gateway is stopped, manually edit `~/.claude/settings.json`.

## Requirements

- Rust 1.85+ (edition 2024)
- SQLite (bundled via `rusqlite`)
- Claude Code (or any Anthropic Messages API client)

No external services. No API keys of its own. Everything runs locally.

## Documentation

- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — Full pipeline, module map, database schema, CAS layout
- [CHANGELOG.md](CHANGELOG.md) — Release history v0.1.0 through v1.0.5
- [BUG_TRACKER.md](docs/BUG_TRACKER.md) — Bugs found and fixed during dogfooding

## License

Apache License 2.0. See [LICENSE](LICENSE).

Third-party attribution in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
