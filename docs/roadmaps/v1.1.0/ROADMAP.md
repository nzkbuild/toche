# Toche 1.1.0 — Frozen Execution Roadmap

**Release theme:** Multi-Client Runtime
**Roadmap status:** Contract frozen; implementation adaptive
**Development base:** Public `v1.0.10`, never local `v1.0.6`
**Target version:** `1.1.0`
**Primary implementation language:** Rust
**Public package:** `@nzkbuild/toche`
**Installed command:** `toche`

---

# 1. Mandatory context for Claude Code

The current local workspace may still report `1.0.6`. That state is stale.

Changes were completed and published outside the current local workspace:

| Version  | Published outcome                                                                          |
| -------- | ------------------------------------------------------------------------------------------ |
| `1.0.7`  | CI and release workflow corrections                                                        |
| `1.0.8`  | Public release packaging, npm installer, committed filter inventory, release documentation |
| `1.0.9`  | npm package changed to `@nzkbuild/toche`                                                   |
| `1.0.10` | Windows npm extraction fix and Windows installer regression coverage                       |

Claude must not reconstruct these changes manually from summaries.

Claude must fetch and use the actual public Git history and exact `v1.0.10` source.

## Prohibited reconciliation actions

Claude must not:

* start `1.1.0` work from `v1.0.6`;
* recreate `1.0.7`–`1.0.10` manually;
* move or rewrite existing tags;
* use `git reset --hard`;
* force-push;
* silently delete local modifications;
* overwrite a dirty working tree;
* assume local documentation is newer than GitHub;
* bump the version to `1.1.0` before the release gate.

---

# 2. Release contract

Toche `1.1.0` must transform Toche from a Claude-specific local gateway into a safe multi-client AI workload runtime.

The public promise is:

> One local Toche runtime can serve multiple simultaneous AI clients and sessions, isolate their trust boundaries, share only provably identical work, preserve protocol-specific behaviour, and report what happened honestly.

## Required user workflows

### Persistent two-terminal workflow

This remains first-class:

```shell
# Terminal 1
toche

# Terminal 2
claude --dangerously-skip-permissions
```

Users may launch more clients while the same Toche runtime remains active:

```shell
# Terminal 3
claude

# Terminal 4
codex
```

### Managed workflow

This is an optional convenience:

```shell
toche run claude -- --dangerously-skip-permissions
toche run codex
```

The managed workflow must use the same runtime, configuration, protocol handling, trust isolation, ledger and optimization pipeline as persistent mode.

It must not become a second implementation.

---

# 3. Frozen scope

Toche `1.1.0` must deliver:

1. Reconciliation from public `v1.0.10`.
2. Guided and rerunnable `toche setup`.
3. Multiple simultaneous clients and sessions.
4. Separate integrations, upstreams, policies and runtime configuration.
5. Explicit trust-domain isolation.
6. Corrected concurrent request coalescing.
7. Lossless protocol-driver boundaries.
8. Complete Anthropic Messages support.
9. OpenAI Responses pass-through support.
10. Claude Code integration.
11. Codex CLI integration.
12. Multi-client status, statistics and evidence.
13. Backward-compatible migration from the `1.0.x` configuration and database.
14. Reuse and licensing evidence for every adapted or vendored source.

---

# 4. Explicit exclusions

The following are outside `1.1.0`:

* provider account management;
* automatic provider selection;
* provider fallback;
* model load balancing;
* account rotation;
* Toche Cloud;
* Toche Pool;
* user billing;
* shared remote caches;
* organization accounts;
* hosted telemetry;
* Anthropic-to-OpenAI translation;
* OpenAI-to-Anthropic translation;
* Gemini protocol support;
* complete OpenCode support;
* semantic response caching;
* cross-model response reuse;
* automatic replay of tool calls;
* plugin marketplace;
* full OpenTelemetry exporting;
* Graphify expansion;
* checkpoint expansion;
* new behavioural prompt modes;
* aggressive response-style injection;
* broad RTK filter expansion.

Interfaces may leave room for future work, but excluded features must not become partially implemented hidden scope.

---

# 5. Architecture boundary

Toche must use:

```text
Universal control plane
+
Protocol-specific payload plane
```

