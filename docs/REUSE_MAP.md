# Toche Reuse Map

Maps every reusable vendor component against each atomic roadmap release. Each entry names the source file(s), what to adapt, and the integration strategy.

Status: pre-implementation, derived from deep exploration of all five `vendor_reuse/` projects on 2026-07-15.

---

## 0.1.0 — Transparent Gateway

**Goal:** Claude Code operates normally through Toche with no caching or request rewriting.

**Critical finding:** No vendor project has an HTTP proxy or Anthropic-compatible streaming API. Toche must build this ground-up. However, several architectural patterns are reusable.

### Patterns to adapt

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| RTK | `src/main.rs` (Commands enum + subcommand dispatch) | Subcommand routing pattern for `toche setup`, `toche connect`, `toche disconnect`, `toche doctor`, `toche status` | Pattern only — adapt the Clap enum dispatch structure |
| RTK | `src/hooks/init.rs` (`atomic_write()`, `write_if_changed()`, `InitContext`, `PatchMode`) | Claude Code `settings.json` patching for `toche setup` import of gateway config | Adapt the atomic-write + backup + restore patterns. `NamedTempFile::persist()` for crash safety |
| RTK | `src/core/config.rs` (TOML config loading with Serde defaults) | Toche's own TOML config for profiles, upstream endpoints, model mappings | Pattern — hierarchical config with fallback defaults |
| RTK | `src/core/utils.rs` (`resolve_binary()`, `resolved_command()`, `tool_exists()`) | PATHEXT-aware tool discovery on Windows | Direct adaptation — these are general-purpose utilities |
| RTK | `src/hooks/hook_cmd.rs` (agent-specific JSON format handling, BOM stripping, `writeln!` discipline) | Reading and validating Claude Code hook stdin JSON during setup | Adapt the multi-agent format handling |
| caveman | `src/hooks/caveman-config.js` (`safeWriteFlag()`, symlink hardening) | Secure credential write during `toche setup` — prevents symlink-clobber attacks | Adapt to Rust: atomic tempfile + rename, O_NOFOLLOW equivalent on Windows |
| caveman | `bin/install.js` (settings.json merge via JSONC-tolerant read/write) | `toche setup` needs to read/write `~/.claude/settings.json` | Pattern — commented JSON handling. Rust equivalent: strip comments before serde_json |
| caveman | `bin/lib/settings.js` (`validateHookFields()`) | Defensive validation before writing to settings.json | Pattern — validate schema before write |

### Must build from scratch

- HTTP server (loopback `127.0.0.1:8743`) with Anthropic Messages streaming pass-through
- `toche setup` wizard (profile import, upstream detection, credential storage)
- `toche connect/disconnect claude` (backup + restore Claude Code config)
- `toche doctor`, `toche status`, foreground logs
- Named upstream profiles with raw model ID pass-through

---

## 0.2.0 — Usage Ledger

**Goal:** The user can explain where every request and token went.

### High-reuse release — ccusage and RTK provide strong foundations

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| ccusage | `crates/ccusage/src/pricing.rs` (~2764 lines) | PricingMap with 4-stage resolution: exact match → alias → models.dev API → built-in hardcoded. LiteLLM JSON format, tiered long-context pricing (OpenAI-style and marginal), speed multipliers, cache creation split (ephemeral 5m vs 1h) | **Heavily adapt into Rust.** The resolution pipeline, struct fields, and two-pricing-tier strategies are directly portable. Strip network fetch (not needed) and adapt built-in rates to Rust |
| ccusage | `crates/ccusage/src/cost.rs` | `calculate_cost_for_usage()` with CostMode dispatch, `tiered_cost()` for marginal long-context pricing | Adapt cost calculation logic into Toche's Meter module |
| ccusage | `crates/ccusage/src/summary.rs` | `UsageAccumulator`, `SessionAccumulator`, bucketing by day/week/month, date filtering/sorting, model alias application | Adapt aggregation patterns — Toche accumulates per-request, not from JSONL |
| ccusage | `crates/ccusage/src/adapter/jsonl.rs` (`records()`, memmem prefilter, lenient numeric/object/array deserialization) | Request/response stream capture format. Toche captures live streams, not reads files, but the structured record format is adaptable | Pattern — adapt JSONL record format for Toche's local ledger |
| ccusage | `crates/ccusage/src/output.rs` | Table rendering (normal + compact), JSON output with jq filter, project grouping, missing-pricing warnings | Adapt terminal output patterns for `toche stats`, `stats --session`, machine-readable JSON |
| ccusage | `crates/ccusage-terminal/src/lib.rs` | Column width calculation, table layout | Adapt — or use a Rust terminal-table crate directly |
| RTK | `src/core/tracking.rs` (SQLite schema, WAL mode, `busy_timeout=5000`, 90-day retention, `project_filter_params()` with GLOB, `TimedExecution` struct) | SQLite usage ledger | **Heavily adapt.** Schema, WAL mode, busy_timeout, retention, project-scoped filtering — all directly applicable. Extend schema to track: upstream requests, logical tokens, billed tokens, cache reads/writes, latency, errors, cost |
| RTK | `src/core/utils.rs` (`format_tokens()` K/M suffix, `human_bytes()`) | Token and byte formatting for display | Direct adaptation |
| RTK | `src/core/config.rs` (config loading pattern) | LEDGER_DB_PATH resolution (env var > config > default) | Pattern |

