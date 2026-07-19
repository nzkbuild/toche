# Toche Architecture

Toche is a local multi-client gateway that sits between AI coding tools and
their upstream API endpoints. This document covers the system design, crate
map, data flow, and architecture decision records for the 1.1.0 release.

## System design

Toche uses a **universal control plane + protocol-specific payload plane**
architecture:

```text
Universal control plane
+
Protocol-specific payload plane
```

The control plane handles shared concepts: runtime identity, request identity,
trust-domain derivation, coalescing, safe cache, reduction, efficiency,
prompt-cache injection, forwarding, and ledger recording. The payload plane
handles protocol-specific request parsing and header interpretation through
the `Protocol` trait.

The original request bytes are authoritative. Unknown fields survive unchanged.
Protocol-specific transformations cannot execute on another protocol.

## Crate map

```
src/
├── main.rs              — CLI routing (Clap derive, tokio main)
├── lib.rs               — Public module declarations
│
├── gateway/             — HTTP server and request routing
│   ├── server.rs        — Axum router, AppState, serve(), health endpoints
│   └── routes.rs        — /v1/messages and /v1/responses handlers + pipeline
│
├── protocol/            — Protocol trait and drivers
│   ├── mod.rs           — Protocol trait + ResponseHeaders
│   ├── anthropic/       — AnthropicProtocol (Messages API)
│   └── openai_responses/— OpenAiResponsesProtocol (Responses API)
│
├── shield/              — Request fingerprinting and coalescing
│   ├── fingerprint.rs   — Deterministic SHA-256 fingerprint
│   └── coalesce.rs      — CoalesceStore (single-flight in-flight dedup)
│
├── identity/            — Runtime and request identity
│   └── mod.rs           — RuntimeId, RequestId, TrustDomainId, IdentityContext
│
├── safe_cache/          — Persistent cross-session response cache
│   ├── cache_db.rs      — SQLite operations
│   ├── config.rs        — SafeCacheConfig
│   ├── inspect.rs       — Response safety checks
│   └── workspace.rs     — Workspace fingerprinting
│
├── reduce/              — Tool-output reduction engine
│   ├── config.rs        — ReduceConfig
│   ├── transform.rs     — Filter application pipeline
│   ├── storage.rs       — CAS storage (store/retrieve)
│   └── rtk/             — RTK-adapted filter engine
│
├── efficiency/          — Instruction injection
│   ├── config.rs        — EfficiencyConfig, EfficiencyMode
│   ├── inject.rs        — System prompt injection
│   └── instructions.rs  — Instruction text per mode
│
├── cache/               — Ephemeral prompt-cache injection
│   ├── breakpoint.rs    — Breakpoint detection
│   └── inject.rs        — cache_control injection
│
├── meter/               — Usage metering and recording
│   ├── db.rs            — LedgerDb (SQLite)
│   ├── pricing.rs       — PricingMap (embedded pricing)
│   ├── recorder.rs      — Request timing and recording
│   └── types.rs         — Data types
│
├── config/              — Configuration management
│   ├── toche_config.rs  — TocheConfig v2 types
│   ├── loader.rs        — Config loading from disk
│   ├── migration.rs     — Schema v1 → v2 migration
│   ├── resolver.rs      — ResolvedIntegration construction
│   └── utils.rs         — atomic_write, home_dir
│
├── setup/               — Interactive setup engine
│   ├── mod.rs           — Setup lifecycle
│   └── preview.rs       — Change preview rendering
│
├── integrations/        — Client integrations
│   ├── claude/          — Claude Code config, discovery, launch
│   └── codex/           — Codex CLI config, discovery, launch
│
├── cli/                 — Command implementations
│   ├── setup.rs, connect.rs, disconnect.rs, run.rs
│   ├── doctor.rs, status.rs, stats.rs, expand.rs
│   ├── cache.rs, checkpoint.rs, graph.rs
│   └── mod.rs
│
├── continuity/          — Session checkpoints
├── graphify/            — Knowledge graph adapter
├── profiles/            — Legacy 1.0.x profile types
└──
```

## Data flow

### Anthropic Messages (`/v1/messages`)

