# M22 — Mission Alignment Audit

**Author:** Astra (Release Director)  
**Date:** 2026-07-20  
**Classification:** Pre-1.2.0 Strategic Review  
**References:** M19 Acceptance Testing, M20 Product Evaluation, M21 Architecture Strategy, ARCHITECTURE.md

---

## Executive Summary

**Toche 1.1.0 is NOT YET the intelligence layer for AI coding agents.**

It is a well-engineered Claude Code companion gateway with excellent architecture that happens to support a second client (Codex CLI). The mission statement is mostly aspirational. Five key gaps exist between what Toche claims to be and what it actually delivers today.

The good news: the architecture is correct. The pipeline is genuinely agent-agnostic. The Protocol trait, trust domains, and ledger recording are designed for a multi-agent future. The gap is in user-visible behavior — not in code correctness.

The mission statement is honest about its aspirations but misleading about its current state. Fix: revise the mission to acknowledge where Toche is today, then close the gaps in 1.2.0.

---

## Part 1 — Mission Validation

> *"Toche is the intelligence layer for AI coding agents."*

Let's evaluate every phrase.

### "Intelligence layer"

| Truthfulness | Evidence |
|-------------|----------|
| **Partially true** | M20 Product Evaluation: "The core value proposition — cache coalescing to save API costs — is invisible to the user. [Checkpoint] breaks on any machine that has previously run Toche because of a schema version mismatch." |
| What's intelligent | Request coalescing, safe cache matching, trust domain derivation, checkpoint stale detection |
| What's not | All decisions are rule-based (SHA-256 matching, TOML policies). No learned behavior, no adaptive optimization, no predictive caching. The user never feels the intelligence — they run `toche stats` post-hoc. |
| Verdict | **Aspirational.** Toche optimizes intelligently but the user experience is a utility, not a layer. An "intelligence layer" would surface insights, not hide them. |

### "AI coding agents" (plural)

| Truthfulness | Evidence |
|-------------|----------|
| **Partially true** | Toche supports Claude Code (primary, full integration) and Codex CLI (pass-through only, no reduction/caching/efficiency). Two agents = technically plural. |
| What M21 found | 8 viable agent candidates exist. Toche integrates with 2. Hermes (217K★) is architecturally the best match — but not integrated yet. |
| What a user sees | `toche run claude` works. `toche run codex` works but is bare-bones. Every other agent: use raw `claude -p` calls. |
| Verdict | **Misleading.** "AI coding agents" implies a platform. Toche is a Claude Code tool that happens to also route Codex. The architecture supports more agents — the product does not (yet). |

### "Sits between AI coding assistants and AI providers"

| Truthfulness | Evidence |
|-------------|----------|
| **True** | ARCHITECTURE.md confirms: Axum HTTP server on `127.0.0.1:8743`, routes `/v1/messages` and `/v1/responses`, proxies to configured upstreams. Config rewrite in `toche connect` points Claude Code's `ANTHROPIC_BASE_URL` to Toche. |
| Evidence | `toche doctor` confirms: `base_url: http://127.0.0.1:8743`, `env.ANTHROPIC_BASE_URL: https://freeai.jembatanai.com/`, `points to Toche: true`. |
| Verdict | **True.** This is the one phrase that holds up under scrutiny. |

### "Persistent memory"

| Truthfulness | Evidence |
|-------------|----------|
| **Partially true** | SQLite ledger (`~/.toche/ledger.db`) persists every request. Safe cache persists cross-session responses. Checkpoint DB persists session state. CAS stores content by hash. |
| What works | The ledger never loses data. `toche stats` shows complete history including pre-1.1.0 requests. |
| What doesn't | Checkpoint breaks on schema mismatch (`version 11 > 9`). Safe cache is invisible — user never knows it served a cached response. No "memory recall" feature — can't ask Toche "what did I work on yesterday?" |
| Verdict | **Partially true.** The storage exists. The retrieval UX doesn't. A database is not memory until the user can access it. |

### "Intelligent caching"

