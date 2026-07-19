# Toche v1.0.10 — Current Reality Audit

## Base commit

`3a2a3e13dff897edcc51ce690e4ccd5f7af0c049` (tag `v1.0.10`)

## Repository and release state

| Item | Value |
|------|-------|
| Rust package version | `1.0.10` (`Cargo.toml`) |
| npm package version | `1.0.10` (`package.json`, scope `@nzkbuild/toche`) |
| Minimum Rust version | `1.86` |
| Default branch | `master` (local `0c492d2` stale, public now at `3a2a3e1`) |
| Feature branch | `feat/1.1.0-multi-client-runtime` |
| Edition | `2024` |

### npm installer

- Entry: `npm/install.js`
- Test entry: `npm/install.test.js`
- Supported platforms: `darwin-arm64`, `darwin-x64`, `linux-x64`, `win32-x64`
- Archive formats: `tar.gz` (Unix), `zip` (Windows)
- Binary name inside archive: `toche` / `toche.exe`
- Post-install script: `node npm/install.js`
- Supports `TOCHE_BINARY_PATH` and `TOCHE_DOWNLOAD_BASE` env overrides.

### CI workflows

- `.github/workflows/ci.yml`
  - `quality` job: format, clippy, tests on Ubuntu
  - `windows` job: tests + npm installer tests on Windows
  - `minimum-rust` job: tests with Rust 1.86.0
  - `npm-package` job: npm tests + `npm pack --dry-run`
- `.github/workflows/prepare-release.yml`
  - Validates tag vs package versions
  - Runs full test matrix
  - Creates/verifies annotated tag
  - Creates draft GitHub release
  - Builds four platform archives
  - Builds npm tarball
  - Uploads assets to draft release

### Supported release platforms

| OS | Target | Format |
|----|--------|--------|
| Linux | `x86_64-unknown-linux-gnu` | `tar.gz` |
| Windows | `x86_64-pc-windows-msvc` | `zip` |
| macOS Intel | `x86_64-apple-darwin` | `tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `tar.gz` |

### Filter inventory

- Location: `assets/filters/rtk/` (63 RTK filters) + `assets/filters/toche/` (2 Toche-owned filters)
- Combined at build time by `build.rs` into `builtin_filters.toml`
- Schema version: `1`
- RTK source commit: `5d32d0736f686b69d1e8b9dc45c007d4eb77a0a2`
- License: Apache-2.0

### Third-party notices

- `THIRD_PARTY_NOTICES.md` lists reuse inventory and license texts.
- `NOTICE` file lists attribution.
- Current reuse actions: reference/adaptation only; no whole-gateway dependencies.

### Test and benchmark structure

- Unit tests: embedded in source files under `#[cfg(test)]`
- Integration tests: `tests/*.rs`
- Benchmarks: `benches/pipeline.rs` (criterion)
- npm tests: `npm/install.test.js`

## CLI surface

| Command | Module | Config deps | DB deps | Files modified | Claude assumption | 1.1.0 surface |
|---------|--------|-------------|---------|----------------|-------------------|---------------|
| `toche` (default runtime) | `gateway::serve` | `profiles.toml` | `ledger.db` | None at runtime | Only Anthropic Messages API | Primary |
| `toche setup` | `cli::setup` | `profiles.toml` | None | `~/.toche/profiles.toml` | Imports from Claude Code `settings.json` | Primary |
| `toche connect [claude]` | `cli::connect` | `profiles.toml` | None | `~/.claude/settings.json`, backup | Claude Code only | Primary |
| `toche disconnect [claude]` | `cli::disconnect` | `profiles.toml` | None | `~/.claude/settings.json`, backup | Claude Code only | Primary |
| `toche doctor` | `cli::doctor` | `profiles.toml` | `ledger.db` | None | Checks Claude Code settings | Primary |
| `toche status` | `cli::status` | `profiles.toml` | None | None | Reads default profile | Primary |
| `toche stats` | `cli::stats` | `profiles.toml` | `ledger.db` | None | None | Primary |
| `toche expand <hash>` | `cli::expand` | None | CAS `~/.toche/cas/` | None | None | Advanced |
| `toche cache inspect/clear/why` | `cli::cache` | `profiles.toml` | `ledger.db` | CAS | None | Advanced |
| `toche checkpoint save/show/list/delete` | `cli::checkpoint` | `profiles.toml` | `ledger.db` | None | Session facts from Anthropic responses | Advanced |
| `toche graph query/path/explain/affected/status/extract` | `cli::graph` | `profiles.toml` (graphify config) | None | `graph.json` | Calls external `graphify` CLI | Experimental/Advanced |