### Must build from scratch

- Live request interception (capture stream metadata without buffering)
- Per-profile, model, session, project reporting queries
- Cost calculation with explicit pricing overrides for custom model IDs
- Clear separation: measured vs estimated vs unknown cost
- `toche stats` CLI with JSON output

---

## 0.3.0 — Provider Cache Coordinator

**Goal:** Remaining upstream requests preserve or improve short-term provider cache eligibility.

### Partial reuse — cache coordination patterns exist but are scattered

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| CCUSAGE | Pricing struct fields: `cache_creation_input_token_cost`, `cache_read_input_token_cost`, `_above_200k` variants, `long_context_threshold`, `fast_multiplier` | Cache cost tracking — understanding what provider caches cost | Adapt pricing model fields for cache-read and cache-write cost reporting |
| CCUSAGE | Cache creation split: `CacheCreationRaw` with `ephemeral_5m_input_tokens` + `ephemeral_1h_input_tokens` | Anthropic's two-tier ephemeral cache durations | Adapt parsing of cache creation breakdown |
| RTK | `src/core/tee.rs` (file rotation, char-boundary-safe truncation, `human_bytes()`, `tee_and_hint()`) | Local storage of cached payloads with rotation/eviction | Adapt rotation + truncation patterns |
| RTK | `src/core/toml_filter.rs` (trust-gated loading — SHA-256 check before loading local files) | Provider capability detection files — verify integrity before trusting cached capability data | Pattern |

### Must build from scratch

- Provider capability detection (Anthropic explicit cache-control, OpenAI prompt caching)
- Stable request serialization and breakpoint policy
- Cache hit/write/miss diagnostic logging
- Observe-only recommendation mode before automatic mutation
- Per-profile bypass for upstreams that already manage caching

---

## 0.4.0 — Request Shield

**Goal:** Duplicate, concurrent, or retried requests do not automatically create duplicate upstream charges.

### Patterns exist but ground-up fingerprinting engine needed

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| RTK | `src/discover/lexer.rs` (`TokenKind` enum, quote-aware tokenizer, `split_for_permissions()`, `contains_unattestable_construct()`) | Shell command understanding for request fingerprint safety analysis | Adapt tokenizer to understand what's in a tool_use request — identify dangerous vs safe tool calls |
| RTK | `src/hooks/integrity.rs` (SHA-256 hash storage/verification, `sha256sum`-compatible format, read-only hash files) | Content-addressing for request fingerprints | Adapt hash computation and storage patterns |
| RTK | `src/hooks/permissions.rs` (multi-file rule loading, pattern matching with `*` wildcard, precedence: Deny > Ask > Allow > Default, compound command splitting) | Safety rules for replay decisions | Pattern — rule precedence and compound analysis |
| RTK | `src/hooks/rewrite_cmd.rs` (exit code protocol for hook decisions, `PermissionVerdict::Default` → "ask" to prevent auto-allow bypass) | Safety decision framework for replay | Pattern — never auto-allow uncertain decisions |

### Must build from scratch

- Canonical request fingerprinting (hash of: model, messages, tools, system prompt, stop sequences, temperature)
- Single-flight coalescing for identical in-flight requests
- Completed-stream capture and storage
- Idempotent retry replay for strictly identical safe requests
- Failure and partial-stream handling
- Explicit counters for coalesced, replayed, and forwarded requests

---

## 0.5.0 — Safe Context Reduction

**Goal:** Noisy tool output becomes compact while its full original remains recoverable.

