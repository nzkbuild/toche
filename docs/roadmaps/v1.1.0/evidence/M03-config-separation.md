# M03 — Configuration schema separation

## M02 compliance-correction commit SHA

`3f1154a`

## M03 commit SHA

`af0d065`

## Base commit SHA

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049` (tag `v1.0.10`)

## Canonical file path

`~/.toche/config.toml` (or `TOCHE_CONFIG_DIR`)

## Schema version

`2`

## Authority precedence

1. Existing valid `config.toml` with `schema_version = 2` is authoritative.
2. If only `profiles.toml` exists, it is migrated to `config.toml`.
3. If both exist, `config.toml` wins and `profiles.toml` is left untouched.
4. If neither exists, configuration is reported as missing.

## Files changed

- `src/config/mod.rs` — exports new config submodules
- `src/config/toche_config.rs` — new: `TocheConfig`, `RuntimeConfig`, `Integration`, `Upstream`, `Policy`, `DefaultsConfig`, `StorageConfig`, `SecretRef`, `derive_id`
- `src/config/resolver.rs` — new: `ResolvedIntegration`, `ResolvedAuth`, `resolve_default`, `resolve_secret`
- `src/config/migration.rs` — new: `detect_and_load`, `migrate_v1_to_v2`, `config_to_legacy_profiles`
- `src/config/loader.rs` — new: `config_dir`, `load_config`, `load_default_integration`
- `src/lib.rs` — adds `pub mod config;`
- `src/profiles/loader.rs` — backward-compat wrapper
- `src/profiles/types.rs` — removes now-unused `default_profile`
- `src/gateway/routes.rs` — uses `load_default_integration` / `ResolvedIntegration`
- `src/gateway/server.rs` — `/ready` uses `load_default_integration`
- `src/cli/status.rs` — uses `load_config`
- `src/cli/doctor.rs` — uses `load_config`
- `src/cli/graph.rs` — uses `load_default_integration`
- `src/cli/setup.rs` — writes `config.toml` via `migrate_v1_to_v2`
- `src/cache/breakpoint.rs` — imports `CacheBreakpoint` from new config module
- `src/cache/inject.rs` — test imports updated
- `tests/cache_fixtures.rs` — imports `CacheBreakpoint` from new config module
- `docs/roadmaps/v1.1.0/M03_CONFIG_DESIGN.md` — design document

## Schema v2 summary

```
TocheConfig
├── schema_version = 2
├── runtime: RuntimeConfig
├── defaults: DefaultsConfig
├── storage: StorageConfig
├── integrations: [Integration]
├── upstreams: [Upstream]
└── policies: [Policy]
```

- `RuntimeConfig`: port, listen_address, request_timeout_ms
- `Upstream`: id, name, url, auth (SecretRef + header_name), headers
- `SecretRef`: Environment { key }, Command { program }, LegacyInline { value }, None
- `Policy`: id, name, cache, reduce, efficiency, safe_cache
- `Integration`: id, name, upstream (id ref), policy (id ref), graphify, models
- `DefaultsConfig`: integration (id ref)
- `StorageConfig`: ledger_db, cas_dir

## Deterministic IDs

8-hex-char SHA-256 of `"<prefix>:<normalized_name>"`:

```rust
pub fn derive_id(prefix: &str, name: &str) -> String {
    let normalized = name.trim().to_lowercase();
    let input = format!("{prefix}:{normalized}");
    let hash = sha2::Sha256::digest(input.as_bytes());
    hex::encode(&hash[..4])
}
```

Same name + prefix always yields the same ID across runs.

## Legacy migration rules

1. Each `Profile` → one `Integration` + one `Upstream` + one `Policy`.
2. IDs derived from `integration:`, `upstream:`, `policy:` + normalized profile name.
3. `AuthMethod::ApiKey` → `SecretRef::LegacyInline { value: key }` + `header_name`.
4. `AuthMethod::BearerToken` → `SecretRef::LegacyInline { value: token }` + `authorization`.
5. `AuthMethod::None` → `SecretRef::None` + `x-api-key`.
6. Per-profile feature configs → `Policy`.
7. `profile.graphify` → `integration.graphify`.
8. `profiles.default` → `defaults.integration` (by ID reference).

## Migration procedure

`detect_and_load(config_dir)`:
1. If `config.toml` exists and `schema_version == 2`, load it.
2. Else if `profiles.toml` exists:
   - If `config.toml` also exists, prefer existing v2 config (idempotent).
   - Parse legacy `Profiles`.
   - `migrate_v1_to_v2()` → `TocheConfig`.
   - Atomic write to `config.toml` via temp file + rename.
   - Rename `profiles.toml` → `profiles.toml.v1.bak` (only if backup does not exist).
3. Else return `Missing`.

## Secret handling

- `SecretRef::LegacyInline` value is never printed by `Debug`, `Display`, or serialized in evidence.
- `Debug` shows `LegacyInline(***)`.
- `Display` shows `inline(***)`.
- Environment key and command program are shown (they are key names, not secrets).

## Validation results

| Command | Result |
|---------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-features --locked` | PASS (391 tests) |
| `npm run test:npm` | PASS (5 tests) |
| `cargo deny check` | PASS |
| `cargo about generate about.hbs --fail -o target/THIRD_PARTY_DEPENDENCIES.md` | PASS |
| `cargo bench` | PASS |
| `git diff --check` | PASS |

