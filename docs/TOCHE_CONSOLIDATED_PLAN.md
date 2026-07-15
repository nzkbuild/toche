# Toche Consolidated Product Plan

Status: pre-implementation source of truth  
Target: local, Windows-first release for Claude Code  
Roadmap: `0.1.0` through `1.0.0`

## 1. Product definition

Toche is a local context-efficiency gateway placed between Claude Code and any supported upstream API, router, or proxy.

```text
Claude Code -> Toche -> 9Router or another gateway -> model provider
```

Toche reduces AI usage through five operations:

1. **Observe** every request, token category, model, cache event, and cost.
2. **Reduce** redundant tool output and unnecessary context before it reaches the model.
3. **Remember** validated project and session state across days.
4. **Reuse** safe local results before contacting the upstream provider.
5. **Coordinate** the provider's short-lived prompt cache for remaining requests.

Short definition:

> Short-term cache from the provider. Long-term reuse and context efficiency from Toche.

Toche does not replace 9Router. It can use 9Router as its upstream and preserve raw model IDs such as `cx/gpt-5-sol`.

## 2. Local user experience

Initial setup:

```powershell
toche setup
```

Toche detects and offers to import the user's existing Claude Code gateway configuration:

- upstream endpoint;
- authentication method and credential;
- Anthropic or OpenAI-compatible protocol;
- exact model IDs and caller prefixes;
- Opus, Sonnet, Haiku, and custom mappings;
- required custom headers.

Toche backs up the existing Claude Code configuration, stores the upstream credential securely, and changes Claude Code's base URL to the local gateway.

Normal usage:

```powershell
# Terminal 1
toche

# Terminal 2
claude --dangerously-skip-permissions
```

Default endpoint:

```text
http://127.0.0.1:8743
```

The endpoint binds only to loopback by default. Toche does not control Claude Code's local permission mode. It only handles API transport, routing, measurement, reduction, and caching.

Docker is not required for personal use. The normal distribution is one native executable. Docker remains a post-1.0 deployment option for shared servers.

## 3. Architecture boundary

Request path:

```text
Claude Code
  -> Anthropic-compatible local ingress
  -> canonical request fingerprint
  -> safe local reuse decision
  -> deterministic context reduction
  -> optional project-context retrieval
  -> provider cache coordination
  -> upstream protocol adapter
  -> 9Router/provider
  -> streamed response capture
  -> usage ledger and persistent cache
  -> Claude Code
```

Primary modules:

| Module | Responsibility |
| --- | --- |
| Gateway | Local Anthropic-compatible API and streaming |
| Profiles | Endpoints, authentication, headers, protocols, and raw model IDs |
| Adapters | Anthropic pass-through and OpenAI-compatible translation |
| Meter | Requests, tokens, costs, cache reads/writes, latency, and savings |
| Reducer | Deterministic compression of supported tool results |
| Cache | Content-addressed storage, validation, TTL, and replay policy |
| State | Workspace fingerprints and session checkpoints |
| Context | Optional project graph and retrieval integrations |
| Policy | Safety, cache eligibility, bypass, and feature profiles |

### Core implementation choice

Use **Rust** for the Toche executable. RTK is predominantly Rust and ccusage now exposes substantial Rust implementation, so Rust gives Toche the best chance of reusing their maintained components without cross-language rewrites. Graphify remains an optional external Python CLI/MCP integration behind an adapter. SQLite provides local metadata storage; compressed content-addressed blobs hold larger cached payloads.

## 4. Implementation strategy: reuse first

Toche will not recreate mature functionality when a suitable maintained implementation is available.

Decision order for each capability:

1. Use the upstream project as a versioned dependency when it exposes a stable library.
2. Integrate its CLI, MCP server, hook, or documented API behind a Toche adapter.
3. Vendor only the smallest required modules when direct integration is impossible.
4. Implement original code only for Toche-specific orchestration or when no reusable component is suitable.

Rules:

- Pin reused dependencies to a release or commit.
- Preserve copyright, licence, and notice files.
- Record every reused component in `THIRD_PARTY_NOTICES.md`.
- Keep external integrations behind interfaces so they can be upgraded or replaced.
- Avoid permanent forks where a small upstream contribution or adapter is sufficient.
- Keep modifications narrow and covered by compatibility tests.
- Never claim third-party benchmark savings as Toche results. Measure Toche's own observed savings.

### Reuse matrix

| Project | Planned use | Integration method | Current licence status |
| --- | --- | --- | --- |
| ccusage | Pricing, usage categories, reporting concepts, compatible parsing components | Reuse suitable Rust packages/modules where stable | MIT |
| RTK | Command/tool-output reduction and Claude Code hook behaviour | Prefer library/module reuse; otherwise manage the RTK binary through an adapter | Apache-2.0 |
| Graphify | Local project graph and query service | Optional CLI/MCP adapter; do not rebuild its Python graph engine | MIT |
| andrej-karpathy-skills | Optional careful-work policy profile | Licensed policy pack with attribution and user opt-in | MIT |
| Obsidian skills | Future knowledge-source conventions | Post-1.0 adapter reference | MIT |
| Caveman Claude | General idea of concise responses | Original Toche policy only; do not copy repository material without a licence | No licence visible in the linked repository as of 2026-07-15 |

MIT and Apache-2.0 components can be used together if their notice and attribution requirements are preserved. A final distribution licence for Toche must be selected before public release.

## 5. Cache and reduction model

Toche has two distinct savings paths.

### Local Toche hit

The upstream provider is not called.

```text
Upstream requests: 0
Upstream prompt tokens: 0
Upstream completion tokens: 0
```

### Toche miss with provider cache hit

The upstream request still happens, but the provider may process repeated input at its cached-token rate.

Provider dashboards may continue showing logical prompt tokens even when those tokens are billed at a discount. Toche must report logical tokens, billed categories, and locally avoided tokens separately.

### Replay safety

Before `1.0.0`, Toche never locally replays a response containing `tool_use`, destructive actions, or an uncertain state dependency. Structured tool-call replay is explicitly out of scope for the first stable release.

Tool-result reduction stores the full original output locally. A compact result includes an opaque reference that can be expanded through a Toche tool when the model needs the omitted detail.

## 6. Atomic release roadmap

Each release has one theme, an independently useful outcome, and a hard acceptance gate. A release does not begin until the previous gate passes.

### `0.1.0` — Transparent Gateway

**Outcome:** Claude Code operates normally through Toche with no caching or request rewriting.

Scope:

- Windows-first single executable.
- `toche setup` imports an existing Claude Code endpoint, credential type, headers, and model mappings.
- Local endpoint on `127.0.0.1:8743`.
- Named upstream profiles.
- Raw model IDs, including slash-prefixed caller IDs, pass through unchanged.
- Anthropic Messages streaming pass-through.
- Configuration backup, `toche connect claude`, and `toche disconnect claude`.
- `toche doctor`, `status`, and foreground logs.

Acceptance gate:

- Claude Code completes a representative tool-using session through Toche and directly through the same upstream with equivalent protocol behaviour.
- Disconnect restores the exact previous Claude Code configuration.
- Toche performs no cache, compression, or semantic modification.

### `0.2.0` — Usage Ledger

**Outcome:** The user can explain where every request and token went.

Scope:

- Live request ledger.
- Per-profile, model, session, and project reporting.
- Logical input, output, provider cache-write, and provider cache-read categories.
- Latency, errors, retries, and streaming completion state.
- Cost calculation with explicit pricing overrides for custom model IDs.
- `toche stats`, `stats --session`, and machine-readable JSON output.
- Clear separation between measured, estimated, and unknown cost.

Reuse target: ccusage reporting and pricing components where technically suitable.

Acceptance gate:

- Recorded totals reconcile with upstream usage fields for a fixed trace.
- Unknown custom-model pricing never produces invented cost.
- The ledger adds no prompt or completion tokens.

### `0.3.0` — Provider Cache Coordinator

**Outcome:** Remaining upstream requests preserve or improve short-term provider cache eligibility.

Scope:

- Provider capability detection and explicit override.
- Anthropic automatic/explicit cache-control support.
- Stable request serialization and breakpoint policy.
- Pass-through of upstream cache usage fields.
- Cache hit/write/miss diagnostics.
- Observe-only recommendation mode before automatic mutation.
- Per-profile bypass for gateways that already manage caching.

Acceptance gate:

- Repeated-prefix fixtures demonstrate provider cache reads without altering response semantics.
- Unsupported upstreams remain untouched.
- Toche reports provider cache discounts without claiming that logical tokens disappeared.

### `0.4.0` — Request Shield

**Outcome:** Duplicate, concurrent, or retried requests do not automatically create duplicate upstream charges.

Scope:

- Canonical request fingerprinting.
- Single-flight coalescing for identical in-flight requests.
- Completed-stream capture.
- Idempotent retry replay for strictly identical safe requests.
- Failure and partial-stream handling.
- Explicit counters for coalesced, replayed, and forwarded requests.
- No persistent semantic matching.

Acceptance gate:

- Concurrent identical fixtures produce one upstream request.
- Interrupted-client retry reuses a fully captured safe response.
- Partial or tool-using responses are never incorrectly replayed.

### `0.5.0` — Safe Context Reduction

**Outcome:** Noisy tool output becomes compact while its full original remains recoverable.

Scope:

- RTK-backed reduction for supported command/tool outputs.
- Deterministic reduction so repeated prompts remain provider-cache friendly.
- Full raw output stored locally by content hash.
- `toche_expand` integration for recovering hidden details.
- Per-command and global bypass.
- Side-by-side raw versus reduced token measurement.
- Conservative fallback to raw output on parser uncertainty.

Reuse target: RTK filters and hook/integration mechanisms under Apache-2.0, with required notices.

Acceptance gate:

- Golden fixtures retain failures, exit codes, paths, diagnostics, and non-repeated warnings.
- Every reduced result can recover its byte-identical original.
- Unsupported or ambiguous output passes through unchanged.

### `0.6.0` — Efficiency Profiles

**Outcome:** Users can reduce output and rework through explicit, reversible behaviour profiles.

Scope:

- `normal`, `concise`, and `custom` response profiles.
- Concision applies to explanatory text, not code, errors, security warnings, or destructive-action details.
- Optional careful-work policy based on think-first, surgical-change, and verify-outcome principles.
- Profile preview and per-session override.
- Stable prompt placement to preserve provider prefix caching.
- Measurement of output saved and its downstream context effect.

Reuse target: licensed andrej-karpathy-skills material with attribution. Caveman source is not copied unless a licensed source is identified.

Acceptance gate:

- Profiles are opt-in and removable without leaving hidden project instructions.
- Technical fixtures preserve required code and diagnostic detail.
- Reported savings use observed token counts, not a fixed percentage claim.

### `0.7.0` — Persistent Safe Cache

**Outcome:** Safe, unchanged work can be reused across sessions without an upstream request.

Scope:

- Content-addressed SQLite metadata and compressed blob storage.
- Model, profile, endpoint, protocol, prompt, tool schema, and relevant workspace fingerprints.
- Exact safe-response reuse across sessions.
- TTL, size limits, eviction, inspection, and manual invalidation.
- Cache namespace per project and upstream model.
- Default denial for tool calls, time-sensitive responses, uncertain workspace state, and cross-model reuse.
- `toche cache inspect`, `clear`, and `why`.

Acceptance gate:

- Valid fixture returns locally with zero upstream request and zero upstream tokens.
- Any relevant file, model, tool schema, or policy change invalidates the fixture.
- No tool-call response is locally replayed.

### `0.8.0` — Session Continuity