## Universal control plane

Shared concepts include:

* runtime identity;
* integration identity;
* request identity;
* client instance identity when observable;
* session or conversation identity when observable;
* workspace identity;
* trust domain;
* upstream identity;
* requested and reported model;
* configuration snapshot;
* timing;
* usage;
* errors;
* optimization decisions;
* measurement confidence.

## Protocol-specific payload plane

Payload handling remains separate:

```text
protocol/anthropic
protocol/openai_responses
```

The original request bytes are authoritative.

Unknown fields must survive unchanged.

Toche must not reduce all protocols to a lossy universal message schema.

## Required fallback rule

```text
Unknown field           → preserve
Unsupported content     → raw pass-through
Parser uncertainty      → raw pass-through
Transform failure       → original request
Optional subsystem fail → continue without that optimization
Protocol violation      → explicit error
```

---

# 6. Reuse policy

Toche is reuse-first.

Implementation preference order:

1. Maintained versioned dependency.
2. Narrow adapter around maintained upstream work.
3. Minimal vendored data, fixture or algorithm.
4. Toche-specific original implementation only when necessary.

Every reused source must be pinned and attributed.

## Reuse actions

Four actions are permitted:

### `DEPEND`

Add a maintained package as a normal or development dependency.

### `ADAPT`

Study or port a narrowly defined upstream implementation into Toche architecture.

Adapted code must be reduced to the required capability and rewritten around Toche’s interfaces and tests.

### `VENDOR`

Commit a pinned dataset, fixture corpus or declarative asset required at build or runtime.

### `REFERENCE`

Use architecture, vocabulary or failure patterns without importing source code.

“Reuse” never means copying an entire application into Toche.

---

# 7. Frozen reuse map

## Direct dependencies

| Work                  | Source             |     Action | Purpose                                                                       |
| --------------------- | ------------------ | ---------: | ----------------------------------------------------------------------------- |
| Interactive setup     | `inquire`          |     DEPEND | Selection, multiselect, confirmation, validation and secret-safe prompts      |
| TOML preservation     | `toml_edit`        |     DEPEND | Modify Codex and Toche TOML without destroying comments or ordering           |
| Runtime IDs           | `uuid` with UUIDv7 |     DEPEND | Time-sortable runtime, request and session identities                         |
| HTTP middleware       | `tower-http`       |     DEPEND | Request IDs, tracing and controlled timeout middleware                        |
| Mock upstreams        | `wiremock-rs`      | DEV DEPEND | Anthropic/OpenAI/SSE and failure simulation                                   |
| Snapshot testing      | `insta`            | DEV DEPEND | Setup previews, status, stats, receipts and config patches                    |
| Property testing      | `proptest`         | DEV DEPEND | Preservation, fingerprinting, trust isolation and reversible patch invariants |
| Concurrency modelling | `loom`             | DEV DEPEND | Extracted flight-registry concurrency verification                            |
| Licence policy        | `cargo-deny`       |    CI TOOL | Licences, advisories, duplicates and source restrictions                      |
| Licence report        | `cargo-about`      |    CI TOOL | Generated Rust dependency notices                                             |

All resolved versions and licences must be recorded by the reuse-lock milestone.

## Narrow adaptations

| Work                        | Source         |          Action | Exact reuse boundary                                                                              |
| --------------------------- | -------------- | --------------: | ------------------------------------------------------------------------------------------------- |
| Multi-client config editing | CC Switch      |           ADAPT | Client config paths, structured JSON/TOML subset detection, merge and remove algorithms           |
| OpenAI Responses behaviour  | Official Codex |           ADAPT | Request/event types, SSE state-machine patterns, provider configuration expectations and fixtures |
| Usage aggregation           | ccusage        |           ADAPT | Session/project/model grouping concepts and missing-price handling                                |
| Reduction inventory         | RTK            | CONTINUE VENDOR | Existing pinned declarative filters and provenance; no wholesale runtime adoption                 |

## Vendored data and fixtures