## Configuration

### `profiles.toml`

- Loaded by `src/profiles/loader.rs`
- Location: `~/.toche/profiles.toml` (or `TOCHE_CONFIG_DIR`)
- Schema: `Profiles { default: Option<String>, profiles: Vec<Profile> }`
- `Profile` fields:
  - `name`
  - `upstream_url`
  - `auth_method` (`ApiKey`, `BearerToken`, `None`)
  - `headers` (HashMap)
  - `models` (HashMap)
  - `cache`, `reduce`, `efficiency`, `safe_cache`, `graphify` (optional per-profile)

### Claude settings

- Path: `~/.claude/settings.json`
- Read with JSONC tolerance (`//` comments stripped)
- `connect` sets `baseURL` and `env.ANTHROPIC_BASE_URL`
- `disconnect` restores from backup or saved original URL

### Backup files

- `~/.claude/settings.json.toche-backup`
- `~/.toche/pre_toche_url.txt`

### Environment variables

- `TOCHE_CONFIG_DIR`
- `TOCHE_BINARY_PATH`
- `TOCHE_DOWNLOAD_BASE`

### JSONC handling

- `src/config/utils.rs::read_jsonc` strips `//` comment lines before parsing.

### Atomic-write utilities

- `src/config/utils.rs::atomic_write` — temp file + rename
- `src/config/utils.rs::atomic_write_secure` — restrictive permissions (0o600 on Unix)

### Profile resolution

- If `default` name is set, find matching profile; otherwise use first profile.
- No integration/upstream separation; profile is overloaded with client, upstream, policy, and storage config.

### Authentication structures

- `AuthMethod::ApiKey { header_name, key }`
- `AuthMethod::BearerToken { token }`
- `AuthMethod::None`
- API keys are stored in plaintext in `profiles.toml`.

### Model mappings

- `Profile.models` is a `HashMap<String, String>` but is not used in the request path.
- Model is extracted from request body in `gateway::routes::extract_model`.

### Feature configuration

- Cache, reduction, efficiency, safe cache, and graphify are all optional per-profile structs.

### Risks and limitations

- Plaintext API keys in `profiles.toml`.
- Profile combines integration, upstream, policy, and storage.
- Setup is one-shot import from Claude Code; not rerunnable as a reconciler.
- Only one default profile; no multi-client concept.
- Connect/disconnect only supports Claude Code.

## Runtime and request path

```text
client request
→ Axum route /v1/messages (src/gateway/routes.rs)
→ extract_model (Anthropic-specific)
→ load_profiles → default_profile
→ bypass header checks
→ shield::fingerprint::compute (SHA-256 of normalized JSON)
→ shield::coalesce::store().try_acquire(upstream_url, fingerprint)
  → if coalesced: return captured response
→ safe_cache lookup (project_path + fingerprint + workspace_fingerprint)
  → if hit: return cached response
→ reduce::transform::reduce_body (tool output reduction)
→ efficiency::inject::inject_efficiency (system prompt injection)
→ cache::inject::inject_cache_control (provider prompt cache breakpoints)
→ forward_to_upstream (reqwest POST to upstream_url/v1/messages)
→ collect full response body into memory
→ safe_cache inspect + store if safe
→ build SSE stream from collected bytes
→ continuity::observer::observe_response (extract facts)
→ ledger record (fire-and-forget tokio task)
→ return SSE stream
```

### Stage details