| Truthfulness | Evidence |
|-------------|----------|
| **Partially true** | Coalescing works (same in-flight request → one upstream call). Safe cache works (same request later → replay cached response). M20 verified: 1 local cache hit out of 8 requests. |
| What's intelligent | Flight key derivation (`url|fingerprint|trust_domain|policy_hash`) correctly isolates by trust domain. Coalescing prevents duplicate upstream calls. |
| What's not | The cache hit is invisible. No "saved $X this session" indicator. No cache warmth dashboard. No predictive pre-warming. No adaptive TTL based on content freshness. |
| Verdict | **Partially true — technically correct, experience-invisible.** The caching engine works. The user would never know unless they ran `toche stats` and noticed `Local cache hits: 1`. |

### "Cost optimization"

| Truthfulness | Evidence |
|-------------|----------|
| **Partially true** | `toche stats` shows exact costs: `Known cost: $0.007611`. Per-model breakdown. Recent requests timeline with latitude. |
| What's measurable | Toche records every request's token usage, computes cost from embedded pricing, and stores it. This is real cost visibility. |
| What's not | No cost budget (can't set `$10/month` cap). No cost prediction before requests. No "you're spending 30% more this week" alert. No cost comparison across models. Coalescing saves money but the user can't prove it. |
| Verdict | **Partially true — excellent visibility, no active optimization.** Toche shows costs. It doesn't optimize them beyond coalescing. |

### "Checkpointing"

| Truthfulness | Evidence |
|-------------|----------|
| **Partially true** | M20 verified: `toche checkpoint save` creates entries with task/completed/next/git HEAD. `toche checkpoint show` displays stale detection (dual HEAD + workspace check). `toche checkpoint list` shows all project checkpoints. |
| What works | When it works, checkpoint is genuinely clever. Stale detection catches both git changes and workspace changes. |
| What doesn't | Breaks on schema mismatch. No integration with `toche run` (can't `toche run --continue`). No auto-save on session end. Manual only. |
| Verdict | **Not production-ready.** A feature that breaks on 100% of machines that previously ran Toche is not ready. The concept is excellent. The reliability is not. |

### "Observability"

| Truthfulness | Evidence |
|-------------|----------|
| **Partially true** | `toche stats` and `toche doctor` are M20's "premium feel" features. Structured output, cost breakdown, request timeline, doctor audit. |
| What's excellent | Stats output quality, doctor diagnostics, CLI help completeness. |
| What's missing | No real-time observability (`toche stats --watch`). No per-session summary. No notifications (cost threshold alerts). No visualization. No export (CSV/JSON for analysis). |
| Verdict | **Partially true — great static reporting, no active observability.** |

### "Workflow enhancements"

| Truthfulness | Evidence |
|-------------|----------|
| **Partially true** | M20 identified: `toche run` is faster than manually typing `claude -p` with flags. Checkpoint stale detection warns before you resume stale context. `toche stats` makes costs tangible. |
| What a first-time user notices | M19 Acceptance: "toche setup requires interactive terminal" — first impression is friction, not enhancement. |
| What a daily user notices | M20: "I'd have `toche run` muscle memory. I'd check stats at breaks. I'd use checkpoints on feature branches." |
| Verdict | **Partially true — enhancements exist but are gated behind setup friction.** |

### "Without requiring changes to the agents themselves"

| Truthfulness | Evidence |
|-------------|----------|
| **True for Claude Code** | `toche connect` modifies `~/.claude/settings.json` to point `ANTHROPIC_BASE_URL` to Toche. Claude Code itself is unchanged — it just sees a different URL. |
| **True for Codex CLI** | Same pattern: config rewrite via `toche connect codex`. |
| **False for agents without connect support** | Hermes, Aider, Cline — none have `toche connect` support. They require manual config changes or env var injection. The architecture supports it, but the product doesn't (yet). |
| Verdict | **True for the two supported agents. Aspirational for the rest.** |

---

## Part 2 — Capability Maturity Matrix

Each capability scored 0–5. Evidence from M19/M20/M21/ARCHITECTURE.