| Work                    | Source                                           | Action | Boundary                                |
| ----------------------- | ------------------------------------------------ | -----: | --------------------------------------- |
| Model pricing           | Portkey Models or other approved pinned source   | VENDOR | Generated compact pricing snapshot only |
| Responses compatibility | Official Codex                                   | VENDOR | Minimal protocol fixtures only          |
| Anthropic edge cases    | Permissively licensed proxy fixtures where valid | VENDOR | SSE/tool/image/error fixtures only      |

## References only

| Source                          | Use                                                                                                   |
| ------------------------------- | ----------------------------------------------------------------------------------------------------- |
| TensorZero                      | Request identity, episode/session grouping, configuration provenance and time-to-first-token concepts |
| OpenTelemetry GenAI conventions | Shared field names for model, provider, usage, duration, conversation and finish status               |
| 9Router                         | Gateway/upstream behaviour and real-world compatibility scenarios                                     |
| LiteLLM                         | Provider translation, routing and pricing patterns                                                    |
| Bifrost                         | Multi-provider gateway failure and fallback patterns                                                  |
| Portkey Gateway                 | Gateway configuration and observability patterns                                                      |
| Envoy AI Gateway                | Protocol-aware gateway architecture patterns                                                          |

These full gateways must not become Toche dependencies in `1.1.0`.

## Toche-owned original work

The following are Toche’s actual product and must be implemented locally:

* runtime registry;
* integration and upstream resolver;
* trust-domain derivation;
* Toche protocol-driver interface;
* Toche evidence collector;
* multi-client ledger;
* safe flight registry;
* Toche setup transaction coordinator;
* Toche-specific response reuse rules;
* Toche-specific optimization policy;
* runtime status and statistics.

---

# 8. Source and licence governance

Create:

```text
docs/roadmaps/v1.1.0/REUSE_LOCK.md
docs/roadmaps/v1.1.0/REUSE_MANIFEST.toml
```

Each reused item must record:

```toml
[[sources]]
project = ""
repository = ""
commit = ""
license = ""
action = "depend | adapt | vendor | reference"
source_files = []
toche_files = []
purpose = ""
modified = true
verification = ""
```

A GitHub licence badge is not sufficient evidence.

Claude must inspect the actual licence file at the pinned commit.

No adapted or vendored code may enter the implementation branch before its reuse-manifest entry is approved and committed.

---

# 9. Development discipline

## Branch

After reconciliation:

```shell
git switch -c feat/1.1.0-multi-client-runtime v1.0.10
```

## Atomic commit rules

Each commit must:

* represent one coherent change;
* include its tests;
* preserve compilation;
* preserve relevant existing tests;
* use a conventional commit message;
* avoid unrelated formatting;
* update evidence when completing a gate.

Examples:

```text
chore(sync): reconcile workspace to v1.0.10
docs(roadmap): freeze 1.1.0 release contract
chore(reuse): lock approved upstream sources
refactor(config): separate integrations from upstreams
feat(setup): add environment discovery plan
feat(runtime): introduce trust domain identity
fix(shield): isolate flights by trust domain
refactor(protocol): isolate anthropic request handling
feat(protocol): add openai responses pass-through
feat(integration): configure codex through setup
feat(stats): report multi-client evidence
test(release): add 1.1.0 acceptance matrix
```

## Evidence records

Every milestone ends with:

```text
docs/roadmaps/v1.1.0/evidence/MXX-<name>.md
```

Each evidence file records:

* commit SHA;
* files changed;
* reused sources involved;
* commands run;
* tests passed;
* benchmarks changed;
* known limitations;
* acceptance-gate result;
* next unlocked milestone.

A milestone is not complete merely because code exists.

---

# 10. Atomic roadmap

## M00 — Reconcile the real source of truth

### Goal

Move from the stale local `1.0.6` state to the exact public `v1.0.10` source without losing local work.

### Required actions

1. Inspect:

```shell
git status --short
git remote -v
git branch --show-current
git rev-parse HEAD
git tag --points-at HEAD
```

2. Fetch:

```shell
git fetch origin --tags --prune
```

3. Verify:

```shell
git rev-parse "v1.0.10^{commit}"
git log --oneline --decorate v1.0.6..v1.0.10
```

4. Confirm the `v1.0.10` commit is the published source.

5. If the working tree is dirty:

   * do not overwrite it;
   * do not auto-stash;
   * produce a reconciliation-blocker report;
   * preserve the diff and untracked-file list;
   * stop before branch creation.

