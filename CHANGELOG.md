# Changelog

All notable changes to Toche are documented in this file.

## [Unreleased]

### Fixed

- **Config directory race in background tasks.** Fire-and-forget `tokio::spawn`
  tasks in request handlers could read a different test's `TOCHE_CONFIG_DIR` at
  task-execution time. The config directory is now captured in `AppState` when the
  router is built and used by asynchronous work.

- **npm installer test fixture.** The `releaseAsset` test now derives expected
  archive names from the version in `package.json`, keeping the fixture synchronized
  with the package version.

- **CHANGELOG typo.** "All exiting 1.0.x configurations" corrected to "All
  existing 1.0.x configurations".

## [1.1.1] — 2026-07-20

### Fixed

- **Shared schema version mismatch.** The `LedgerDb`, `CacheDb`, and `CheckpointDb`
  modules share a single SQLite database but maintained independent expected schema
  versions. The ledger wrote version 11 while the cache and checkpoint modules
  expected version 9, causing `CacheDb::open` and `CheckpointDb::open` to reject a
  valid database with a misleading "newer version" error. All three modules now
  expect version 11, and table creation in `CacheDb` and `CheckpointDb` is
  unconditional (`CREATE TABLE IF NOT EXISTS` without a version gate) so a fresh
  database initializes correctly regardless of insertion order.

### Added

- **`m24_schema_version_sync` test suite** (5 tests) that verifies:
  - `CheckpointDb` and `CacheDb` open after a simulated ledger writes version 11.
  - Both modules reject a database whose schema version exceeds 11.
  - Both modules open correctly with no prior `schema_version` table.

## [1.1.0] — 2026-07-20

### Added

- **Multi-client runtime.** The Toche gateway now accepts connections from
  multiple simultaneous AI clients. Claude Code and Codex CLI can run
  concurrently through the same local Toche instance.
- **Codex CLI integration.** `toche setup` detects Codex installations and
  configures an OpenAI Responses upstream. Both persistent mode (`toche` +
  `codex`) and managed mode (`toche run codex`) are supported.
- **OpenAI Responses protocol support.** A protocol driver for the OpenAI
  Responses API (`/v1/responses`) with correct pass-through forwarding,
  streaming, trust isolation, measurement, and unknown-field preservation.
- **Protocol-driver architecture.** Protocol-specific logic lives behind a
  `Protocol` trait (`src/protocol/`). Raw request bytes remain authoritative
  and unknown fields survive unchanged. The Anthropic pipeline is
  behaviourally equivalent to 1.0.x.
- **Runtime identity and trust domains.** Every runtime instance, request,
  and client carries independently identifiable UUIDv7 metadata. Trust domains
  are derived from integration, upstream, and credential-reference identity
  without exposing raw secrets.
- **Configuration schema v2.** The overloaded `Profile` concept is replaced
  with separate `RuntimeConfig`, `Integration`, `Upstream`, `Policy`, and
  `StorageConfig` types. Deterministic ID derivation ensures stable references.
- **Rerunnable setup engine.** `toche setup` detects, resolves, previews,
  applies, and verifies configuration transactionally. It modifies only
  Toche-owned fragments, preserves unrelated settings, and is safe to interrupt.
- **Safe flight registry.** Coalescing keys now include protocol version,
  upstream ID, trust-domain ID, and policy hash — not just URL and body
  fingerprint. RAII leader cleanup, deterministic waiter wake-up, and
  cancellation isolation prevent cross-client corruption.
- **`toche status` command.** Reports live runtime endpoint, active
  integrations, active flights, coalesced waiters, protocol counts, and
  degraded optional systems.
- **`toche stats` filtering.** Stats accept `--protocol`, `--integration`,
  and `--trust-domain` filters.
- **Measurement confidence.** Every value in stats output is classified as
  `measured`, `provider-reported`, `estimated`, `configured`, or `unknown`.
