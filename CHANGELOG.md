# Changelog

All notable changes to Toche will be documented in this file.

## [1.0.4] — 2026-07-17

### Fixed
- Version alignment: Cargo.toml bumped from 1.0.0 to 1.0.4 (was frozen across 3 releases)
- 24 compiler warnings silenced across vendored and first-party code
- `toche doctor` now checks both `baseURL` and `env.ANTHROPIC_BASE_URL` (F2)
- No-backup disconnect path saves original upstream URL for recovery (F4)
- Disconnect cleans up empty `env: {}` object in settings.json (F5)
- Stale version strings removed from source comments and test data

### Added
- CHANGELOG.md (this file)
- docs/ARCHITECTURE.md — full pipeline and module documentation

### Changed
- README updated with all CLI commands, bypass headers, and pipeline docs

## [1.0.3] — 2026-07-16

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

## [1.0.2] — 2026-07-16

### Fixed
- `resolve_command()` now correctly accesses `input.input.command` on Bash tool_use blocks
- Cargo test fixture includes compilation noise the filter actually strips
- `.akar/` added to `.gitignore`

## [1.0.1] — 2026-07-16

### Fixed
- Second-round audit: schema consistency, metrics completeness, test coverage

## [1.0.0] — 2026-07-15

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

## [0.9.0] — 2026-07-14

### Added
- Graphify CLI wrapper for knowledge graph queries

## [0.8.0] — 2026-07-14

### Added
- Session Continuity checkpoint system

## [0.7.0] — 2026-07-14

### Added
- Persistent Safe Cache for cross-session response reuse

## [0.6.0] — 2026-07-13

### Added
- Efficiency profiles: concise and careful instruction injection

## [0.5.0] — 2026-07-13

### Added
- Safe Context Reduction with RTK TOML filter engine

## [0.4.0] — 2026-07-12

### Added
- Request Shield: fingerprinting, coalescing, ledger tracking

## [0.3.0] — 2026-07-12

### Added
- Provider Cache Coordinator with breakpoint detection

## [0.2.0] — 2026-07-11

### Added
- Usage metering and cost estimation with embedded pricing map

## [0.1.0] — 2026-07-10

### Added
- Initial gateway: reverse proxy for Anthropic Messages API
- Profile-based upstream configuration
- TOML config loading with `TOCHE_CONFIG_DIR` support
