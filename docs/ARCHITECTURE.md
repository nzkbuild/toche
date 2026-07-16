# Toche Architecture

## Pipeline

Each request passes through a staged pipeline:

```
Request → Fingerprint → Shield → Safe Cache → Reduce → Efficiency → Cache → Upstream → Ledger
```

### Stage 1: Fingerprint (`src/shield/fingerprint.rs`)

Computes a deterministic SHA-256 hash of the normalized JSON request body. This
fingerprint is used for coalescing, safe cache lookup, and cache deduplication.

### Stage 2: Request Shield (`src/shield/coalesce.rs`)

Single-flight coalescing via broadcast channels. Identical in-flight requests
(by `{upstream_url}|{fingerprint}`) share one upstream call — the first creates
the flight, subsequent waiters receive the same response. This protects against
Claude Code's parallel retry behavior.

### Stage 3: Safe Cache (`src/safe_cache/`)

Persistent cross-session cache keyed by `(project_path, fingerprint)`. Unlike
the ephemeral cache layer, safe cache survives restarts. Repository workspace
fingerprint prevents replay across different code states. Unsafe responses
(containing tool_use blocks) are rejected. Cached responses are stored in
content-addressed storage (`~/.toche/cas/`).

### Stage 4: Reduce (`src/reduce/`)

Applies TOML-defined filter pipelines to tool result content blocks in the
request body. Reduces token count by stripping noise from command outputs
(compilation progress, passing tests, download bars, etc.). Original content
is preserved in CAS for `toche expand` recovery. The committed inventory has
65 built-in filters: 63 definitions imported from the pinned RTK source plus
Toche-owned Cargo and Git diff filters.

### Stage 5: Efficiency (`src/efficiency/`)

Injects instruction text into the system prompt to guide model behavior.
Three modes: normal (no injection), concise (shorter responses), careful
(verbatim preservation).

### Stage 6: Cache (`src/cache/`)

Ephemeral Anthropic prompt cache injection. In observe mode, logs breakpoint
opportunities. In auto mode, injects `cache_control` breakpoints into the
system prompt and consecutive non-tool message runs to maximize cache hits.

### Forward (`src/gateway/routes.rs`)

Proxies the (possibly modified) request to the configured upstream and
collects the SSE response body.

### Ledger (`src/meter/`)

Fire-and-forget SQLite recording of every request: model, tokens, cache
hit tokens, coalescing, reduction savings, latency, cost estimate.
`toche stats` reads this ledger for human or JSON output.

## Module Map

```
src/
├── main.rs            — CLI routing (setup, connect, doctor, stats, ...)
├── gateway/           — Axum HTTP server, routes, health/ready endpoints
├── shield/            — Fingerprinting + request coalescing
├── safe_cache/        — Persistent cache DB + workspace fingerprint
├── reduce/            — TOML filter engine + request transform + CAS storage
├── efficiency/        — Instruction injection for concise/careful modes
├── cache/             — Ephemeral prompt cache breakpoint injection
├── meter/             — Ledger DB, pricing, request recording
├── continuity/        — Session checkpoint save/restore
├── graphify/          — Knowledge graph CLI adapter
├── profiles/          — TOML profile loading, types, config resolution
├── config/            — Shared utilities: atomic_write, JSONC parsing, home_dir
└── cli/               — User-facing commands: connect, disconnect, stats, ...
```

## Database Schema

All databases live in `~/.toche/ledger.db` (SQLite, WAL mode):

**ledger** — Every forwarded request:
- model, profile_name, input_tokens, output_tokens
- cache_read_input_tokens, cache_creation_input_tokens
- coalesced_count, latency_ms, status, cost
- reduction_input_tokens, reduction_output_tokens, reduction_count
- efficiency_mode, local_cache_hit, project_path

**safe_cache** — Persistent cross-session cache entries:
- project_path, fingerprint, workspace_fingerprint
- response_hash (CAS pointer), model, status
- tokens_input, tokens_output, hit_count

**cache_rejects** — Cache rejection audit trail:
- project_path, fingerprint, reason

**checkpoints** — Session continuity snapshots:
- project_path, goal, completed, next, changed_files, verification

**schema_version** — Forward-compatible migration tracking.

## CAS Storage

Content-addressed storage at `~/.toche/cas/<first2>/<remaining>` keyed by
SHA-256 hex digest. Stores:
- Raw unreduced tool outputs (for `toche expand` recovery)
- Cached response bodies (for safe cache replay)

## Config Files

| File | Purpose |
|------|---------|
| `~/.toche/profiles.toml` | Upstream URL, auth, feature flags per profile |
| `~/.claude/settings.json` | Claude Code settings (modified by `toche connect`) |
| `~/.claude/settings.json.toche-backup` | Pre-Toche backup (connect creates, disconnect restores) |

Environment: `TOCHE_CONFIG_DIR` overrides the `~/.toche/` path.

## Bypass Headers

All bypass headers accept `true` (case-insensitive). The umbrella header
overrides all individual bypasses.

| Header | Skips |
|--------|-------|
| `x-toche-bypass` | All stages (raw forward) |
| `x-toche-bypass-shield` | Request coalescing |
| `x-toche-bypass-safe-cache` | Persistent cache lookup/store |
| `x-toche-bypass-reduce` | Tool output reduction |
| `x-toche-bypass-efficiency` | Instruction injection |
| `x-toche-bypass-cache` | Ephemeral prompt cache injection |