| Capability | Score | Evidence |
|-----------|-------|----------|
| **Intelligence** | 2/5 | Rule-based optimization only. No learning, prediction, or adaptation. Correct but not smart. |
| **Memory** | 3/5 | SQLite persistence works. Checkpoint breaks on schema mismatch. No retrieval UX beyond `toche stats`. |
| **Context** | 4/5 | `IdentityContext` is well-designed. Trust domains, runtime ID, request ID, attribution — all correct. |
| **Cache** | 3/5 | Engine works (coalescing, safe cache, CAS). Invisible to user. No cache warmth indicator. |
| **Cost Optimization** | 3/5 | Excellent visibility (`toche stats`). No active optimization (budgets, predictions, alerts). |
| **Observability** | 3/5 | Great static reports (`stats`, `doctor`). No real-time. No alerts. No export. |
| **Checkpointing** | 2/5 | Concept excellent. Reliability broken (schema mismatch). No session integration. Manual only. |
| **Recovery** | 4/5 | Gateway lifecycle clean. 5/5 consecutive runs without leaks. Kill mid-flight: handles well. |
| **Multi-project** | 4/5 | Two projects, back-to-back, zero contamination. Trust domain isolation correct. |
| **Multi-agent** | 2/5 | Architecture supports N agents. Product supports 2. Claude Code = full, Codex = pass-through only. |
| **Documentation** | 3/5 | CLI help = excellent (5/5). README = broken install instructions (2/5). Architecture doc = excellent (5/5). |
| **UX** | 2/5 | CLI is premium. Value is invisible. Setup is friction-heavy. No inline feedback. |
| **Reliability** | 4/5 | Runtime rock-solid. Schema migration fragile. Gateway lifecycle clean. |
| **Performance** | 4/5 | ~2-3s gateway overhead. No degradation over 5 consecutive runs. Acceptable for daily use. |

**Average: 3.07/5** — Toche is a solid 3-star product with a 4-star architecture.

---

## Part 3 — Gap Analysis

### Critical Gaps (block mission)

| # | Gap | Current State | Mission Requires | Severity |
|---|-----|--------------|-----------------|----------|
| 1 | **Multi-agent is aspirational** | 2 agents (1 full, 1 pass-through) | Multiple AI coding agents | 🔴 Critical |
| 2 | **Value is invisible** | Cache savings and coalescing hidden behind `toche stats` | User visibly benefits from intelligence | 🔴 Critical |
| 3 | **Checkpoint is unreliable** | Schema mismatch breaks on existing installations | Reliable persistent memory | 🔴 Critical |

### Important Gaps (degrade experience)

| # | Gap | Current State | Mission Requires | Severity |
|---|-----|--------------|-----------------|----------|
| 4 | **No active optimization** | Passive recording only | Intelligence layer that acts | 🟠 Important |
| 5 | **Setup is friction** | Terminal-only, config poisoning, broken npm | Seamless onboarding | 🟠 Important |
| 6 | **No cost controls** | Shows costs, can't limit them | Cost optimization | 🟠 Important |
| 7 | **No session integration** | Checkpoints and runs are separate features | Workflow enhancements | 🟠 Important |

### Minor Gaps (polish)

| # | Gap | Current State | Severity |
|---|-----|--------------|----------|
| 8 | No real-time observability | `toche stats` is pull-only | 🟡 Minor |
| 9 | No data export | Stats viewable in terminal only | 🟡 Minor |
| 10 | No agent migration tools | Can't switch between agents | 🟡 Minor |

### Future Gaps (post-1.2.0)

| # | Gap | Severity |
|---|-----|----------|
| 11 | Adapt beyond rule-based optimization (ML-driven cache TTL, cost prediction) | 🔵 Future |
| 12 | Cross-machine Toche sync | 🔵 Future |
| 13 | Enterprise policy enforcement | 🔵 Future |

---

## Part 4 — Product Identity

### What Toche IS today