### Highest-reuse release — RTK is purpose-built for this

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| RTK | `src/core/toml_filter.rs` (8-stage filter pipeline) | Deterministic context reduction pipeline: strip_ansi → replace → match_output → strip/keep_lines → truncate_lines_at → head/tail_lines → max_lines → on_empty | **Heavily adapt into Rust.** Pipeline architecture is directly reusable. Extend stages for Toche-specific reducers |
| RTK | `src/core/toml_filter.rs` (`Lossiness` enum: `None`, `Tail { tee_payload, tail_offset }`, `Whole`) | Lossiness tracking with tee recovery — "show filtered version, keep original for recovery" | Direct adaptation — this is the core of Toche's reduction model |
| RTK | `src/core/filter.rs` (Minimal/Aggressive comment stripping, 12+ languages, data format protection, `smart_truncate()`) | Source code comment filtering for tool output that contains code | Adapt filter strategies and language detection |
| RTK | `src/core/utils.rs` (`strip_ansi()`, `truncate()` multibyte-safe) | ANSI stripping and truncation primitives | Direct adaptation |
| RTK | `src/filters/*.toml` (63 built-in filter configs) | Pre-built filter configurations for common tools (git, cargo, npm, docker, terraform, etc.) | Adapt individual filter configs — the 8-stage DSL is the reuse target, individual configs are examples to port |
| RTK | `src/core/toml_filter.rs` (TOML DSL: `command_matches_filter()` with `RegexSet`, `deny_unknown_fields`, compile-time embedding via `build.rs`) | Filter configuration format and loading | Pattern — Toche could use TOML or adopt a programmatic approach |
| CCUSAGE | `rust/crates/ccusage/src/adapter/jsonl.rs` (byte-line JSONL parsing with memmem prefilter) | Reduced+raw output storage format — content-addressed blobs | Pattern — efficient binary search for stored artifacts |

### Must build from scratch

- `toche_expand` MCP tool or hook for recovering hidden details
- Per-command and global reduction bypass
- Side-by-side raw vs reduced token measurement in ledger
- Conservative fallback to raw output on parser uncertainty

---

## 0.6.0 — Efficiency Profiles

**Goal:** Users can reduce output and rework through explicit, reversible behaviour profiles.

### Reuses skill files as policy blueprints — adapt to Rust

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| caveman | `skills/caveman/SKILL.md` (6 intensity levels: lite, full, ultra, wenyan-lite, wenyan-full, wenyan-ultra) | Concision profile definitions — what each level strips vs preserves | Adapt level definitions into Rust config enum. Transform natural-language rules into structured filter parameters |
| caveman | `skills/caveman/SKILL.md` (auto-clarity rule: normal prose for security warnings, irreversible actions, user confusion) | Safety boundary for concision — when to drop the profile and speak normally | Adapt into profile engine: certain request types always bypass reduction |
| caveman | `skills/caveman/SKILL.md` (boundaries: code, commits, PRs written normal) | Scope limits — what content types are never reduced | Adapt into content-type detection rules |
| caveman | `src/rules/caveman-activate.md` (630-byte always-on rule snippet) | Compact policy representation for prompt injection | Pattern — how short can a policy be while remaining effective? |
| andrej-karpathy-skills | `skills/karpathy-guidelines/SKILL.md` (4 principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution) | Careful-work policy profile content | Adapt principles into structured policy rules. "Think before coding" → reduce exploration scope. "Simplicity first" → reduce over-engineering. "Surgical changes" → limit edit scope. "Goal-driven" → success criteria tracking |
| caveman | `src/hooks/caveman-mode-tracker.js` (slash-command activation, natural-language detection, per-turn reinforcement via `hookSpecificOutput`) | Profile activation/deactivation mechanism | Pattern — how to toggle profiles at runtime |
| caveman | `skills/caveman-compress/SKILL.md` (file compression with validation: headings, code blocks, URLs preserved) | Validation rules for compressed output | Adapt into output validation checks |

### Must build from scratch

- `normal`, `concise`, and `custom` response profiles as Rust enums
- Profile preview and per-session override
- Stable prompt placement to preserve provider prefix caching
- Measurement of output saved and its downstream context effect
- Profile removal without leaving hidden project instructions

---

## 0.7.0 — Persistent Safe Cache

**Goal:** Safe, unchanged work can be reused across sessions without an upstream request.