| Stage | Module | Input | Output | Shared/global state | Anthropic assumptions |
|-------|--------|-------|--------|---------------------|------------------------|
| Route | `gateway::routes` | `HeaderMap`, `String` body | `Sse<impl Stream>` | None | `/v1/messages` only; `model` field extraction |
| Fingerprint | `shield::fingerprint` | Raw body string | 64-char hex SHA-256 | None | Strips `stream` and `cache_control` from Anthropic body shape |
| Coalescing | `shield::coalesce` | `upstream_url`, `fingerprint` | `CoalesceResult` | Global `LazyLock<CoalesceStore>` | None in key, but only one upstream URL |
| Safe cache | `safe_cache::cache_db` | `project_path`, `fingerprint` | `Option<CacheEntry>` | `~/.toche/ledger.db` | None |
| Reduction | `reduce::transform` | Body string | Modified body | `~/.toche/cas/` | Looks for `tool_result` blocks |
| Efficiency | `efficiency::inject` | Body string | Modified body | None | Injects into `system` prompt |
| Cache injection | `cache::inject` | Body string | Modified body | None | Injects `cache_control` into Anthropic content blocks |
| Upstream forward | `gateway::routes::forward_to_upstream` | `Client`, URL, headers, body | `ForwardedResponse` | New `Client` per request | Reads Anthropic cache token headers |
| Response stream | `gateway::routes` | `ForwardedResponse` | SSE stream | None | Buffers full response, then emits single SSE event |
| Observer | `continuity::observer` | Response bytes | None | Global `LazyLock<Mutex<SessionFacts>>` | Parses Anthropic tool_use blocks |
| Ledger | `meter::db` | Record fields | Row ID | `~/.toche/ledger.db` | None |

### Error behaviour

- Profile load failure → `500 INTERNAL_SERVER_ERROR`
- No default profile → `500 INTERNAL_SERVER_ERROR`
- Upstream request failure → `502 BAD_GATEWAY`
- Coalescing prior failure → `503 SERVICE_UNAVAILABLE`
- Reduction/efficiency/cache injection failures fall back to original body.

### Bypass behaviour

- `x-toche-bypass` disables all optimizations.
- `x-toche-bypass-shield` disables coalescing.
- `x-toche-bypass-safe-cache` disables safe cache.
- `x-toche-bypass-reduce` disables reduction.
- `x-toche-bypass-efficiency` disables efficiency injection.
- `x-toche-bypass-cache` disables cache breakpoint injection.

### Persistence behaviour

- Safe cache entries stored in `ledger.db` with response hash pointing to CAS.
- Ledger records stored in `ledger.db`.
- CAS blobs stored in `~/.toche/cas/<first2>/<remaining>`.

### Concurrency behaviour

- Coalescing uses a global `LazyLock<CoalesceStore>` with `std::sync::Mutex` and `tokio::sync::broadcast`.
- Ledger writes happen in a spawned `tokio::spawn` task.
- Safe cache DB is opened per request.

## Identity and isolation

### What exists

- Request fingerprint: SHA-256 of normalized JSON body (`shield::fingerprint`).
- Project path: current working directory canonical path.
- Profile name: from `profiles.toml`.
- Upstream URL: from profile.
- Model: extracted from request body.
- Cache key: `(project_path, fingerprint)`.
- Ledger identity: `(model, profile_name, project_path)`.

### What does not exist

- `runtime_id`
- `request_id`
- `integration_id`
- `trust_domain_id`
- `upstream_id`
- `client_instance_id`
- `conversation_id`
- `session_id`
- `configuration_snapshot_hash`

### Collision risks

- Two clients using the same profile and same request body will coalesce because the coalescing key is `(upstream_url, fingerprint)` only.
- Two clients with different credentials but same upstream URL and body will coalesce.
- Safe cache key is `(project_path, fingerprint)`; different credentials do not affect it.
- Ledger records only `profile_name`; no integration or trust-domain separation.
- Global `SessionFacts` observer is shared across all concurrent requests.

## Coalescing audit

### Implementation

- File: `src/shield/coalesce.rs`
- Key: `format!("{}|{}", upstream_url, fingerprint)`
- Store: `HashMap<String, broadcast::Sender<Option<CapturedResponse>>>` protected by `std::sync::Mutex`
- Global singleton via `LazyLock`

### Verified behaviour

| Aspect | Status | Evidence |
|--------|--------|----------|
| Key composition | URL + fingerprint only | `try_acquire` line 55 |
| Leader lifecycle | Creates entry, removes on `complete` | `complete` removes sender |
| Waiter lifecycle | Subscribes to broadcast, waits for result | `try_acquire` lines 58-76 |
| Error propagation | `Failed` returned on `Closed`/`Lagged` | lines 74-75 |
| Cancellation | `cancel` method sends `None` and removes entry | lines 91-96 |
| Panic cleanup | Mutex poison recovery via `unwrap_or_else` | lines 58, 81, 92 |
| Response buffering | Full body collected before completing | `gateway::routes` |
| Streaming behaviour | Buffered then emitted as single SSE event | `gateway::routes` lines 470-475 |