6. If clean:

```shell
git switch -c feat/1.1.0-multi-client-runtime v1.0.10
```

7. Verify source versions:

```shell
cargo metadata --no-deps --format-version 1
node -p "require('./package.json').version"
```

8. Run the complete `v1.0.10` baseline.

### Done gate

* Branch ancestry begins at exact `v1.0.10`.
* Cargo and npm versions report `1.0.10`.
* All Rust tests pass.
* npm installer tests pass.
* formatting passes.
* Clippy passes with warnings denied.
* existing benchmarks execute.
* no existing tag moved.
* evidence file committed.

No `1.1.0` implementation may begin before this gate passes.

---

## M01 — Freeze the contract and current-reality audit

### Goal

Compare this roadmap against the real `v1.0.10` modules and produce an implementation map.

### Required outputs

```text
docs/roadmaps/v1.1.0/ROADMAP.md
docs/roadmaps/v1.1.0/CURRENT_REALITY.md
docs/roadmaps/v1.1.0/MODULE_IMPACT.md
docs/roadmaps/v1.1.0/DECISIONS.md
```

### Audit requirements

Map:

* every current CLI command;
* configuration files;
* database schema;
* current profile resolution;
* connect/disconnect ownership;
* request route;
* request parsing;
* streaming path;
* coalescing;
* safe cache;
* reduction;
* provider prompt caching;
* ledger;
* checkpoints;
* Graphify;
* npm/release workflow.

Identify:

* reusable stable modules;
* Anthropic assumptions embedded outside protocol code;
* global singleton state;
* credential-bearing structures;
* multi-client collision risks;
* schema migration requirements;
* obsolete or duplicated surfaces.

### Done gate

* Every planned milestone maps to current files/modules.
* No roadmap task relies on a nonexistent capability.
* No hidden feature scope is introduced.
* Any implementation deviation is recorded as an ADR.
* No runtime behaviour has changed yet.

---

## M02 — Lock reuse and dependency policy

### Goal

Approve every external source before implementation depends on it.

### Required actions

* Pin upstream commits.
* Inspect exact licence files.
* Complete `REUSE_LOCK.md`.
* Complete `REUSE_MANIFEST.toml`.
* Add `cargo-deny`.
* Add `cargo-about`.
* Define approved licence policy.
* Generate a baseline third-party report.
* Preserve existing RTK attribution.
* Identify copied versus adapted versus reference-only material.

### Done gate

* CI fails on unapproved licences.
* Every planned adaptation has a pinned source and licence.
* No whole-gateway dependency is introduced.
* `THIRD_PARTY_NOTICES.md` update strategy is documented.
* No production behaviour changes.

---

## M03 — Configuration schema separation

### Goal

Replace the overloaded `Profile` concept with separate runtime concepts.

### Required model

```text
RuntimeConfig
Integration
Upstream
Policy
StorageConfig
SecretRef
```

### Required principles

* Integration describes the client and connection mode.
* Upstream describes destination protocol, URL and authentication reference.
* Policy describes Toche-controlled optimizations.
* Storage describes local persistence.
* Secrets are referenced rather than newly copied into plaintext configuration.
* Existing `1.0.x` profiles migrate deterministically.
* Unknown fields are preserved where practical.
* The migration is one-way but old data is backed up before activation.

### Secret references in `1.1.0`

Support:

```text
environment variable
existing command/helper reference
existing legacy value during migration
```

Native keyring storage is deferred.

### Done gate

* Fresh configuration parses.
* `1.0.10` configuration migrates.
* Migration is transactional.
* Failed migration leaves the old active configuration usable.
* Migration rerun is idempotent.
* Existing credentials are not printed.
* Existing ledger and CAS remain available.
* Tests cover missing, malformed and partially migrated configuration.

---

## M04 — Setup transaction engine

### Goal

Turn `toche setup` into a rerunnable configuration reconciler.

### Required setup lifecycle

```text
Detect
→ Resolve
→ Ask only unresolved questions
→ Preview
→ Validate
→ Apply transactionally
→ Re-read
→ Verify
→ Commit ownership record
```