## Test coverage

### Schema tests (`src/config/toche_config.rs`)
- `derive_id_deterministic`
- `derive_id_different_names`
- `derive_id_whitespace_insensitive`
- `derive_id_case_insensitive`
- `derive_id_different_prefixes`
- `derive_id_is_8_hex_chars`
- `secret_ref_debug_hides_legacy_inline`
- `secret_ref_display_hides_legacy_inline`
- `secret_ref_environment_debug_shows_key`
- `runtime_config_defaults`
- `storage_config_defaults`
- `toche_config_roundtrip_minimal`
- `toche_config_roundtrip_full`

### Migration tests (`src/config/migration.rs`)
- `migrate_single_profile_api_key`
- `migrate_all_feature_configs`
- `migrate_bearer_token_to_legacy_inline`
- `migrate_two_profiles`
- `deterministic_ids_across_runs`
- `detect_and_load_migrates_v1_to_v2`
- `detect_and_load_is_idempotent`
- `detect_and_load_prefers_existing_v2`

### Resolver tests (`src/config/resolver.rs`)
- `resolve_default_returns_integration`
- `resolve_default_returns_none_when_no_default`
- `resolve_secret_legacy_inline`
- `resolve_secret_none`
- `resolve_secret_environment`
- `resolve_secret_environment_missing`
- `resolved_integration_has_cache_policy`
- `resolved_integration_has_upstream_headers`

## Dependencies added

None. M03 reused existing `serde`, `toml`, `sha2`, `hex`.

## Files changed

- `src/config/mod.rs`
- `src/config/toche_config.rs`
- `src/config/resolver.rs`
- `src/config/migration.rs`
- `src/config/loader.rs`
- `src/lib.rs`
- `src/profiles/loader.rs`
- `src/profiles/types.rs`
- `src/gateway/routes.rs`
- `src/gateway/server.rs`
- `src/cli/status.rs`
- `src/cli/doctor.rs`
- `src/cli/graph.rs`
- `src/cli/setup.rs`
- `src/cache/breakpoint.rs`
- `src/cache/inject.rs`
- `tests/cache_fixtures.rs`
- `docs/roadmaps/v1.1.0/M03_CONFIG_DESIGN.md`
- `docs/roadmaps/v1.1.0/evidence/M03-config-separation.md`

## Exact tests added

- `src/config/toche_config.rs`: 13 tests
- `src/config/migration.rs`: 8 tests
- `src/config/resolver.rs`: 8 tests

Total new tests: 29.

## Before-and-after test totals

| Version | Tests |
|---------|-------|
| v1.0.10 | 285 |
| M03     | 391 |

## Benchmark comparison

No benchmark regression observed. `cargo bench` completed successfully.
Baseline and M03 results recorded separately.

## CLI compatibility checks

- `toche doctor` reads `config.toml` and reports schema v2.
- `toche status` reads `config.toml` and reports default integration.
- `toche setup` writes `config.toml` and preserves legacy migration backup.
- `toche connect` / `toche disconnect` unchanged at surface; operate on Claude `settings.json`.

## Database changes

None. SQLite schema unchanged.

## Known limitations

- `SecretRef::Command` is supported but not yet exposed in interactive setup.
- Native keyring storage deferred beyond 1.1.0.
- Multi-client runtime routing belongs to M06.

## Acceptance-gate result

PASS. Schema v2 is implemented, legacy migration is deterministic and idempotent,
all callers are wired to the new types, and the full validation suite passes.

## Next unlocked milestone

M04 — Setup transaction engine.