```text
Client request (Anthropic Messages API)
    │
    ▼
┌─────────────────────────────────────────────────┐
│ 1. Protocol dispatch → AnthropicProtocol        │
│    Extract model, compute fingerprint           │
├─────────────────────────────────────────────────┤
│ 2. Request Shield (coalescing)                  │
│    Key: upstream|fingerprint|trust_domain|policy│
│    If another identical request is in-flight    │
│    with the same trust domain and policy → wait │
├─────────────────────────────────────────────────┤
│ 3. Safe Cache (persistent replay)               │
│    Check: project_path + fingerprint match      │
│    Guard: workspace fingerprint, no tool_use    │
├─────────────────────────────────────────────────┤
│ 4. Reduce (tool-output reduction)               │
│    65 TOML-defined filters strip noise from     │
│    tool result content blocks                   │
│    Original stored in CAS for `toche expand`    │
├─────────────────────────────────────────────────┤
│ 5. Efficiency (instruction injection)           │
│    Inject mode-specific instructions into       │
│    system prompt (normal/concise/careful)       │
├─────────────────────────────────────────────────┤
│ 6. Cache (prompt-cache breakpoints)             │
│    Observe mode: log breakpoint opportunities   │
│    Auto mode: inject cache_control breakpoints  │
├─────────────────────────────────────────────────┤
│ 7. Forward → Upstream API                       │
│    Collect SSE response body                    │
├─────────────────────────────────────────────────┤
│ 8. Ledger recording (fire-and-forget)           │
│    SQLite: model, tokens, cost, latency,        │
│    cache hits, reduction, coalescing, identity  │
└─────────────────────────────────────────────────┘
    │
    ▼
Response bytes → Client
```

### OpenAI Responses (`/v1/responses`)

```text
Client request (OpenAI Responses API)
    │
    ▼
┌─────────────────────────────────────────────────┐
│ 1. Protocol dispatch → OpenAiResponsesProtocol  │
│    Extract model, compute fingerprint           │
├─────────────────────────────────────────────────┤
│ 2. Request Shield (coalescing)                  │
│    Key: upstream|fingerprint|trust_domain|policy│
├─────────────────────────────────────────────────┤
│ 3. Forward → Upstream API                       │
│    Pass-through only (no reduction, caching,    │
│    efficiency, or prompt-cache injection)       │
├─────────────────────────────────────────────────┤
│ 4. Ledger recording (fire-and-forget)           │
│    SQLite: model, tokens, cost, latency,        │
│    coalescing, identity, protocol="openai"      │
└─────────────────────────────────────────────────┘
    │
    ▼
Response bytes → Client
```

## Protocol trait

```rust
pub trait Protocol: Send + Sync {
    fn extract_model(&self, body: &str) -> String;
    fn path(&self) -> &str;
    fn fingerprint(&self, body: &str) -> String;
    fn parse_response_headers(&self, headers: &HeaderMap) -> ResponseHeaders;
    fn inject_cache_control(&self, body: &str, plan: &BreakpointPlan) -> Result<String, String>;
    fn is_streaming(&self, body: &str) -> bool;
}
```

Protocol drivers are stateless — all methods are pure functions of their
inputs. The pipeline (coalescing, safe cache, reduce, efficiency, ledger)
is product logic and stays in routes.rs; the trait does not model the full
request lifecycle.

## Trust domains

Trust domains isolate traffic by integration identity, upstream identity,
and credential reference. They are derived via SHA-256 but never include
raw credential values:

```text
SHA-256("toche-trust-domain-v1:{integration_id}:{integration_name}:{upstream_id}:{secret_ref_display}")
```

Where `secret_ref_display` is the debug-safe representation of the
`SecretRef` (e.g. `env:ANTHROPIC_API_KEY`, `cmd:op-helper`, `none`).

Different trust domains never share:
- In-flight request coalescing
- Persistent safe-cache entries
- Provider prompt-cache state

## Configuration model