### Required behaviour

* First run performs onboarding.
* Later runs review or modify existing configuration.
* Setup may configure multiple supported clients.
* It detects existing endpoints before asking.
* It never silently sends a paid model request.
* It previews exact external-file changes.
* It modifies only Toche-owned fragments.
* Interrupted setup leaves active configuration unchanged.
* Repeated setup with no changes is a no-op.
* Graphify and checkpoints do not appear in standard onboarding.

### Reuse

* `inquire` for prompts.
* Adapted CC Switch structured config algorithms.
* `toml_edit` for TOML preservation.
* Existing Toche atomic-write utilities.

### Done gate

* Setup no-op test passes.
* Setup interruption test passes.
* Preview snapshots pass.
* Apply/remove round-trip property tests pass.
* Unrelated settings and comments survive.
* Setup remains functional in non-interactive validation mode.

---

## M05 — Claude Code integration under the new setup

### Goal

Move existing Claude support onto the new integration/upstream model without regression.

### Required modes

```text
persistent
managed
```

### Persistent mode

Supports:

```shell
toche
claude --dangerously-skip-permissions
```

### Managed mode

Supports:

```shell
toche run claude -- --dangerously-skip-permissions
```

### Requirements

* Existing connection backup/restore semantics remain safe.
* Setup imports the current upstream before inserting Toche.
* Disconnect removes only Toche-owned configuration.
* Unknown Claude settings survive.
* Several Claude processes may connect simultaneously.
* Managed and persistent modes use the same runtime pipeline.

### Done gate

* Existing connect/disconnect tests remain green.
* New structured ownership tests pass.
* Two simultaneous Claude clients work.
* Direct Claude and Toche-routed Claude can coexist where managed mode is used.
* Claude flags are forwarded correctly.
* Ctrl+C does not damage persistent configuration.

---

## M06 — Runtime identity and trust domains

### Goal

Make concurrent clients independently identifiable and safely isolated.

### Required identities

```text
runtime_id
request_id
external_request_id
integration_id
instance_id, nullable
conversation_id, nullable
workspace_id, nullable
upstream_id
trust_domain_id
configuration_snapshot_hash
```

### Trust-domain derivation

The trust domain must incorporate:

* integration identity;
* upstream identity;
* credential-reference identity;
* privacy policy.

Raw credential values must never be placed in:

* logs;
* IDs;
* hashes visible to users;
* database diagnostics;
* receipts.

Use a machine-local secret or equivalent keyed derivation where needed.

### Attribution confidence

Record attribution as:

```text
exact
client-reported
workspace-level
inferred
unknown
```

Do not invent exact process identity in persistent proxy mode when it cannot be observed.

### Done gate

* UUIDv7 IDs are generated and persisted.
* Concurrent clients receive independent request IDs.
* Different trust domains never share cache or flights.
* Configuration snapshot provenance is recorded.
* Missing session identity does not block traffic.
* No credentials appear in snapshots or test output.

---

## M07 — Correct multi-client flight registry

### Goal

Replace URL-only coalescing with trust-safe concurrency.

### Required flight key

```text
protocol version
+ upstream ID
+ trust-domain ID
+ policy hash
+ canonical request fingerprint
```

### Required corrections

* Remove dependence on URL plus body fingerprint alone.
* Add RAII leader cleanup.
* Wake waiters deterministically on completion, failure, panic or cancellation.
* Add real tests where leader and waiters share the same store.
* Prevent one client cancellation from corrupting another client.
* Prevent stale flight entries.

### Streaming decision

`1.1.0` must not retain buffered-after-completion behaviour while claiming transparent streaming coalescing.

Implementation order:

1. Disable coalescing for streaming requests.
2. Prove safe non-streaming coalescing.
3. Implement bounded live fan-out.
4. Enable streaming coalescing only after the fan-out acceptance gate passes.

Live fan-out requires:

* replay of already-emitted events for late waiters;
* bounded memory;
* slow-consumer isolation;
* terminal event propagation;
* no persistence of incomplete streams.

### Done gate

* Different credentials never coalesce.
* Same trust domain plus identical request may coalesce.
* Leader panic leaves no stale flight.
* Waiter cancellation leaves leader intact.
* Slow waiter does not block the upstream stream.
* Streaming coalescing remains disabled unless all fan-out tests pass.

