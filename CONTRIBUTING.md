# Contributing to Toche

Thanks for your interest in contributing. This document covers setup,
conventions, and the pull request workflow.

## Setup

You need Rust 1.86 or newer (edition 2024). The project uses the standard
Cargo toolchain.

```shell
git clone https://github.com/nzkbuild/toche.git
cd toche
cargo build
cargo test
```

No additional system dependencies are required. SQLite is bundled through
`rusqlite`. The npm installer tests require Node.js 18 or newer.

## Development workflow

1. Find or open an issue describing the change.
2. Create a branch from `master`.
3. Make your changes.
4. Run the full check suite (see below).
5. Open a pull request.

## Before submitting

Run these commands and ensure they all pass:

```shell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
npm run test:npm
```

If your change affects the npm installer, also run:

```shell
npm run check:package
```

## Commit conventions

Commits follow the [Conventional Commits](https://www.conventionalcommits.org/)
format:

```
type(scope): description
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `perf`.

Scopes reflect the module or subsystem being changed: `gateway`, `shield`,
`protocol`, `reduce`, `setup`, `config`, `cli`, `cache`, `meter`, `identity`,
`integrations`, `stats`, `release`, `roadmap`.

Each commit must:

- Represent one coherent change.
- Include its tests.
- Preserve compilation.
- Preserve relevant existing tests.
- Avoid unrelated formatting.

## Code conventions

- **Rust edition 2024** throughout.
- **No warnings.** Clippy runs with `-D warnings` and must be clean.
- **No unsafe code** except in test-only `TOCHE_CONFIG_DIR` overrides
  (guarded by `// SAFETY:` comments).
- **Tests live alongside the code they test** in `#[cfg(test)] mod tests`
  blocks, or in `tests/` for integration tests.
- **Secrets are never printed.** The `SecretRef` type redacts inline values
  in `Debug` and `Display` output.
- **Credentials never appear in IDs, hashes, logs, or receipts.** Trust
  domains are derived from secret reference identity (e.g. `env:KEY_NAME`),
  never from raw credential values.

## Architecture principles

### Reuse-first

Implementation preference order:

1. Maintained versioned dependency.
2. Narrow adapter around maintained upstream work.
3. Minimal vendored data, fixture, or algorithm.
4. Toche-specific original implementation only when necessary.

Every reused source must be pinned and attributed. See
`docs/roadmaps/v1.1.0/REUSE_MANIFEST.toml`.

### Protocol boundary

Protocol-specific logic is isolated behind the `Protocol` trait in
`src/protocol/`. The universal pipeline (coalescing, safe cache, reduce,
efficiency, ledger) is protocol-agnostic.

Rules:

- Raw request bytes are authoritative.
- Unknown fields must survive unchanged.
- Protocol-specific transformations cannot execute on another protocol.
- Unsupported content passes through unchanged.
- Transform failure returns the original request.

### Trust isolation

Different credential references never share cache entries or in-flight
coalescing. When adding state that could cross client boundaries, check
that it is keyed by trust domain.

## Project structure

```
src/
├── main.rs            — CLI routing
├── lib.rs             — Public module declarations
├── gateway/           — Axum HTTP server, routes, health/ready/status
├── protocol/          — Protocol trait + anthropic + openai_responses drivers
├── shield/            — Request fingerprinting + coalescing store
├── safe_cache/        — Persistent cross-session response cache
├── reduce/            — TOML filter engine + CAS storage
├── efficiency/        — Instruction injection
├── cache/             — Ephemeral prompt cache breakpoint injection
├── meter/             — Ledger DB, pricing, request recording
├── identity/          — Runtime, request, and trust-domain identity
├── config/            — Config types, loader, migration, resolver, utils
├── integrations/      — Client integrations (claude, codex)
├── setup/             — Setup transaction engine
├── continuity/        — Session checkpoint save/restore
├── graphify/          — Knowledge graph CLI adapter
├── profiles/          — Legacy profile types (1.0.x compatibility)
└── cli/               — User-facing command implementations
```

## Testing

- **Unit tests** use `#[cfg(test)] mod tests` within source files.
- **Integration tests** live in `tests/` as separate test targets.
- **Property tests** use `proptest` for invariants (preservation,
  fingerprinting, trust isolation, config round-trips).
- **Snapshot tests** use `insta` for setup previews, status output, and
  config patches.
- **Mock upstreams** use `wiremock` for Anthropic, OpenAI, SSE, and failure
  simulation.

Run specific test targets:

```shell
cargo test --test gateway_integration
cargo test --test m12_failure_shard1
cargo test --test m13_migration_audit
```

## Pull request workflow

1. Open a PR against `master`.
2. The PR description should explain what changed and why.
3. CI must pass: format, clippy, tests, npm installer tests.
4. A maintainer will review and merge.

## Documentation

When adding or changing a user-facing command, update the README command
reference and the changelog (under the Unreleased section).

When adding a new milestone, create an evidence file in
`docs/roadmaps/v1.1.0/evidence/` recording the commit SHA, files changed,
tests passed, and gate status.

## License

By contributing, you agree that your contributions will be licensed under
the Apache License 2.0, the same license that covers the project.