**Outcome:** A new Claude Code session resumes from a compact, validated project checkpoint instead of rediscovering state.

Scope:

- Structured checkpoint containing task, completed work, changed files, verification, open risks, and next action.
- Git and file-state validation on resume.
- `toche checkpoint`, `resume`, and checkpoint history.
- Deterministic facts collected from observed activity where possible.
- Optional model-assisted summarisation clearly marked as generated.
- Stale checkpoint detection.

Acceptance gate:

- Resume rejects or marks stale facts after conflicting workspace changes.
- Checkpoint size is bounded and measured.
- A fixture can resume with fewer reread tokens than a cold session without hiding unverified state.

### `0.9.0` — Project Knowledge Adapter

**Outcome:** Claude can query a local project map before broadly rereading the repository.

Scope:

- Optional Graphify installation detection and setup.
- Toche adapter to Graphify CLI/MCP query operations.
- Incremental graph refresh after repository changes.
- Query results included only when requested or policy-approved.
- Source paths and confidence/provenance retained.
- Graceful operation when Graphify is absent.

Reuse target: Graphify as an external versioned integration rather than rebuilding its graph engine.

Acceptance gate:

- A code-navigation fixture retrieves the correct files and relationships through the graph.
- Disabling the adapter restores normal Claude Code behaviour.
- Code indexing remains local unless the user explicitly configures a remote semantic backend.

### `1.0.0` — Stable Local Toche

**Outcome:** The complete local product is safe, installable, recoverable, documented, and suitable for daily use.

Scope:

- Stable configuration and database migrations.
- Signed Windows installer and upgrade/uninstall path.
- Foreground and background operation.
- Crash recovery and corruption handling.
- Secure credential storage and loopback authentication.
- Cache privacy controls, export, deletion, and storage limits.
- Compatibility matrix for supported Claude Code and upstream protocol versions.
- End-to-end benchmark suite using fixed public fixtures.
- Complete third-party notices and dependency inventory.
- Setup, troubleshooting, recovery, and architecture documentation.
- Feature-level bypass and full restoration of pre-Toche Claude Code configuration.

Acceptance gate:

- Fresh install to first Claude Code request succeeds through guided setup.
- Upgrade preserves profiles, usage history, and valid cache entries.
- Uninstall or disconnect restores the user's previous routing configuration.
- Fixed benchmarks publish request, logical-token, billed-token, latency, and local-reuse results separately.
- No unsafe tool-call replay is enabled.

## 7. Pre-1.0 exclusions

The following are intentionally postponed:

- Shared or public Toche Pool.
- Multi-tenant hosted service.
- Semantic response replay.
- Cross-user cache sharing.
- Cross-model response reuse.
- Automatic replay of model tool calls.
- Obsidian and general document-vault adapters.
- Complex automatic cheap-model routing.
- Docker as the default personal installation.

These may become `1.1+` work only after local correctness and measured savings are proven.

## 8. Success metrics

Toche reports these independently rather than collapsing them into one marketing percentage:

- Claude Code requests received.
- Upstream requests forwarded.
- Requests coalesced or locally replayed.
- Raw logical input and output tokens.
- Provider cache writes and reads.
- Tokens removed by deterministic reduction.
- Tokens avoided by local reuse.
- Raw output retained and expanded.
- Measured or estimated upstream cost.
- Latency saved on local hits.
- Invalidated and rejected cache candidates.

Primary `1.0.0` promise:

> Toche never claims a saving it cannot explain from recorded evidence.

## 9. Immediate next action

Before implementation, complete a dependency-adoption audit for ccusage, RTK, and Graphify:

1. Identify stable reusable packages, CLI contracts, and extension points.
2. Record exact versions, licences, notices, and transitive constraints.
3. Decide dependency versus adapter versus minimal vendoring for each project.
4. Produce protocol fixtures from the user's current Claude Code -> 9Router setup.
5. Begin only `0.1.0`; do not implement later release behaviour behind unfinished flags.