---

## M08 — Protocol-driver boundary

### Goal

Move Anthropic-specific interpretation behind a lossless protocol interface.

### Required structure

```text
src/protocol/
  mod.rs
  anthropic/
```

### Principles

* Raw request bytes remain authoritative.
* Analysis returns only known safe facts.
* Unknown fields survive.
* Fingerprinting is deterministic.
* Protocol-specific transformations cannot execute on another protocol.
* Unsupported content passes through unchanged.
* The existing Anthropic pipeline remains behaviourally equivalent.

### Done gate

* Existing Anthropic tests pass unchanged or with justified fixture updates.
* Unknown-field property tests pass.
* Raw pass-through is byte-equivalent where no transformation occurs.
* Existing reduction, safe replay, prompt cache and ledger behaviour remain valid.
* Benchmark regression is measured and documented.

---

## M09 — OpenAI Responses ingress

### Goal

Add a second real protocol and prove the core is not only an imagined abstraction.

### Required route

```text
/v1/responses
```

### Required `1.1.0` support

* non-streaming forwarding;
* streaming forwarding;
* request identity;
* trust isolation;
* timing;
* status and errors;
* available token usage;
* requested model;
* response model where reported;
* raw unknown-field preservation;
* cancellation;
* configuration provenance.

### Deliberately disabled initially

* Toche output reduction;
* persistent response reuse;
* provider prompt-cache modification;
* cross-protocol coalescing;
* semantic normalization;
* protocol translation.

These may be enabled later only through protocol-specific evidence.

### Reuse

* Official Codex protocol types and fixtures as pinned adaptation/reference material.
* `wiremock-rs` for upstream simulation.
* `insta` for event and error snapshots.
* `proptest` for lossless preservation.

### Done gate

* Official Codex-compatible request fixtures pass.
* Unknown fields survive.
* SSE ordering survives.
* Malformed SSE fails explicitly.
* Missing usage remains unknown.
* Custom model names pass through.
* Anthropic and Responses traffic can run concurrently.

---

## M10 — Codex CLI integration

### Goal

Configure Codex to use Toche’s OpenAI Responses ingress.

### Required setup behaviour

* Detect Codex installation.
* Detect current Codex configuration.
* Preserve comments and unrelated providers.
* Add a Toche-owned provider/config fragment.
* Preserve the original upstream as a Toche upstream.
* Support persistent and managed modes.
* Remove only Toche-owned fragments on disconnect.

### Required workflows

```shell
toche
codex
```

and:

```shell
toche run codex
```

### Done gate

* Claude and Codex run simultaneously.
* Two Codex sessions run simultaneously.
* Codex TOML comments survive setup and disconnect.
* Setup is idempotent.
* Managed arguments are forwarded correctly.
* Codex failure cannot terminate unrelated Claude traffic.
* Trust-domain separation is verified across clients.

---

## M11 — Multi-client evidence and reporting

### Goal

Make runtime behaviour understandable without exposing internal complexity.

### `toche status`

Must report:

* runtime endpoint;
* active integrations;
* active clients where observable;
* active requests;
* active flights;
* coalesced waiters;
* upstream health;
* protocol counts;
* degraded optional systems.

### `toche stats`

Must aggregate by:

* integration;
* protocol;
* upstream;
* workspace;
* requested model;
* reported response model;
* trust domain without exposing secret material;
* session or conversation where known;
* time range.

### Measurement confidence

Every value must be classified:

```text
measured
provider-reported
estimated
configured
unknown
```

### Required measurements

* request bytes;
* response bytes;
* input/output tokens when known;
* time to first chunk;
* total duration;
* upstream calls;
* coalesced calls;
* safe replay;
* reduction;
* provider cache usage;
* Toche processing overhead;
* failures by class.

### Pricing

Use a pinned, licence-approved pricing snapshot.

Display:

```text
equivalent public list-price estimate
```

Do not call it the user’s actual cost unless the upstream reports actual billing.

### Done gate