| Label | Accuracy | Evidence |
|-------|----------|----------|
| *Claude Code companion* | ✅ Most accurate | 90% of `toche run` usage is Claude. Stats show 7/8 requests are Claude. |
| *AI cache/proxy* | ✅ Technically accurate | Gateway sits between client and upstream. Coalescing + safe cache work. |
| *Developer productivity tool* | ⚠️ Partially | Saves ~5s per `toche run` vs manual `claude -p`. Checkpoint concept is productive but unreliable. |
| *CLI utility* | ✅ Accurate | 10 subcommands, all work. `stats` and `doctor` are genuinely useful standalone. |
| *AI coding middleware* | ❌ Aspirational | Architecture supports it. Product doesn't deliver it (2 agents, 1 full). |
| *Intelligence layer* | ❌ Aspirational | Nothing learns. Nothing adapts. Nothing predicts. Visibility > intelligence. |
| *Infrastructure platform* | ❌ Not yet | No plugin system. No community adapters. No enterprise features. |

**Toche 1.1.0's honest identity: a Claude Code efficiency companion with excellent architecture that is ready to become more.**

### What Toche SHOULD become (3-5 years)

**The universal intelligence layer for AI coding agents.** Not just a proxy — a platform that:

1. Any AI coding agent routes through transparently (adapter layer → zero config)
2. Learns from usage patterns to optimize proactively (predictive caching, cost-aware routing)
3. Provides observability developers actually see (inline savings, cost budgets, weekly digests)
4. Preserves workflow context across sessions and agents (reliable checkpoints, agent migration)
5. Becomes the standard local middleware that AI coding tools integrate with — not because it forces them, but because it adds measurable value

This is what the mission statement describes. Toche 1.1.0 is on the path but not there yet.

---

## Part 5 — Mission-Aligned Roadmap

Every recommendation closes a specific gap. No feature for its own sake.

### 1.2.0: "Visible Intelligence"

| # | Feature | Gap Closed | Impact | Effort |
|---|---------|-----------|--------|--------|
| 1 | **Inline savings after every `toche run`** | Gap 2 — invisible value | Every user sees savings at point of use | 1-2 days |
| 2 | **`toche run --continue`** (pull last checkpoint) | Gap 7 — session integration | Checkpoint becomes part of daily workflow | 2-3 days |
| 3 | **Schema migration fix** (never show `version 11 > 9`) | Gap 3 — checkpoint reliability | Checkpoint works on 100% of machines | 1-2 days |
| 4 | **`toche run --dry-run --cost`** (predict cost) | Gap 6 — cost controls | Users see cost before burning tokens | 1-2 days |
| 5 | **Cost budget** (`toche config budget 10`) | Gap 6 — cost controls | 80% warning, 100% hard stop | 2-3 days |
| 6 | **`toche stats --watch`** (live updating) | Gap 8 — real-time observability | Terminal dashboard for active sessions | 2-3 days |

**1.2.0 total: 9-15 days. Closes all 3 critical gaps.**

### 1.3.0: "Universal Middleware"

| # | Feature | Gap Closed | Impact | Effort |
|---|---------|-----------|--------|--------|
| 7 | **`AgentAdapter` trait** | Gap 1 — multi-agent aspirational | Any agent can be integrated in ~200 lines | 3-5 days |
| 8 | **`ChatCompletionsProtocol`** | Gap 1 — protocol gap for OpenAI agents | Hermes, Aider, Cline get native protocol support | 2-3 days |
| 9 | **Hermes adapter** | Gap 1 — first proof of multi-agent | 217K★ agent joins Toche ecosystem | 2-3 days |
| 10 | **Aider adapter** | Gap 1 — second adapter (env-var only, trivial) | 47.5K★ agent, proves adapter speed | 1-2 days |
| 11 | **`toche doctor` → auto-discover all agents** | Gap 1 — user sees ecosystem | One command shows every installable agent | 1-2 days |
| 12 | **`toche setup --non-interactive`** | Gap 5 — setup friction | CI/CD, Docker, headless machines | 1-2 days |

**1.3.0 total: 10-17 days. Closes Gap 1 and Gap 5.**