- **Managed mode (`toche run`).** Launch a client through Toche in one step
  using the same runtime, configuration, and optimization pipeline as
  persistent mode.
- **Migration from 1.0.x.** `profiles.toml` is migrated to `config.toml`
  automatically, with backup, idempotency, and safe failure on malformed input.

### Changed

- **Configuration format.** `~/.toche/profiles.toml` is superseded by
  `~/.toche/config.toml` (schema v2). The legacy file is backed up on migration
  and the migration is one-way but safe.
- **`toche connect` and `toche disconnect`** accept a client argument
  (`claude` or `codex`) instead of only operating on Claude Code.
- **Coalescing is disabled for streaming requests** in 1.1.0 pending
  bounded live fan-out with full acceptance testing.
- **Request identity** now flows through the entire pipeline rather than
  being derived ad-hoc at the ledger boundary.
- **Secret references** use an explicit `SecretRef` enum (`environment`,
  `command`, `legacy_inline`, `none`) instead of raw plaintext values in
  configuration.

### Fixed

- Different credential references no longer share cache entries or in-flight
  coalescing (previously keyed only by URL and body fingerprint).
- Leader panic in the flight registry no longer leaves a stale flight entry
  that blocks subsequent identical requests.
- Waiter cancellation no longer corrupts the leader's upstream request.
- Secret values are never placed in logs, IDs, hashes, database diagnostics,
  or receipts.

### Summary

This release transforms Toche from a Claude Code-specific local gateway into a
safe multi-client AI workload runtime. One local Toche instance can now serve
Claude Code and Codex CLI simultaneously, with trust-domain isolation preventing
cross-client cache or flight sharing. The configuration schema, protocol-driver
architecture, and rerunnable setup engine establish the foundation for future
client and protocol support. All existing 1.0.x configurations migrate
automatically and the persistent two-terminal workflow remains first-class.

## [1.0.10] - 2026-07-17

### Fixed
- Windows npm installation now passes ZIP archive and destination paths to PowerShell through environment variables, preventing empty `Expand-Archive` arguments

### Added
- A Windows-only regression test that creates and extracts a ZIP archive through paths containing spaces
- Windows npm installer validation in both CI and the draft-release workflow

## [1.0.9] - 2026-07-17

### Fixed
- Renamed the npm package to `@nzkbuild/toche` after npm rejected the unscoped name as too similar to existing packages
- Updated installation, removal, verification, and recovery instructions for the public npm scope

### Changed
- Bumped Rust, npm, release workflow, test fixture, and public status metadata to 1.0.9

## [1.0.8] - 2026-07-17

### Fixed
- Release packaging now uploads only archives and checksum files instead of also passing build directories to GitHub
- Preserved the existing `v1.0.7` tag after it was created by an earlier draft-release attempt

### Added
- npm installer package that exposes the native binary as the global `toche` command on supported platforms

### Changed
- Reworked the README around measurable outcomes, numbered installation, normal daily use, and clearly optional commands
- Added npm package checks and an npm tarball to the draft release workflow

## [1.0.7] - 2026-07-17

### Fixed
- Clean public clones now include every filter definition required by `build.rs`
- Bash tool resolution now preserves subcommands so command-specific filters can match correctly
- Declared minimum Rust version corrected from 1.85 to 1.86 to match the locked dependency graph
- Repository-wide Rust formatting aligned with the enforced CI baseline

### Added
- GitHub Actions checks for formatting, Clippy, Linux tests, Windows tests, and the minimum Rust version
- Draft-first release workflow for Windows, Linux, Intel macOS, and Apple Silicon macOS archives with SHA-256 checksums
- Repository-owned status badge artwork and first public release notes
- Toche-owned Cargo and Git diff filters, bringing the committed built-in inventory to 65

## [1.0.6] - 2026-07-17

### Added
- Criterion benchmarks for request fingerprinting, tool-output reduction, workspace fingerprinting, and safe-response inspection

### Changed
- Expanded the README with the complete CLI surface, bypass headers, troubleshooting, and current pipeline documentation
- Bumped the package version to 1.0.6