### Architecture patterns from ccusage + RTK tracking

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| RTK | `src/core/tracking.rs` (SQLite schema, WAL mode, busy_timeout, project-scoped queries with GLOB) | Cache metadata database — SQLite with the same robustness patterns | Adapt schema for content-addressed cache entries |
| RTK | `src/core/tee.rs` (rotation, char-boundary truncation, max files/max size limits) | Cache blob storage with eviction | Adapt rotation + size limits + truncation |
| CCUSAGE | Claude adapter dedup: (message_id + request_id) hashing + sidechain dedup | Deduplication strategy for cache entries | Pattern — compound key hashing |
| RTK | `src/hooks/integrity.rs` (SHA-256 verification, `IntegrityStatus` enum) | Cache entry integrity verification | Adapt hash verification pattern |
| RTK | `src/core/toml_filter.rs` (trust-gated loading + SHA-256 before loading) | Cache validation before replay — verify workspace state fingerprint matches | Pattern |

### Must build from scratch

- Content-addressed SQLite metadata + compressed blob storage
- Model, profile, endpoint, protocol, prompt, tool schema, and workspace fingerprints
- Exact safe-response reuse across sessions
- TTL, size limits, eviction, inspection, and manual invalidation
- Cache namespace per project and upstream model
- Default denial for tool calls, time-sensitive responses, uncertain workspace state, cross-model reuse
- `toche cache inspect`, `clear`, `why`

---

## 0.8.0 — Session Continuity

**Goal:** A new Claude Code session resumes from a compact, validated project checkpoint.

### Adapts caveman's hook state + RTK's file integrity patterns

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| caveman | `src/hooks/caveman-config.js` (`safeWriteFlag()`, `readFlag()`, atomic temp+rename, symlink hardening) | Checkpoint file write/read with crash safety | Adapt to Rust — atomic tempfile + rename, symlink protection on Windows |
| caveman | `src/hooks/caveman-mode-tracker.js` (flag file communication between hooks, per-turn state tracking) | Session state tracking via flag/marker files | Pattern — lightweight state communication without a daemon |
| RTK | `src/hooks/integrity.rs` (SHA-256 hash storage/verification, `IntegrityStatus`: Verified/Tampered/NoBaseline/NotInstalled/OrphanedHash) | Git and file-state validation on resume — hash workspace files, compare against checkpoint | Adapt hash computation + status enum |
| RTK | `src/hooks/hook_check.rs` (`HookStatus` enum: Ok/Outdated/Missing, version tracking, rate-limited warnings) | Stale checkpoint detection — version-tagged checkpoints with expiry | Pattern — version + expiry for state artifacts |
| RTK | `src/core/tracking.rs` (project-scoped queries with GLOB) | Scoping checkpoint data to project boundaries | Pattern — project isolation |

### Must build from scratch

- Structured checkpoint format: task, completed work, changed files, verification, open risks, next action
- `toche checkpoint`, `resume`, checkpoint history
- Deterministic facts collected from observed activity
- Optional model-assisted summarization (clearly marked as generated)
- Stale checkpoint detection on resume

---

## 0.9.0 — Project Knowledge Adapter

**Goal:** Claude can query a local project map before broadly rereading the repository.

### External integration — Graphify is the adapter target, not code to port

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| Graphify | `graphify/serve.py` (MCP server: 10 tools — `query_graph`, `get_node`, `get_neighbors`, `get_community`, `god_nodes`, `graph_stats`, `shortest_path`, `list_prs`, `get_pr_impact`, `triage_prs`) | MCP tool contract — Toche calls these tools through Graphify's MCP interface | **External adapter.** Toche writes an MCP client, not a graph engine. Never port Python graph code to Rust |
| Graphify | `graphify/cli.py` (CLI command dispatch: `graphify query "..."`, `graphify path A B`, `graphify explain X`) | CLI contract for when MCP is unavailable | Pattern — fall back to CLI if MCP isn't running |
| Graphify | `ARCHITECTURE.md` (pipeline: detect → extract → build → cluster → analyze → report) | Understanding the graph model to design smart queries | Documentation — informs Rust-side query construction |
| Graphify | `graphify/build.py` (node dedup, edge normalization, cross-language guards) | Knowledge of how nodes/edges relate — Toche sends well-formed queries | Documentation |
| Graphify | `graphify/security.py` (input validation: URLs, paths, labels, content size caps) | Security boundaries for queries | Pattern — validate all inputs to the graph adapter |

### Must build from scratch

- Optional Graphify installation detection and setup
- Toche adapter to Graphify CLI/MCP query operations
- Incremental graph refresh after repository changes
- Query result inclusion only when requested or policy-approved
- Source paths and confidence/provenance retained
- Graceful operation when Graphify is absent

---

## 1.0.0 — Stable Local Toche

**Goal:** The complete local product is safe, installable, recoverable, documented, and suitable for daily use.

### Integration of all prior patterns, plus:

| Source | File(s) | What to adapt | Strategy |
|--------|---------|---------------|----------|
| caveman | `bin/install.js` (complete install/uninstall flow, settings.json merge, agent detection, `--dry-run`, `--force`, `--list`, `--uninstall`, `--non-interactive`) | Toche installer — Windows-first, signed executable distribution | Adapt install/uninstall flow patterns. Symlink hardening for config writes. JSONC-tolerant settings handling |
| caveman | `bin/lib/settings.js` (`readSettings()`, `validateHookFields()`) | Settings validation before write — prevents one malformed hook from poisoning settings.json | Adapt — validate entire settings.json schema before merging |
| RTK | `src/hooks/init.rs` (full uninstall: remove hook script, integrity hash, @RTK.md reference, settings.json entry) | Complete restoration of pre-Toche Claude Code configuration on uninstall/disconnect | Pattern — thorough cleanup, no orphaned files |
| RTK | `src/hooks/integrity.rs` (SHA-256 hook verification, `runtime_check()` on startup) | Runtime integrity checks for Toche's own hooks and config | Pattern — verify own installation hasn't been tampered |
| RTK | `src/core/tracking.rs` (90-day retention, `cleanup_old()` called on each insert) | Database maintenance — automatic cleanup of old cache entries and usage data | Adapt retention + cleanup pattern |
| caveman | `src/hooks/caveman-config.js` (multi-source config resolution: env var → repo-local → user config → default) | Toche config resolution hierarchy | Pattern — predictable config layering |
| RTK | `src/core/config.rs` (TOML with Serde defaults for backward compatibility) | Stable configuration format with migration support | Pattern — new fields get defaults, old configs keep working |

### Must build from scratch

- Signed Windows installer and upgrade/uninstall path
- Foreground and background operation (service/daemon mode)
- Crash recovery and corruption handling for SQLite + blob storage
- Secure credential storage (DPAPI on Windows) and loopback authentication
- Cache privacy controls, export, deletion, and storage limits
- Compatibility matrix for supported Claude Code and upstream protocol versions
- End-to-end benchmark suite using fixed public fixtures
- Complete third-party notices and dependency inventory
- Feature-level bypass and full restoration of pre-Toche configuration

---

## Summary: Reuse intensity by release

| Release | Reuse level | Primary vendor(s) | What Toche builds new |
|---------|-------------|-------------------|----------------------|
| 0.1.0 Gateway | Low (patterns only) | RTK, caveman | HTTP server, streaming, setup wizard |
| 0.2.0 Ledger | **High** | ccusage, RTK | Live interception, reporting queries |
| 0.3.0 Cache Coord | Medium | ccusage, RTK | Provider capability detection, breakpoint policy |
| 0.4.0 Shield | Medium (patterns) | RTK | Fingerprinting engine, coalescing, stream capture |
| 0.5.0 Reduction | **Very High** | RTK | `toche_expand`, bypass system, token measurement |
| 0.6.0 Profiles | Medium | caveman, andrej-karpathy | Profile engine, prompt placement, savings measurement |
| 0.7.0 Cache | Medium | RTK, ccusage | Cache storage engine, fingerprint validation, invalidation |
| 0.8.0 Continuity | Medium | caveman, RTK | Checkpoint format, resume logic, state collection |
| 0.9.0 Knowledge | Low (external) | Graphify | MCP client adapter, detection/setup, graceful absence |
| 1.0.0 Stable | Medium (all) | caveman, RTK | Installer, upgrade, crash recovery, benchmarks |

## What NOT to reuse

These components exist in vendor projects but Toche should NOT adapt them:

1. **RTK's CLI proxy architecture** — RTK is a command interceptor, not an HTTP gateway. The entire HTTP/streaming layer is Toche's original work.
2. **ccusage's JSONL file reading** — Toche captures live streams, not reads post-hoc log files. The data model adapts; the I/O model doesn't.
3. **Graphify's Python graph engine** — Never port Python to Rust. Use Graphify as an external MCP/CLI integration.
4. **caveman's JavaScript hook files** — These are Node.js. Toche's hooks will be Rust-native. Patterns adapt; code doesn't.
5. **Any vendor's async runtime** — RTK is explicitly single-threaded. Toche needs async for the HTTP gateway. Use tokio; don't adapt RTK's sync patterns for the proxy layer.
6. **RTK's `lazy_static!` for regex** — This is an RTK-specific performance pattern. Toche may use `once_cell`/`LazyLock` or compile regex differently. Don't cargo-cult the pattern without measuring.