### 1.4.0: "Platform"

| # | Feature | Gap Closed | Impact | Effort |
|---|---------|-----------|--------|--------|
| 13 | Community adapter template + docs | Gap 1 — ecosystem growth | Third parties write adapters | 2-3 days |
| 14 | `toche stats --export csv` | Gap 9 — data export | Analysis in spreadsheets, BI tools | 1 day |
| 15 | Weekly cost digest (cron + `toche stats --report`) | Gap 4 — active optimization | "You saved $4.20 this week from cache hits" | 2-3 days |
| 16 | Agent migration (`toche migrate --from aider --to claude`) | Gap 10 — agent flexibility | Switch agents without losing context | 3-5 days |
| 17 | `toche config profile` (project-level config) | Gap 7 — workflow | Different upstreams per project | 2-3 days |

**1.4.0 total: 10-15 days.**

### Post-1.4.0: "Intelligence Layer"

| # | Feature | Gap Closed | Impact | Effort |
|---|---------|-----------|--------|--------|
| 18 | Adaptive cache TTL (ML-driven freshness) | Gap 11 — adaptive intelligence | Cache learns which responses go stale | 5-10 days |
| 19 | Cost prediction before requests | Gap 4 — active optimization | "This request will cost ~$0.03. Continue?" | 3-5 days |
| 20 | Cross-machine Toche sync | Gap 12 | Laptop + desktop share cache and checkpoints | 5-10 days |

---

## Part 6 — Success Criteria

Toche can honestly claim to be "the intelligence layer for AI coding agents" when:

| # | Criterion | Measurable | Status Today |
|---|-----------|-----------|-------------|
| 1 | **5+ agents with production-quality adapters** | `toche doctor` lists 5+ agents with ✅ status | ❌ 2 agents |
| 2 | **Visible intelligence** — user sees savings, predictions, and insights inline | After-run summary shows `[toche] saved $0.42 | 3 cache hits` | ❌ Hidden |
| 3 | **99.9% checkpoint reliability** | Zero schema mismatch errors in 1000 test runs | ❌ Breaks on existing installs |
| 4 | **Active cost optimization** — budgets, alerts, predictions | User sets `$10/month`, gets 80% warning, 100% stop | ❌ Passive only |
| 5 | **Community adapters exist** | At least 1 community-written adapter merged | ❌ No adapter layer yet |
| 6 | **Zero-setup onboarding** | `toche setup` completes without terminal, npm install works | ❌ Both broken |
| 7 | **Session integration** | `toche run --continue` works without manual checkpoint save | ❌ Separate features |

---

## What NOT to Build

Based on M21 evidence:

| Feature | Why Not |
|---------|---------|
| Cursor/Windsurf integration | Closed IDEs, impossible to proxy. Revisit only if they add proxy support. |
| OpenClaw integration | It's a gateway, not a client. Architecture mismatch. |
| Cross-protocol translation (Anthropic↔OpenAI) | ADR-004 already decided against this. Adds complexity without clear value. |
| Native agent (Toche becoming its own AI agent) | Dilutes mission. Toche is middleware, not a generator. |
| Cloud-hosted Toche | Violates local-first privacy promise. Keep it local. |
| API marketplace (selling Toche as SaaS) | Different product category. Stay focused on local middleware. |

---

## Final Verdict

### NOT YET

Toche 1.1.0 cannot honestly claim to be "the intelligence layer for AI coding agents."

**It can claim to be: "A local gateway that optimizes Claude Code usage with caching, cost tracking, and session checkpoints — architected to expand to other agents in future releases."**

That is an honest, accurate, and still impressive statement. Toche 1.1.0 is a 79/100 product with a 90/100 architecture. The mission statement describes where Toche is going, not where it is.

**The path to mission fulfillment:** Close the three critical gaps in 1.2.0 (visible intelligence, checkpoint reliability, session integration). Add the adapter layer in 1.3.0. By 1.4.0, the mission statement becomes true.

The architecture was built right. Now the product needs to be visible.
