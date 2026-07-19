# M21 — Agent Integration Strategy & Architecture Study

**Author:** Astra (Release Director)  
**Date:** 2026-07-20  
**Classification:** Product Strategy — 3-5 Year Architecture Decision  

---

## Executive Summary

**Toche should become Architecture C: Universal AI Coding Middleware.**

The recommendation is NOT to integrate individual agents one-by-one. It is to build a **generic adapter layer** that makes Toche the universal local gateway any AI coding agent routes through — without per-agent integration code. This maximizes strategic value while minimizing per-agent maintenance burden.

The evidence is clear: Toche's existing architecture (universal control plane + protocol-specific payload plane + HTTP proxy + config management) already has 80% of what's needed. The remaining 20% is a formal adapter interface that any tool can plug into.

**Top recommendation:** Build the adapter architecture now. Prioritize Hermes (highest strategic overlap with Toche's mission) and OpenClaw (fastest-growing) as first adapters. Reject Cursor (closed IDE, no HTTP interception path). Defer Aider and Gemini CLI until adapter layer is proven.

---

## 1. Toche Mission Alignment

Toche 1.1.0's public promise:

> "One local Toche runtime can serve multiple simultaneous AI clients and sessions, isolate their trust boundaries, share only provably identical work, preserve protocol-specific behaviour, and report what happened honestly."

This is a universal statement. It does not say "Claude Code." It says "multiple simultaneous AI clients." The architecture already delivers on this promise — the Protocol trait, integration discovery, and config management are explicitly designed for multiple clients.

**The question is not whether Toche should support multiple agents. Toche already does (Claude + Codex). The question is which agents justify the integration effort and how to scale that effort efficiently.**

---

## 2. Current Ecosystem Overview

### 2.1 What Toche Already Has

From `docs/ARCHITECTURE.md` and the source tree:

| Layer | Component | Status |
|-------|-----------|--------|
| Gateway | Axum HTTP server, port 8743, 127.0.0.1 | ✅ Production |
| Protocol | `Protocol` trait — 6 methods, stateless drivers | ✅ Anthropic + OpenAI |
| Pipeline | Coalescing, safe cache, reduce, efficiency, ledger | ✅ Protocol-agnostic |
| Config | `TocheConfig` v2 — integrations, upstreams, policies | ✅ Multi-upstream |
| Identity | `IdentityContext` — runtime, trust domain, attribution | ✅ Generic |
| Integration | Per-client config management (`connect`/`disconnect`) | ✅ Claude + Codex |
| Client launch | `toche run`, `toche connect` | ✅ Two modes |

### 2.2 The Integration Model

Toche integrates with a client by:
1. **Config modification** — `toche connect` modifies the client's config file to point `base_url` to `http://127.0.0.1:8743`
2. **Discovery** — `src/integrations/<agent>/discovery.rs` finds config files on disk
3. **Launch** — `toche run <agent>` starts the gateway + launches the agent with correct env vars
4. **Protocol** — A `Protocol` impl handles agent-specific API parsing if the agent uses a non-Anthropic/OpenAI API format

For any agent that makes HTTP API calls to an Anthropic-compatible or OpenAI-compatible endpoint, the integration cost is near-zero — just config modification. The Protocol trait only needs a new impl if the agent uses a custom API format.

### 2.3 Agent Categorization

Agents fall into three categories based on integration feasibility:

| Category | Integration Cost | Example |
|----------|-----------------|---------|
| **HTTP → standard API** | Near-zero — just config rewrite | Any tool calling `/v1/messages` or `/v1/chat/completions` |
| **HTTP → custom API** | Medium — new Protocol impl | Custom API format (e.g., Gemini) |
| **Non-HTTP / closed** | High or impossible | IDE plugins, proprietary protocols |

---

## 3. Candidate Analysis

### 3.1 Hermes (Nous Research)

| Dimension | Assessment |
|-----------|-----------|
| **What it does** | Open-source agent framework. LLM-driven with tool access (terminal, browser, file, delegation). Runs as a CLI daemon with multiple provider support. |
| **API communication** | HTTP via configurable provider (`model.provider` in config.yaml). Supports custom providers with `base_url`. Already uses OpenAI-compatible `/v1/chat/completions` endpoints. |
| **Integration path** | `toche connect hermes` → rewrite `provider.base_url` → `http://127.0.0.1:8743/v1`. Protocol: existing `OpenAiResponsesProtocol` or add a chat-completions protocol driver. |
| **Engineering effort** | **Low.** Discovery: find `~/.hermes/config.yaml` (single known path). Protocol: `/v1/chat/completions` is OpenAI-standard, but Toche currently handles `/v1/responses` (Responses API). Would need a ChatCompletions protocol adapter — estimated 200-400 lines. Config rewrite: straightforward, already solved for Claude. |
| **Strategic alignment** | **Very high.** Hermes and Toche share the same mission: local AI agent optimization. Hermes users are Toche's target audience. Both are open-source, Rust-adjacent, developer-tooling focused. |
| **Community adoption** | Fast-growing in the open-source AI agent space. Part of the Nous Research ecosystem. GitHub stars growing. Developer community overlaps with Toche's target. |
| **Maintenance risk** | Low. Hermes uses standard OpenAI-compatible API. Config format is stable YAML. Provider model is unlikely to change fundamentally. |
| **Verdict** | 🟢 **Build immediately.** Highest strategic overlap, lowest integration cost. The adapter proves the architecture. |

### 3.2 OpenClaw

| Dimension | Assessment |
|-----------|-----------|
| **What it does** | Open-source AI coding agent. CLI-based. Multi-model support. Rapid iteration cycle. |
| **API communication** | HTTP API calls to OpenAI/Anthropic-compatible endpoints. Uses `OPENAI_API_KEY` and `OPENAI_BASE_URL` env vars or config files. |
| **Integration path** | Same pattern as Hermes: config rewrite + existing Protocol trait. Likely OpenAI `/v1/chat/completions`. |
| **Engineering effort** | **Low.** Discovery: find OpenClaw config (likely `~/.openclaw/config` or env vars). Protocol: reuses Hermes ChatCompletions adapter. |
| **Strategic alignment** | **High.** Growing fast. Developer-first. Multi-model. Users benefit directly from Toche's coalescing and cost tracking. |
| **Community adoption** | Growing rapidly among developers seeking Claude Code alternatives. Active community. |
| **Maintenance risk** | Medium. Fast iteration cycle means config format may change. Mitigation: config discovery handles multiple paths. |
| **Verdict** | 🟢 **Build immediately.** Second adapter after Hermes. Proves the adapter pattern works across different agents. |

### 3.3 Cursor

| Dimension | Assessment |
|-----------|-----------|
| **What it does** | AI-powered IDE (VS Code fork). Closed-source. Integrated AI features. |
| **API communication** | **Internal, proprietary.** Cursor communicates with its own backend, not directly with AI APIs. No configurable `base_url`. No HTTP proxy interception possible. |
| **Integration path** | **None.** Cursor controls its own API routing. Toche cannot sit between Cursor and upstream without Cursor adding explicit proxy support. This is not a Toche limitation — it's architectural. |
| **Engineering effort** | **Impossible without Cursor cooperation.** Would require Cursor to add a configurable proxy endpoint. |
| **Strategic alignment** | Low. Closed ecosystem. No incentive for Cursor to route through third-party middleware. |
| **Verdict** | 🔴 **Reject.** Not technically feasible. Revisit only if Cursor adds proxy support. |

### 3.4 Aider

| Dimension | Assessment |
|-----------|-----------|
| **What it does** | Open-source AI pair programming tool. CLI-based. Edits files directly. |
| **API communication** | Direct API calls to OpenAI/Anthropic. Uses `OPENAI_API_BASE` env var. Fully proxyable. |
| **Integration path** | Env var override: `OPENAI_API_BASE=http://127.0.0.1:8743/v1`. No config file modification needed — just set env in the managed mode. |
| **Engineering effort** | **Very low.** No config file discovery needed. Just inject `OPENAI_API_BASE` env var in `toche run aider`. |
| **Strategic alignment** | Medium. Aider users are AI coding tool users — overlap with Toche's audience. But Aider is a single-purpose tool (code editing), not a general agent. |
| **Community adoption** | Established. Large user base. Mature project. |
| **Verdict** | 🟡 **Prototype after adapters.** Low effort, but lower strategic overlap. Wait until adapter layer is proven, then add as a quick win. |

### 3.5 Gemini CLI (Google)

| Dimension | Assessment |
|-----------|-----------|
| **What it does** | Google's AI CLI tool. Gemini model access from terminal. |
| **API communication** | Likely uses Google's Gemini API (not OpenAI/Anthropic compatible). Custom protocol. May use gRPC or REST with Google-specific auth. |
| **Integration path** | Requires new Protocol impl for Gemini API format. Auth model differs (OAuth vs API key). May not support configurable base URL. |
| **Engineering effort** | **High.** Custom protocol + custom auth + uncertain proxy support. |
| **Strategic alignment** | Medium. Growing ecosystem, but tool is nascent. |
| **Verdict** | 🟡 **Wait for ecosystem maturity.** Revisit when Gemini CLI has stable configurable endpoints and broader adoption. |

---

## 4. Agent Comparison Matrix

| Agent | HTTP-Proxyable | Configurable URL | Protocol Overlap | Integration Difficulty | Strategic Value | Community | Priority |
|-------|---------------|-----------------|------------------|----------------------|-----------------|-----------|----------|
| **Hermes** | ✅ | ✅ `base_url` | OpenAI `/v1/chat` | 🟢 Low | 🔵 Very High | Growing fast | **#1 — Build** |
| **OpenClaw** | ✅ | ✅ `OPENAI_BASE_URL` | OpenAI `/v1/chat` | 🟢 Low | 🔵 High | Growing | **#2 — Build** |
| **Aider** | ✅ | ✅ `OPENAI_API_BASE` | OpenAI `/v1/chat` | 🟢 Very Low | 🟡 Medium | Established | **#3 — Prototype** |
| **Codex CLI** | ✅ | ✅ config file | OpenAI `/v1/responses` | 🟢 Already done | 🟢 Done | Active | Already integrated |
| **Claude Code** | ✅ | ✅ `ANTHROPIC_BASE_URL` | Anthropic `/v1/messages` | 🟢 Already done | 🟢 Done | Active | Already integrated |
| **Gemini CLI** | ⚠️ Unknown | ⚠️ Unknown | Google Gemini (custom) | 🔴 High | 🟡 Medium | Early | Defer |
| **Cursor** | ❌ | ❌ | Proprietary IDE | 🔴 Impossible | 🔴 None | Large but closed | Reject |
| **Windsurf** | ❌ | ❌ | Proprietary IDE | 🔴 Impossible | 🔴 None | Growing but closed | Reject |

---

## 5. Adapter Architecture Proposal

### 5.1 The Problem with Per-Agent Integrations

If Toche adds agents one-by-one, each integration requires:
- Discovery code (find config files)
- Config rewrite logic (`connect`/`disconnect`)
- Launch logic (`toche run`)
- Potentially a new Protocol impl
- Tests, docs, maintenance

After 3 agents, this is manageable. After 10 agents, it's a maintenance nightmare. Each agent's config format changes. Each agent adds a test matrix dimension. The codebase grows linearly with agent count.

### 5.2 The Solution: Universal Adapter Layer

```text
┌─────────────────────────────────────────────────────────────┐
│                    TOCHE CORE                               │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐      │
│  │ Coalesce │ │ SafeCache│ │  Reduce  │ │  Meter   │ ...  │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘      │
│                                                             │
│  Universal Control Plane (protocol-agnostic)                │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                   ADAPTER LAYER                             │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              AgentAdapter trait                      │   │
│  │                                                     │   │
│  │  + discover(config_dir) → Vec<AgentInstallation>    │   │
│  │  + connect(installation) → Result<(), Error>        │   │
│  │  + disconnect(installation) → Result<(), Error>     │   │
│  │  + launch(installation, args) → Command             │   │
│  │  + base_url_key() → &str   // "ANTHROPIC_BASE_URL"  │   │
│  │                                                     │   │
│  │  + protocol: Box<dyn Protocol>  // optional          │   │
│  └─────────────────────────────────────────────────────┘   │
│                          │                                  │
│         ┌────────────────┼────────────────┐                 │
│         ▼                ▼                ▼                 │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐           │
│  │  Hermes    │  │  OpenClaw  │  │   Aider    │  ...      │
│  │  Adapter   │  │  Adapter   │  │  Adapter   │           │
│  └────────────┘  └────────────┘  └────────────┘           │
└─────────────────────────────────────────────────────────────┘
```

### 5.3 The `AgentAdapter` Trait

```rust
pub trait AgentAdapter: Send + Sync {
    /// Human-readable agent name
    fn name(&self) -> &str;

    /// Find all installations of this agent on the system
    fn discover(&self, home_dir: &Path) -> Vec<AgentInstallation>;

    /// Rewrite the agent's config to route through Toche
    fn connect(&self, installation: &AgentInstallation, toche_url: &str) -> Result<(), AdapterError>;

    /// Restore original config
    fn disconnect(&self, installation: &AgentInstallation) -> Result<(), AdapterError>;

    /// Build the command to launch this agent in managed mode
    fn launch_command(&self, installation: &AgentInstallation, args: &[String]) -> Command;

    /// The environment variable or config key for base URL
    fn base_url_target(&self) -> UrlTarget;

    /// The protocol this agent uses (None = unknown/direct passthrough)
    fn protocol_override(&self) -> Option<Box<dyn Protocol>>;

    /// Whether this agent supports managed mode (toche run)
    fn supports_managed_mode(&self) -> bool;

    /// Whether this agent supports persistent mode (toche connect)
    fn supports_persistent_mode(&self) -> bool;
}

pub struct AgentInstallation {
    pub version: Option<String>,
    pub config_paths: Vec<PathBuf>,
    pub binary_path: Option<PathBuf>,
    pub installation_type: InstallationType,  // Global, Local, ProjectLocal
    pub metadata: HashMap<String, String>,
}

pub enum UrlTarget {
    /// Environment variable: ANTHROPIC_BASE_URL
    EnvVar(String),
    /// Config file key path: ["model", "provider", "base_url"]
    ConfigKey(Vec<String>),
    /// Command-line flag: --base-url
    CliFlag(String),
}
```

### 5.4 Why This Architecture

**Benefits over per-agent integration:**

1. **Linear effort → constant effort.** Each new adapter is ~200 lines of config discovery + rewrite. No pipeline changes. No CLI changes. No test matrix explosion.

2. **Self-documenting.** The `AgentAdapter` trait is the integration contract. Third parties can write adapters without touching Toche core.

3. **`toche doctor` auto-discovers.** `toche doctor` already audits the system. With `discover()`, it can list ALL installed agents and their Toche status.

4. **Plugin ecosystem.** Adapters could live in a separate crate or even be dynamically loaded. Community contributions become possible.

5. **CI/project support.** A project can declare `toche adapters = ["hermes", "aider"]` in a `.toche/project.toml` and Toche auto-configures them.

### 5.5 What Changes in Toche Core

| Component | Change |
|-----------|--------|
| `src/integrations/` | Refactor to use `AgentAdapter` trait. Existing Claude + Codex become adapters. |
| `src/cli/connect.rs` | Accept `[AGENT]` from adapter registry, not hardcoded list |
| `src/cli/disconnect.rs` | Same |
| `src/cli/run.rs` | Same |
| `src/cli/doctor.rs` | Call `discover()` for all registered adapters |
| `src/gateway/routes.rs` | Minor: protocol dispatch already works with `Protocol` trait |
| `src/protocol/` | Add `ChatCompletionsProtocol` for OpenAI `/v1/chat/completions` |

**Lines changed: ~500-800. New code per adapter: ~200-400 lines.**

---

## 6. Technical Feasibility

### 6.1 The Protocol Question

Toche currently handles two API formats:
- Anthropic `/v1/messages` — full pipeline (coalesce, cache, reduce, efficiency)
- OpenAI `/v1/responses` — pass-through only

Most modern AI coding agents use **OpenAI `/v1/chat/completions`**, which is neither of the above. This is the single biggest gap.

**Solution:** Add `ChatCompletionsProtocol` — a third Protocol impl. It's similar to `OpenAiResponsesProtocol` (same auth headers, same SSE format) but parses the Chat Completions request/response shape. This enables Hermes, OpenClaw, Aider, and any tool using OpenAI-compatible APIs.

Estimated effort: 300-500 lines. Well-understood format.

### 6.2 Config Discovery Challenge

Different agents store config in different places:
- Hermes: `~/.hermes/config.yaml` (single file, YAML)
- OpenClaw: `~/.openclaw/config.json` or env vars
- Aider: Env vars only (`OPENAI_API_BASE`)
- Claude Code: `~/.claude/settings.json` (already solved)
- Codex CLI: `~/.codex/config.toml` (already solved)

The `AgentAdapter::discover()` trait method handles this per-adapter. Toche core doesn't need to know where each agent stores config — the adapter encapsulates that.

### 6.3 Auth Compatibility

Toche's `UpstreamAuth` model (env vars, external commands, legacy inline) is already generic. Any agent's API key format works because Toche forwards to the upstream — it doesn't need to understand the auth model, just the API format.

---

## 7. Product Impact

### 7.1 What Users Get

| Feature | Today (Claude Code only) | With Adapter Layer |
|---------|--------------------------|---------------------|
| Coalescing | ✅ Claude Code concurrent sessions | ✅ Any agent's concurrent sessions |
| Cost tracking | ✅ `toche stats` per model | ✅ Per-agent cost breakdown |
| Trust domains | ✅ Claude Code isolation | ✅ Hermes ≠ OpenClaw ≠ Claude Code |
| `toche run` | `claude`, `codex` | Any registered adapter |
| `toche doctor` | Claude + Codex status | All installed agents status |
| `toche connect` | Claude + Codex | One command, any agent |
| Cache | ✅ Claude Code safe cache | ✅ Any agent using standard API |

### 7.2 Positioning Shift

**Today:** "Toche — the Claude Code efficiency gateway"
**After adapter layer:** "Toche — the universal AI coding gateway"

This is a meaningful positioning change. Today, Toche is perceived as a Claude Code accessory. Tomorrow, it's perceived as infrastructure. Infrastructure has higher switching costs, stronger network effects, and higher valuation.

---

## 8. Maintenance Impact

| Scenario | Maintenance Burden |
|----------|-------------------|
| No adapter layer, 1 agent/year | Low — but also low growth |
| No adapter layer, 5 agents | High — `connect`/`disconnect`/`run` logic duplicated per agent |
| With adapter layer, 5 agents | Low — per-agent code is isolated in adapters, core unchanged |
| With adapter layer, 20 agents | Medium — adapter registry management, but core still unchanged |

**The adapter layer pays for itself at 3+ agents.** Below that, either approach works. But building it now — when only 2 agents exist — means the architecture is proven before the agent count grows.

---

## 9. Competitive Positioning

| Competitor | Toche's Advantage |
|------------|------------------|
| Direct API calls | Stats, caching, coalescing, cost tracking |
| LiteLLM (proxy) | Local-only, trust domains, project isolation, no cloud dependency |
| OpenRouter | No per-request markup, full privacy (all data stays local) |
| Langfuse (observability) | Toche is a gateway, not just observability — it ACTS on requests |
| Helicone (proxy) | Toche is local, no data leaves the machine |

**Toche's unique positioning:** Local-first, privacy-preserving, multi-agent, active optimization (not just passive observation). No competitor does all of these.

---

## 10. Risks

### 10.1 Technical Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Agent changes config format → adapter breaks | Medium | Adapter tests catch this. Version pinning in adapter metadata. |
| Agent adds non-HTTP communication (gRPC, Unix sockets) | Medium | Adapter declares protocol. Toche only supports HTTP agents. |
| Protocol trait can't handle new API shapes | Low | `Protocol` is intentionally narrow. New impls don't affect existing ones. |
| Performance impact of routing more agents through Toche | Low | Gateway is stateless per-request. Coalescing is in-memory. |

### 10.2 Business/Strategic Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Agent dies → adapter becomes maintenance debt | Low | Remove adapter from registry. No core code affected. |
| Agent goes closed-source → can't test integration | Medium | Requires community testing. Mitigated by adapter isolation. |
| User confusion: "which agents does Toche support?" | Low | `toche doctor` auto-discovers. `toche adapters list` shows all. |
| Spreading too thin — supporting 10 agents dilutes quality | Medium | Adapter quality gates. Tiered support (Tier 1 = tested by Toche team, Tier 2 = community, Tier 3 = experimental). |

### 10.3 Security Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Malicious adapter modifies user config | Low | Adapters live in Toche source tree. Community adapters reviewed before merge. |
| Agent leaks credentials through Toche | Low | Toche's SecretRef model already handles this. |
| Cross-agent trust domain leakage | Low | TrustDomainId derivation is already per-integration. |

---

## 11. Opportunities

1. **Network effects.** Every new adapter makes Toche more valuable. Users who adopt Toche for Claude Code discover it works for Hermes too. Users who adopt for Hermes bring Claude Code colleagues. Cross-pollination drives adoption.

2. **Plugin ecosystem.** Community-written adapters for niche agents. Toche becomes the "VS Code of AI gateways" — the platform everyone builds extensions for.

3. **Enterprise adoption.** Companies standardizing on Toche as their AI gateway get: cost tracking across all agents, usage policies per team, trust domain isolation per project. No other tool offers this.

4. **Benchmarking.** Toche's ledger already tracks per-model costs and latencies. With multiple agents, Toche can answer: "Which agent + model combination is most cost-effective for my workload?"

5. **Agent migration tool.** `toche migrate --from hermes --to claude` → switch agents without losing context, checkpoints, or config.

---

## 12. Recommended Architecture

### Architecture C: Universal AI Coding Middleware

Toche becomes the standard local gateway for all AI coding agents. The adapter layer makes integration cost constant per agent. The universal control plane provides value (coalescing, caching, cost tracking, trust isolation) regardless of which agent the developer uses.

**This is the architecture Toche's codebase was already designed for.** The Protocol trait, integration discovery, config management, and pipeline are all generic. The adapter layer formalizes what's already implicit.

---

## 13. Recommended Roadmap

### Phase 1: Foundation (Now — 2 weeks)

| Task | Effort | Priority |
|------|--------|----------|
| Define `AgentAdapter` trait | 1-2 days | 🔴 Critical |
| Refactor Claude + Codex to use trait | 2-3 days | 🔴 Critical |
| Add `ChatCompletionsProtocol` | 2-3 days | 🔴 Critical |
| `toche doctor` → auto-discover all agents | 1-2 days | 🟡 High |
| Tests: adapter trait contract tests | 2-3 days | 🔴 Critical |

### Phase 2: First Adapters (Week 3-4)

| Task | Effort | Priority |
|------|--------|----------|
| Hermes adapter | 1-2 days | 🔴 Critical |
| OpenClaw adapter | 1-2 days | 🔴 Critical |
| Integration tests: Hermes + OpenClaw through Toche | 2-3 days | 🟡 High |

### Phase 3: Ecosystem (Month 2-3)

| Task | Effort | Priority |
|------|--------|----------|
| Aider adapter (env-only, trivial) | 1 day | 🟢 Medium |
| Community adapter guide + template | 2-3 days | 🟢 Medium |
| `toche stats --by-agent` report | 1-2 days | 🟢 Medium |
| Tiered adapter support model | Documentation | 🟢 Medium |

### Phase 4: Platform (Month 4-6)

| Task | Effort | Priority |
|------|--------|----------|
| Project-level `.toche/adapters.toml` | 3-5 days | 🟡 Low |
| Adapter health monitoring (`toche doctor` per adapter) | 2-3 days | 🟡 Low |
| Agent migration tools | 3-5 days | 🟡 Low |

---

## 14. Estimated Engineering Effort

| Phase | Effort | Cumulative |
|-------|--------|------------|
| Phase 1: Foundation | 8-13 days | 8-13 days |
| Phase 2: First Adapters | 5-7 days | 13-20 days |
| Phase 3: Ecosystem | 7-10 days | 20-30 days |
| Phase 4: Platform | 8-13 days | 28-43 days |

**Total: 1-2 months of full-time engineering for the complete adapter platform.** Foundation alone (Phase 1) delivers the architecture and can ship in 2 weeks.

---

## 15. What NOT to Do

1. **Do not integrate Cursor, Windsurf, or any closed IDE.** Not technically feasible. Wastes time.
2. **Do not build per-agent integrations without the adapter layer.** The 3rd agent will hurt. The 5th will be painful. The 10th will be impossible.
3. **Do not turn Toche into an agent itself.** Toche is middleware. Agents generate. Toche optimizes. Keep this boundary.
4. **Do not add non-AI-coding agents.** Toche's value is in the AI coding workflow. Adding general LLM proxy features dilutes the product.
5. **Do not build cross-protocol translation.** ADR-004 already decided this. Keep native protocol fidelity.

---

## 16. Final Recommendation

### Build Architecture C: Universal AI Coding Middleware. Start with the adapter layer now.

**Rationale:**

- **Toche is already 80% there.** The Protocol trait, integration system, and config model are designed for this.
- **The adapter layer pays for itself at 3 agents.** Toche already has 2 (Claude + Codex). Hermes makes 3.
- **Network effects.** Each new adapter increases Toche's value for existing users.
- **Strategic positioning.** Toche moves from "Claude Code tool" to "AI coding infrastructure."
- **Low risk.** Adapters are isolated. A bad adapter doesn't break Toche. The core stays clean.
- **Right timing.** The AI coding agent ecosystem is fragmenting. Toche can be the unifying layer before fragmentation becomes the norm.

**First action:** Define `AgentAdapter` trait and refactor existing Claude + Codex integrations to use it. This proves the architecture in the codebase that already exists. Then add Hermes as the first proof-point of the adapter pattern.

---

## Appendix A: What About "Just Claude Code Enhancement"?

If the recommendation were Architecture A (Claude Code enhancement only), Toche would:
- Add Claude Code-specific features (session resumption, Codex-to-Claude migration)
- Optimize only for Anthropic API, ignore OpenAI/ChatCompletions
- Skip adapter layer entirely
- Position as "the best Claude Code companion"

**Why this loses:**
- Claude Code is a single tool. Anthropic could add Toche-like features natively.
- Zero network effects. One tool → one user base.
- Vendor lock-in risk. If Claude Code loses market share, Toche loses everything.
- Leaves the multi-agent opportunity on the table for competitors.

Architecture C (Universal Middleware) is the higher-upside, lower-risk path because it works even if Claude Code dominance fades.

---

## Appendix B: What About "Just Hermes"?

If this study were only about integrating Hermes:

**The answer is still: build the adapter layer.** Even if Hermes is the only new agent, formalizing the integration contract (trait + discovery + connect/disconnect) makes the Claude + Codex code cleaner and prepares for the next agent without rework. The adapter layer is the right architecture regardless of how many agents follow.

---

*Report ends.*