### Repository maintenance
- Added the missing `v1.0.2` and `v1.0.3` tags at their original commits

## [1.0.5] - 2026-07-17

### Added
- Integration coverage for `status`, `doctor`, and connect/disconnect edge cases
- Regression coverage for routing detection through `env.ANTHROPIC_BASE_URL`

### Changed
- Added defense-in-depth ignore rules for local profile and backup files
- Documented the synchronous mutex choice used by request coalescing

## [1.0.4] - 2026-07-17

### Fixed
- Version alignment: Cargo.toml bumped from 1.0.0 to 1.0.4 (was frozen across 3 releases)
- 24 compiler warnings silenced across vendored and first-party code
- `toche doctor` now checks both `baseURL` and `env.ANTHROPIC_BASE_URL` (F2)
- No-backup disconnect path saves original upstream URL for recovery (F4)
- Disconnect cleans up empty `env: {}` object in settings.json (F5)
- Stale version strings removed from source comments and test data

### Added
- CHANGELOG.md (this file)
- docs/ARCHITECTURE.md: full pipeline and module documentation

### Changed
- README updated with all CLI commands, bypass headers, and pipeline docs

## [1.0.3] - 2026-07-16

### Fixed
- `extract_model()` replaced hand-rolled JSON char parser with serde_json (F1)
- Cache clear/evict now deletes orphaned CAS blob files (F3)
- `toche setup` refuses to overwrite existing profiles.toml without `--force` (F6)
- Added `/ready` endpoint with profile load verification (F7)

### Changed
- `toche connect` sets both `baseURL` and `env.ANTHROPIC_BASE_URL`
- Connect never overwrites existing backup (preserves original upstream config)
- Connect health-checks gateway before modifying settings
- `toche disconnect` cleans `env.ANTHROPIC_BASE_URL` and empty env objects

## [1.0.2] - 2026-07-16

### Fixed
- `resolve_command()` now correctly accesses `input.input.command` on Bash tool_use blocks
- Cargo test fixture includes compilation noise the filter actually strips
- `.akar/` added to `.gitignore`

## [1.0.1] - 2026-07-16

### Fixed
- Second-round audit: schema consistency, metrics completeness, test coverage

## [1.0.0] - 2026-07-15

### Added
- SQLite WAL-mode ledger with 90-day retention
- Pricing resolution with model name matching
- Request Shield: SHA-256 fingerprinting and single-flight coalescing
- Provider Cache Coordinator with observe/auto modes
- Safe Cache with workspace fingerprinting and CAS storage
- Context Reduction via TOML filter engine (63 filters)
- Efficiency profiles: normal, concise, careful
- Connect/disconnect CLI for Claude Code integration
- Feature bypass headers: `x-toche-bypass-*`
- Metrics dashboard (`toche stats`)
- `toche doctor` and `toche status` commands

## [0.9.0] - 2026-07-14

### Added
- Graphify CLI wrapper for knowledge graph queries

## [0.8.0] - 2026-07-14

### Added
- Session Continuity checkpoint system

## [0.7.0] - 2026-07-14

### Added
- Persistent Safe Cache for cross-session response reuse

## [0.6.0] - 2026-07-13

### Added
- Efficiency profiles: concise and careful instruction injection

## [0.5.0] - 2026-07-13

### Added
- Safe Context Reduction with RTK TOML filter engine

## [0.4.0] - 2026-07-12

### Added
- Request Shield: fingerprinting, coalescing, ledger tracking

## [0.3.0] - 2026-07-12

### Added
- Provider Cache Coordinator with breakpoint detection

## [0.2.0] - 2026-07-11

### Added
- Usage metering and cost estimation with embedded pricing map

## [0.1.0] - 2026-07-10

### Added
- Initial gateway: reverse proxy for Anthropic Messages API
- Profile-based upstream configuration
- TOML config loading with `TOCHE_CONFIG_DIR` support
