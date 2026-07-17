# M03 — Configuration Schema v2 Design

## Current state (v1.0.10 profiles.toml)

Single overloaded `Profile` struct mixes concerns:
- Upstream routing (url, headers, auth)
- Feature configs (cache, reduce, efficiency, safe_cache, graphify)
- Identity (name only, no stable id)

`profiles.toml` has no schema_version field. No migration path exists.

Auth credentials are stored as plaintext in `AuthMethod::ApiKey { key }` and
`AuthMethod::BearerToken { token }`.

## Target state (config.toml v2)

Seven separated types, each with its own identity:

```
TocheConfig
├── schema_version = 2
├── runtime: RuntimeConfig
├── integrations: [Integration]
├── upstreams: [Upstream]
├── policies: [Policy]
├── defaults: DefaultsConfig
└── storage: StorageConfig
```

### Type definitions

**RuntimeConfig** — gateway process configuration (new, hardcoded defaults today):
```toml
[runtime]
port = 8743
listen_address = "127.0.0.1"
request_timeout_ms = 300_000
```

**Upstream** — a provider endpoint with auth:
```toml
[[upstreams]]
id = "7f8a3b2c"
name = "Anthropic"
url = "https://api.anthropic.com"
auth.secret_ref = { type = "environment", key = "ANTHROPIC_API_KEY" }
auth.header_name = "x-api-key"
headers = { "anthropic-version" = "2023-06-01" }
```

**SecretRef** — credential indirection (never printed by doctor/status/errors/debug):
```rust
enum SecretRef {
    Environment { key: String },    // read from env var
    Command { program: String },    // execute and read stdout
    LegacyInline { value: String }, // plaintext (migrated from v1)
    None,
}
```

**Integration** — ties upstream to policies and features:
```toml
[[integrations]]
id = "a1b2c3d4"
name = "default"
upstream = "7f8a3b2c"
policy = "e5f6g7h8"
[integrations.graphify]
enabled = true
graph_path = "/custom/graph.json"
```

**Policy** — feature configuration (cache, reduce, efficiency, safe_cache):
```toml
[[policies]]
id = "e5f6g7h8"
name = "default"
[policies.cache]
enabled = true
mode = "observe"
breakpoint = "standard"
[policies.reduce]
enabled = true
command_bypass = ["kubectl"]
[policies.efficiency]
mode = "normal"
[policies.safe_cache]
enabled = true
ttl_days = 30
max_entry_bytes = 1_048_576
```

**DefaultsConfig**:
```toml
[defaults]
integration = "a1b2c3d4"
```

**StorageConfig**:
```toml
[storage]
ledger_db = "ledger.db"
cas_dir = "cas"
```

### Identifier generation (deterministic, not random)

IDs are 8 hex chars, derived via SHA-256:
```
id = hex(sha256("integration:" || normalized_name))[..8]
```
where `normalized_name = trim(lowercase(name))`.

This is deterministic across runs — re-migrating the same profiles.toml
produces the same IDs. It is not a UUID (intentionally — no randomness).

## Canonical resolver: ResolvedIntegration

At runtime, the loader flattens the graph into:
```rust
struct ResolvedIntegration {
    id: String,
    name: String,
    upstream_url: String,
    upstream_headers: HashMap<String, String>,
    auth: ResolvedAuth,
    cache: Option<CacheConfig>,
    reduce: Option<ReduceConfig>,
    efficiency: Option<EfficiencyConfig>,
    safe_cache: Option<SafeCacheConfig>,
    graphify: Option<GraphifyConfig>,
}
```

The resolver:
1. Finds the default integration from `defaults.integration`
2. Follows `integration.upstream` → Upstream
3. Follows `integration.policy` → Policy
4. Merges integration-level overrides (graphify) with policy defaults
5. Resolves SecretRef → actual credential value (never caches the value)

## Legacy migration: profiles.toml → config.toml

### Rules
1. Each `Profile` → one Integration + one Upstream + one Policy
2. ID derived from `"integration:" || normalized_name`, etc.
3. `AuthMethod::ApiKey` → `SecretRef::LegacyInline { value: key }` (retains existing plaintext; user upgrades to env/command separately)
4. `AuthMethod::BearerToken` → same pattern
5. `AuthMethod::None` → `SecretRef::None`
6. Per-profile feature configs (cache, reduce, efficiency, safe_cache) → Policy
7. `profile.graphify` → `integration.graphify` (extension field)
8. `profiles.default` → `defaults.integration`

