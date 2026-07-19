# M14 — Documentation and Product Hierarchy

**Commit:** b72e3a2 (base; new commit pending)
**Date:** 2026-07-20

## Files changed

- `README.md` — rewritten for multi-client 1.1.0: what Toche is/is not,
  supported clients, persistent vs managed mode, trust isolation, data
  storage, measurement confidence, upgrade guide, clean uninstall
- `CHANGELOG.md` — prepended v1.1.0 section structured by Added/Changed/Fixed
- `CONTRIBUTING.md` — new: setup, conventions, PR workflow, architecture
  principles, project structure, testing guide
- `CODE_OF_CONDUCT.md` — new: Contributor Covenant 2.1
- `docs/ARCHITECTURE.md` — rewritten: system design with control-plane /
  payload-plane split, full crate map, data flow diagrams for both protocols,
  trust-domain derivation, configuration model, identity model, coalescing
  store design, database schema, CAS layout, HTTP routes, secret handling,
  architecture decision records (6 ADRs), known limitations
- `docs/roadmaps/v1.1.0/evidence/M14-documentation.md` — this file

## Reused sources

None — documentation is original writing based on codebase inspection.

## Acceptance gate

| Criterion | Status |
|-----------|--------|
| A new user can understand what Toche is | PASS — README lead, "What Toche is / is not" sections |
| A new user can understand what Toche is not | PASS — explicit non-goals listed |
| Why the runtime must be started | PASS — explained in install flow, persistent vs managed mode |
| How multiple clients connect | PASS — supported clients table, persistent/managed mode docs |
| Where requests go | PASS — architecture data flow, upstreams config, how-it-works diagram |
| What Toche stores | PASS — data storage table in README, database schema in ARCHITECTURE |
| What reported savings mean | PASS — measurement confidence section, caveat about list-price estimates |

## Product hierarchy

Primary commands documented:
- `setup` — guided, rerunnable configuration
- runtime (`toche`) — start the gateway
- `doctor` — verify installation and configuration
- `status` — live runtime state
- `stats` — usage, tokens, cost estimates
- `connect` / `disconnect` — per-client routing
- `run` — managed mode
- `expand` — restore original tool output

Advanced/compatibility commands documented:
- `cache` — persistent safe-cache management
- `checkpoint` — session continuity
- `graph` — knowledge graph

## Covered documentation topics

- [x] Supported clients (Claude Code, Codex CLI)
- [x] Supported protocols (Anthropic Messages, OpenAI Responses)
- [x] Persistent versus managed mode
- [x] Multiple simultaneous clients
- [x] Upstream neutrality
- [x] Trust isolation
- [x] Data storage (config, ledger, CAS, runtime_id)
- [x] Measurement confidence classifications
- [x] Known limitations (ARCHITECTURE.md)
- [x] Upgrade from 1.0.x (README + CHANGELOG)
- [x] Clean disconnect/uninstall (README)
- [x] Third-party notices (referenced in README)

## Gate status

| Gate | Status |
|------|--------|
| README explains product for new users | PASS |
| CHANGELOG covers v1.0.0 through v1.1.0 | PASS |
| CONTRIBUTING.md covers setup, conventions, PR workflow | PASS |
| Code of Conduct is standard Contributor Covenant | PASS |
| ARCHITECTURE.md covers system design, crate map, data flow, ADRs | PASS |
| Evidence file committed | PENDING (this file) |

## Next unlocked milestone

M15 — Full Release Audit