* Reports distinguish Claude from Codex.
* Missing prices do not become zero.
* Missing usage does not become fabricated usage.
* Requested and response models remain separate.
* Concurrent sessions are represented accurately.
* JSON output has an explicit schema version.

---

## M12 — Failure hardening

### Goal

Validate unpredictable real-world usage.

### Required cases

* runtime started before setup;
* setup run repeatedly;
* upstream changed after setup;
* endpoint offline at startup;
* endpoint fails midstream;
* unknown upstream;
* unknown model;
* no usage metadata;
* several Claude instances;
* several Codex instances;
* Claude and Codex together;
* different credentials to the same URL;
* same credentials across different workspaces;
* non-Git workspace;
* dirty Git workspace;
* huge repository;
* binary or image content;
* already reduced content;
* disk full;
* ledger unavailable;
* malformed configuration;
* interrupted setup;
* killed runtime;
* power-loss recovery simulation;
* downgrade attempt;
* newer schema detected;
* self-signed TLS without unsafe implicit bypass;
* unknown headers;
* upstream rejection of Toche-specific headers.

### Failure principle

Optional optimization failure must not unnecessarily block valid model traffic.

Security, protocol integrity and trust-boundary violations must fail explicitly.

### Done gate

* Failure matrix committed.
* No silent credential crossover.
* No silent configuration reset.
* No hidden fallback.
* No fabricated savings.
* No stale flight after cancellation.
* No completed cache entry from a partial response.

---

## M13 — Migration and compatibility audit

### Goal

Prove existing `1.0.10` users can upgrade without rebuilding their configuration.

### Required upgrade preservation

* profiles;
* endpoint;
* auth references;
* model mappings;
* ledger;
* safe-cache metadata;
* CAS;
* checkpoints;
* Graphify configuration;
* Claude backup state.

Graphify and checkpoints may be demoted from onboarding, but existing data must remain readable.

### Required downgrade behaviour

An older binary encountering a newer schema must not blindly modify it.

### Done gate

* Clean `1.0.10` fixture upgrades.
* Customized `1.0.10` fixture upgrades.
* Partially damaged fixture fails safely.
* Upgrade rerun is a no-op.
* No history or CAS data is silently discarded.
* Migration backup and recovery instructions are verified.

---

## M14 — Documentation and product hierarchy

### Goal

Explain Toche as a multi-client runtime without burying normal users in architecture.

### README primary flow

```shell
npm install -g @nzkbuild/toche
toche setup
toche
```

Then:

```shell
claude
codex
```

### Required documentation

* supported clients;
* supported protocols;
* persistent versus managed mode;
* multiple simultaneous clients;
* upstream neutrality;
* trust isolation;
* data storage;
* measurement confidence;
* limitations;
* upgrade from `1.0.x`;
* clean disconnect/uninstall;
* third-party notices.

### Product hierarchy

Primary:

```text
setup
runtime
doctor
status
stats
connect/disconnect
run
expand
```

Advanced or existing compatibility:

```text
cache
checkpoint
graph
```

No stable command needs to be deleted solely for aesthetic simplification.

### Done gate

A new user can understand:

* what Toche is;
* what it is not;
* why the runtime must be started;
* how multiple clients connect;
* where requests go;
* what Toche stores;
* what reported savings mean.

---

## M15 — Full release audit

### Goal

Prove the complete release contract before changing version metadata.

### Required checks

```shell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
npm run test:npm
cargo bench
cargo deny check
cargo about generate
```

Also run:

* Linux CI;
* Windows CI;
* minimum Rust;
* npm installer tests;
* setup integration matrix;
* concurrent-client tests;
* protocol fixtures;
* migration fixtures;
* release packaging dry run.

### Dogfood sessions

At minimum:

1. One persistent Claude session.
2. Three simultaneous Claude sessions.
3. One managed Claude session.
4. One persistent Codex session.
5. Claude and Codex simultaneously.
6. Two integrations using the same URL with different credentials.
7. Upstream failure during active streaming.
8. Runtime interruption and restart.
9. Upgrade from an actual `1.0.10` configuration copy.

### Done gate

* All required tests pass.
* No compiler warnings.
* No unresolved critical or high findings.
* Benchmarks compared against `v1.0.10`.
* Reuse manifest complete.
* Third-party notices complete.
* Acceptance matrix complete.
* Documentation complete.
* No excluded feature has entered scope.
* Final audit report recommends release.