```text
TocheConfig
├── schema_version: 2
├── RuntimeConfig
│   ├── port (default: 8743)
│   ├── listen_address (default: 127.0.0.1)
│   └── request_timeout_ms (default: 300000)
├── DefaultsConfig
│   └── integration (optional default)
├── StorageConfig
│   ├── ledger_db
│   └── cas_dir
├── Integrations[]
│   ├── id (SHA-256 derived, 8 hex chars)
│   ├── name
│   ├── upstream (ID reference)
│   ├── policy (optional ID reference)
│   ├── graphify (optional)
│   └── models (legacy mapping)
├── Upstreams[]
│   ├── id (SHA-256 derived)
│   ├── name
│   ├── url
│   ├── auth: UpstreamAuth { secret_ref, header_name }
│   └── headers
└── Policies[]
    ├── id (SHA-256 derived)
    ├── name
    ├── cache: CachePolicy { enabled, mode, breakpoint }
    ├── reduce: ReduceConfig { enabled, command_bypass }
    ├── efficiency: EfficiencyConfig { mode }
    └── safe_cache: SafeCacheConfig { enabled, ttl_days, max_entry_bytes }
```

IDs are derived deterministically from prefix + normalized name via SHA-256,
so the same named entity always produces the same ID.

## Identity model

Every request carries an `IdentityContext`:

```rust
pub struct IdentityContext {
    pub runtime_id: RuntimeId,           // UUIDv7, persisted across restarts
    pub request_id: RequestId,           // UUIDv7, generated per request
    pub external_request_id: Option<>,   // From x-request-id header
    pub integration_id: String,
    pub integration_name: String,
    pub upstream_id: String,
    pub upstream_name: String,
    pub trust_domain_id: TrustDomainId,  // SHA-256 derived
    pub instance_id: Option<String>,     // Nullable in proxy mode
    pub conversation_id: Option<String>,
    pub workspace_id: Option<String>,
    pub policy_ids: Vec<String>,
    pub config_snapshot_hash: String,
    pub attribution: Attribution,        // exact|client-reported|workspace-level|inferred|unknown
}
```

## Coalescing store

In-memory `CoalesceStore` using `std::sync::Mutex<HashMap<String, broadcast::Sender>>`.
The mutex is `std` (not `tokio`) because all lock durations are short
(HashMap insert/remove) and poison recovery keeps the store available after
a task panic. The mutex is never held across an `.await` point.

Flight key format:
```text
{upstream_url}|{fingerprint}|{trust_domain_id}|{policy_hash}
```

## Database schema

All databases live in `~/.toche/ledger.db` (SQLite, WAL mode):

**ledger** — Every routed request:
- model, profile_name, input_tokens, output_tokens
- cache_read_input_tokens, cache_creation_input_tokens
- coalesced_count, latency_ms, status, cost
- reduction_input_tokens, reduction_output_tokens, reduction_count
- efficiency_mode, local_cache_hit, project_path
- runtime_id, request_id, integration_id, upstream_id, trust_domain_id
- config_snapshot_hash, attribution, protocol

**safe_cache** — Persistent cross-session cache entries:
- project_path, fingerprint, workspace_fingerprint
- response_hash (CAS pointer), model, status
- tokens_input, tokens_output, hit_count

**cache_rejects** — Cache rejection audit trail:
- project_path, fingerprint, reason

**checkpoints** — Session continuity snapshots:
- project_path, goal, completed, next, changed_files, verification
- open_risks, model_assisted

**schema_version** — Forward-compatible migration tracking (current: 11).

## CAS storage

Content-addressed storage at `~/.toche/cas/<first2>/<remaining>` keyed by
SHA-256 hex digest. Stores:
- Raw unreduced tool outputs (for `toche expand` recovery)
- Cached response bodies (for safe cache replay)

## Bypass headers

All bypass headers accept `true` (case-insensitive). The umbrella header
overrides all individual bypasses.

| Header | Skips |
|--------|-------|
| `x-toche-bypass` | All stages (raw forward) |
| `x-toche-bypass-shield` | Request coalescing |
| `x-toche-bypass-safe-cache` | Persistent cache lookup/store |
| `x-toche-bypass-reduce` | Tool output reduction |
| `x-toche-bypass-efficiency` | Efficiency instruction injection |
| `x-toche-bypass-cache` | Ephemeral prompt cache injection |

## Config files

