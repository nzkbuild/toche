<p align="center">
  <img src="assets/branding/toche-logo.png" alt="Toche" width="200">
</p>

# Toche

**Local context-efficiency gateway for Claude Code.**

Toche sits between Claude Code and the Anthropic API, making every request cheaper
without sacrificing quality. It fingerprints requests to prevent duplicate API calls,
manages prompt caching automatically, reduces tool output before it consumes your
context window, and tunes model behaviour per profile — all on your machine, with
zero external dependencies at runtime.

---

## Highlights

- **Deduplication** — SHA-256 canonical request fingerprinting + single-flight
  coalescing means concurrent identical requests hit the API once and share the
  response.
- **Automatic prompt caching** — Breakpoint detection and `cache_control` injection
  without manual config. Observe mode logs what *would* be cached before you
  commit to it.
- **Tool output reduction** — Pattern-matched truncation/summarisation of known
  noisy tools (Cargo, Git, test runners, linters) before results hit Claude's
  context window. Lossless content-addressed storage so you can restore original
  output with `toche expand <hash>`.
- **Efficiency profiles** — `concise` (terse, no filler) or `careful`
  (explicit assumptions, surgical changes) system-prompt injection per profile,
  with `x-toche-bypass-efficiency` for per-request override.
- **Usage ledger** — SQLite ledger with per-request token counts, resolved pricing,
  cache-hit breakdown, coalescing stats, and 90-day retention. `toche stats` gives
  you human or JSON output.

## Quick start

```bash
# Build
cargo build --release

# Import your existing Claude Code config
./target/release/toche setup

# Point Claude Code at Toche
./target/release/toche connect

# Verify
./target/release/toche doctor

# Run the gateway
./target/release/toche
```

Then use Claude Code normally. Toche proxies requests transparently.

```bash
# Check what Toche is doing
toche stats
toche stats --json
toche stats --entries 100

# Restore original tool output from a reduction hash
toche expand a1b2c3d4...

# Go back to direct upstream
toche disconnect
```

## How it works

Every Anthropic API request flows through a pipeline:

```
fingerprint → shield → reduce → efficiency → cache → forward
```

| Stage | What it does |
|-------|--------------|
| **Fingerprint** | SHA-256 over the canonical request body (model, messages, tools, temperature — with cache_control and stream stripped). |
| **Shield** | Single-flight coalescing by fingerprint. First caller forwards; concurrent callers wait and share the response. |
| **Reduce** | RTK-based filter pipeline. Pattern-matches tool command names against built-in filters, truncates/summarises stdout, stores original content keyed by SHA-256 hash. |
| **Efficiency** | Appends `concise` or `careful` system prompt instruction block when configured. Converts system string to content-block array if needed. |
| **Cache** | Injects `cache_control` breakpoints into system prompt and consecutive non-tool message runs (Standard breakpoint) or system prompt only. |
| **Forward** | Proxies the (possibly transformed) request to the upstream API, parses cache-hit response headers, and records everything in the ledger. |

## Configuration

Toche profiles live in your Claude Code config directory. Example `profiles.toml`:

```toml
default = "anthropic"

[[profiles]]
name = "anthropic"
upstream_url = "https://api.anthropic.com/v1/messages"
auth_method = { type = "api_key", header_name = "x-api-key", key = "sk-ant-..." }

[profiles.cache]
enabled = true
mode = "auto"
breakpoint = "standard"

[profiles.reduce]
enabled = true

[profiles.efficiency]
mode = "concise"
```

`toche setup` generates this from your existing Claude Code configuration.

## Requirements

- Rust 1.85+ (edition 2024)
- SQLite (bundled via `rusqlite`)
- Claude Code (or any Anthropic Messages API client)

No external services. No API keys of its own. Everything runs locally.

## License

Apache License 2.0. See [LICENSE](LICENSE).

Third-party attribution in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