---

## M16 — Version and release preparation

Only after M15 passes:

1. Set Rust package version to `1.1.0`.
2. Set npm package version to `1.1.0`.
3. Update lockfile.
4. Update changelog.
5. Add `docs/releases/v1.1.0.md`.
6. Update public status metadata.
7. Verify package and binary version agreement.
8. Run the complete release audit again.
9. Commit:

```text
chore(release): prepare v1.1.0
```

10. Use the existing verified release workflow.
11. Do not manually move tags.
12. Publish GitHub release assets before npm.
13. Verify a clean npm installation on Windows.
14. Verify a clean npm installation on at least one Unix platform.
15. Confirm:

```shell
toche --version
toche setup
toche doctor
toche status
```

---

# 11. Final release acceptance contract

Toche `1.1.0` is done only when all statements below are true.

## Runtime

* One Toche runtime accepts multiple simultaneous clients.
* Multiple Claude Code instances work.
* Multiple Codex instances work.
* Claude and Codex work simultaneously.
* One failed client does not terminate unrelated clients.

## Isolation

* Different credential references never share flights or persistent reuse.
* Personal and work configurations remain isolated.
* Raw credentials never appear in evidence.
* Unknown identity produces reduced attribution confidence, not fabricated identity.

## Protocols

* Anthropic Messages retains the complete existing optimization pipeline.
* OpenAI Responses has correct pass-through, streaming, isolation and measurement.
* Unknown fields survive.
* No cross-protocol translation occurs.

## Setup

* First-time setup is guided.
* Setup is rerunnable.
* Setup is idempotent.
* Setup previews changes.
* Setup modifies only owned fragments.
* Setup interruption is safe.
* Claude and Codex configurations preserve unrelated content.

## Concurrency

* Flight keys include trust boundaries.
* Real concurrent-waiter tests exist.
* Leader panic cleans up.
* Waiter cancellation is isolated.
* Streaming coalescing is either fully proven or disabled.

## Evidence

* Status shows active multi-client state.
* Stats distinguish protocols and integrations.
* Time to first chunk is measured.
* Unknown usage remains unknown.
* Prices are labelled honestly.
* Toche overhead is visible.

## Compatibility

* `1.0.10` configuration upgrades.
* Existing ledger and CAS remain usable.
* Existing two-terminal workflow remains supported.
* Managed mode remains optional.
* Existing npm/release machinery remains functional.

## Reuse

* Every dependency passes licence policy.
* Every adapted source is pinned and attributed.
* Every vendored fixture or dataset has provenance.
* No whole gateway was copied into Toche.
* Original code is limited to Toche-specific product logic.

---

# 12. Authority and deviation rule

Claude Code may improve:

* module names;
* file placement;
* private APIs;
* test organization;
* implementation sequence inside an unlocked milestone;
* dependency version selection;
* performance details;
* error-message wording.

Claude Code may not independently change:

* release theme;
* required protocols;
* supported clients;
* persistent two-terminal workflow;
* trust-domain requirement;
* lossless protocol boundary;
* reuse-first policy;
* atomic milestone gates;
* exclusions;
* migration requirement;
* release acceptance contract.

A necessary deviation requires an entry in:

```text
docs/roadmaps/v1.1.0/DECISIONS.md
```

Each deviation entry must contain:

* discovered local evidence;
* original assumption;
* proposed correction;
* alternatives considered;
* safety impact;
* compatibility impact;
* scope impact;
* recommendation.

The roadmap remains frozen until the owner accepts that decision.

---

# 13. First instruction to execute

Begin only with M00.

Do not start implementation.

Inspect the workspace, fetch the exact public tags, verify the `v1.0.10` source, preserve any local changes, create the `1.1.0` feature branch from the exact public release, run the complete baseline, and produce the M00 evidence report.

Stop after the M00 gate and report:

* original local state;
* whether the tree was clean;
* fetched remote state;
* verified `v1.0.10` commit;
* branch created;
* baseline command results;
* test count;
* warnings;
* benchmark availability;
* blockers;
* exact next unlocked milestone.