| File | Purpose |
|------|---------|
| `~/.toche/config.toml` | Runtime, integrations, upstreams, policies (schema v2) |
| `~/.toche/profiles.toml.v1.bak` | Legacy v1 backup after migration |
| `~/.toche/ledger.db` | SQLite request ledger |
| `~/.toche/cas/` | Content-addressed storage |
| `~/.toche/runtime_id` | Persistent UUIDv7 runtime identity |

Environment: `TOCHE_CONFIG_DIR` overrides the `~/.toche/` path.

Client configuration files modified by `toche connect` / `toche disconnect`:
- Claude Code: `~/.claude/settings.json` (with `.toche-backup`)
- Codex CLI: `~/.codex/config.toml` (with `.toche-backup`)

## HTTP routes

| Method | Path | Handler | Purpose |
|--------|------|---------|---------|
| POST | `/v1/messages` | `routes::messages` | Anthropic Messages API |
| POST | `/v1/responses` | `routes::responses` | OpenAI Responses API |
| GET | `/health` | `server::health` | Liveness probe |
| GET | `/ready` | `server::ready` | Readiness probe (config check) |
| GET | `/status` | `server::runtime_status` | Live runtime status |

## Secret handling

Credentials use the `SecretRef` enum:

```rust
pub enum SecretRef {
    Environment { key: String },     // Read from env var
    Command { program: String },     // Shell out to helper
    LegacyInline { value: String },  // Migrated from 1.0.x (redacted in Debug/Display)
    None,
}
```

The `Debug` and `Display` implementations for `SecretRef` redact inline values.
Raw credentials are never placed in logs, IDs, hashes, database diagnostics,
or receipts.

## Architecture decision records

### ADR-001: Protocol trait is narrow

The `Protocol` trait answers protocol-specific questions (model extraction,
fingerprinting, header parsing, cache injection, streaming detection) but
does not model the full request lifecycle. The pipeline (coalescing, safe
cache, reduce, efficiency, ledger) is product logic in routes.rs because it
is protocol-agnostic. This avoids the trap of a lossy universal message
schema.

### ADR-002: std::sync::Mutex for coalescing

The coalescing store uses `std::sync::Mutex` not `tokio::sync::Mutex`.
All lock durations are short (HashMap operations) and the mutex is never
held across an `.await` point. Poison recovery (`unwrap_or_else(|e|
e.into_inner())`) keeps the store available after a task panic.

### ADR-003: Streaming coalescing disabled in 1.1.0

Streaming requests are not coalesced in 1.1.0. The implementation order
is: (1) disable, (2) prove safe non-streaming coalescing, (3) implement
bounded live fan-out, (4) enable streaming coalescing only after fan-out
acceptance. This avoids buffered-after-completion behaviour being mistaken
for transparent streaming coalescing.

### ADR-004: No cross-protocol translation

Anthropic-to-OpenAI and OpenAI-to-Anthropic translation are explicit
non-goals for 1.1.0. Requests pass through their native protocol driver
only. The architecture leaves room for future translation layers but
does not partially implement them.

### ADR-005: Migration is one-way but safe

The v1 → v2 configuration migration is one-way: once `config.toml`
exists, it is authoritative. The legacy `profiles.toml` is backed up to
`profiles.toml.v1.bak` before removal. If config.toml is malformed, the
migration fails without touching the legacy file. An older binary
encountering a newer schema version refuses to modify it.

### ADR-006: Deterministic IDs from names

Integration, upstream, and policy IDs are derived from prefix + normalized
name via SHA-256 → first 4 bytes → hex. Same input always produces the
same ID. This avoids synthetic keys that break across machines while
still allowing stable cross-references in configuration.

## Known limitations

- OpenAI Responses is pass-through only in 1.1.0 (no reduction, caching,
  or efficiency). These may be enabled later through protocol-specific
  evidence gates.
- Streaming coalescing is disabled. All streaming requests make independent
  upstream calls.
- Codex conversation identity is not yet extracted from Codex-specific
  headers — attribution defaults to `unknown` or `workspace-level`.
- Native keyring storage for secrets is deferred to a future release.
  Secrets must be provided via environment variables, external commands,
  or legacy inline migration.
- Graphify and checkpoints remain in advanced/compatibility tier — they
  are not part of standard onboarding.