### Migration procedure
1. Read `profiles.toml` and parse as legacy `Profiles`
2. Generate deterministic IDs for each profile
3. Build new `TocheConfig` with `schema_version = 2`
4. Serialize to `config.toml` (pretty TOML)
5. Atomic write via temp file + rename
6. On success, rename `profiles.toml` → `profiles.toml.v1.bak`

### Idempotency
- If `config.toml` already exists with `schema_version = 2`, migration is a no-op
- If `profiles.toml` does not exist, `config.toml` is looked up directly (no migration needed on fresh setups)

### Validation on migration
- Every profile must have a non-empty name and upstream_url
- AuthMethod::ApiKey must have a non-empty header_name
- Invalid profiles are skipped with a warning, not aborted

### Failure behavior
- If serialization fails: config.toml is not written, profiles.toml is untouched
- If atomic write fails: temp file cleaned up, profiles.toml is untouched
- Migration is transactional: either config.toml is written AND backup created, or neither

## Runtime compatibility

The `load_profiles()` function is replaced by a config loader that:
1. Checks for `config.toml` (schema v2)
2. Falls back to `profiles.toml` (schema v1) → migrates → uses result
3. Returns a `ResolvedIntegration` for the default

The gateway hot path (`routes.rs`) accesses only already-resolved fields, so
the resolver runs once at gateway startup, not per request.

For the `/ready` endpoint: the server resolves on startup and re-resolves on
each `/ready` probe (config may have changed between startup and connect).

## File layout

```
src/config/
├── mod.rs              # updated module exports
├── utils.rs            # unchanged (atomic_write, read_jsonc, home_dir)
├── toche_config.rs     # NEW: TocheConfig, RuntimeConfig, Integration, Upstream,
                        #      Policy, DefaultsConfig, StorageConfig, SecretRef
├── resolver.rs         # NEW: ResolvedIntegration, resolve_default()
├── migration.rs        # NEW: migrate_v1_to_v2(), detect_config_version()
└── loader.rs           # NEW: load_config_dir(), config_dir() (moved from profiles/loader.rs)

src/profiles/
├── mod.rs              # re-exports from config for backward compat
├── loader.rs           # thin wrapper → config::loader
└── types.rs            # thin re-export → config::toche_config (deprecated)
```

Existing callers of `load_profiles()` → `Profile` are updated to use
`config::resolver::resolve_default()` → `ResolvedIntegration`.

## Tests required

### Schema tests
- `TocheConfig` round-trip: serialize → deserialize → identical
- `SecretRef` serialization: all four variants produce correct TOML
- `SecretRef` Display/Debug: must NOT contain credential values
- Default `RuntimeConfig` is 127.0.0.1:8743 / 300s timeout
- `StorageConfig` defaults to ledger.db / cas

### Migration tests
- Single profile with ApiKey → Integration, Upstream, Policy with matching IDs
- Profile with all feature configs → Policy preserves all fields
- Profile with BearerToken → Upstream with LegacyInline
- Profile with AuthMethod::None → Upstream with SecretRef::None
- Profiles.default → DefaultsConfig.integration (id-based reference)
- Two profiles → two Integration/Upstream/Policy triples
- Deterministic IDs: same profile name → same ID every time
- Different profile names → different IDs (no collisions on reasonable names)
- profiles.toml with invalid profile → skipped with warning, others migrated
- Missing profiles.toml and config.toml → clean error, no migration attempted
- config.toml already exists (schema_version=2) → migration skipped
- Atomic write failure → both files unchanged
- profiles.graphify → integration.graphify (extension field, not in policy)

### Resolver tests
- Default integration resolves → upstream url, headers, auth present
- SecretRef::Environment reads from env var
- SecretRef::Command executes and reads stdout
- SecretRef::LegacyInline returns the stored value
- SecretRef::None returns no auth
- Integration with no graphify → ResolvedIntegration.graphify is None
- Integration override merges with policy defaults

## Dependencies

No new crate dependencies. Existing `serde`, `toml`, `sha2`, `hex` are sufficient.
New dev-dependencies for M03: none.

## What does NOT change

- SQLite schema (unchanged — config schema is independent)
- Gateway port (8743, hardcoded → RuntimeConfig default)
- CAS storage path
- Ledger DB path
- All per-request processing (cache, reduce, efficiency, safe_cache)
- Claude Code settings.json (connect/disconnect unchanged)
- npm installer
- Build script

## What IS deferred to later milestones

- M04: Interactive setup wizard using multiple integrations
- M06: Multi-upstream runtime routing
- M10: Codex multi-client protocol support
- M11: Pricing data integration