### Test verification

- `forward_on_first_call`: single request becomes leader.
- `coalesced_on_second_concurrent_call`: only verifies leader acquires key (does not actually test waiter coalescing).
- `complete_then_new_request_gets_forward`: store cleaned after complete.
- `cancel_cleans_up`: cancel removes entry.
- `different_urls_different_keys`: different URLs get different keys.
- `different_fingerprints_both_forward`: different fingerprints get different keys.
- `concurrent_waiter_gets_coalesced`: **creates a fresh store for the waiter**, so it does not exercise true shared-store concurrency.

### Conclusion

The existing tests do not exercise true shared-store concurrency where a leader and waiter share the same `CoalesceStore`. The `concurrent_waiter_gets_coalesced` test creates a separate store, defeating the purpose. This is a confirmed gap.

## Persistence

### SQLite schema

- Single file: `~/.toche/ledger.db`
- WAL enabled (`PRAGMA journal_mode=WAL`)
- Busy timeout 5000ms
- Integrity check on open
- Schema version table: `schema_version(version INTEGER)`
- Current expected version: `9`

### Tables

- `ledger` — request records (migrated through versions 1-7)
- `safe_cache` — cache entries (version 8)
- `cache_rejects` — rejected cache candidates (version 8)
- `checkpoints` — session checkpoints (version 9)

### WAL use

- Enabled in `LedgerDb::open`, `CacheDb::open`, `CheckpointDb::open`.

### Safe-cache records

- Fields: `id`, `project_path`, `fingerprint`, `workspace_fingerprint`, `response_hash`, `model`, `status`, `tokens_input`, `tokens_output`, `created_at`, `last_hit_at`, `hit_count`
- Unique: `(project_path, fingerprint)`

### Cache rejection records

- Fields: `id`, `timestamp`, `project_path`, `fingerprint`, `reason`

### Checkpoint records

- Fields include project path, git head, workspace fingerprint, task, completed, changed files, verification, open risks, next action, facts JSON, model assisted, timestamps.

### CAS storage

- Directory: `~/.toche/cas/<first2>/<remaining>`
- Content-addressed by SHA-256 hex.
- Used by reduction for original tool outputs.

### Content retention

- Safe cache TTL: 30 days default.
- Ledger cleanup: 90 days.
- CAS deletion triggered by cache eviction/clear.

### Corruption and disk-error behaviour

- Integrity check fails → bail.
- Newer schema version detected → bail with upgrade message.
- Ledger write failures are logged but do not block the response.

## Existing reusable modules

| Module | Path | Reuse recommendation |
|--------|------|----------------------|
| Atomic writes | `src/config/utils.rs` | Preserve and extend |
| JSONC reading | `src/config/utils.rs::read_jsonc` | Preserve |
| Profile loader | `src/profiles/loader.rs` | Replace schema, reuse path/env logic |
| CLI test harnesses | `tests/cli_*.rs` | Extend for multi-client tests |
| Ledger | `src/meter/db.rs`, `src/meter/types.rs` | Extend schema, preserve aggregation |
| CAS | `src/reduce/storage.rs` | Preserve |
| Reduction engine | `src/reduce/transform.rs`, `src/reduce/rtk/` | Preserve behind protocol boundary |
| RTK filter loader | `build.rs`, `src/reduce/rtk/toml_filter.rs` | Preserve |
| Request fingerprinting | `src/shield/fingerprint.rs` | Extend with trust-domain components |
| Bypass headers | `src/gateway/routes.rs` | Preserve pattern |
| Health/readiness | `src/gateway/server.rs` | Extend for multi-client status |
| npm installer tests | `npm/install.test.js` | Preserve |
| Release workflow | `.github/workflows/prepare-release.yml` | Preserve |

## Major gaps vs 1.1.0 contract

1. No integration/upstream separation.
2. No trust-domain identity.
3. No runtime/request/session IDs.
4. Coalescing key lacks trust boundaries.
5. No true concurrent-waiter coalescing tests.
6. Only Anthropic Messages API route exists (`/v1/messages`).
7. No OpenAI Responses ingress.
8. Setup is not rerunnable as a reconciler.
9. Only Claude Code is supported.
10. Global `SessionFacts` observer is shared across all traffic.
11. Plaintext credential storage.
12. Stats do not distinguish integrations or protocols.
