# M09 — OpenAI Responses Ingress Design

## Goal

Add a second real protocol and prove the protocol boundary is not only an imagined abstraction. OpenAI Responses is pass-through only in 1.1.0.

## Decision

**Route-based protocol selection.** No protocol field in config. Each route handler statically knows its protocol:

| Route | Protocol | Pipeline |
|-------|----------|----------|
| `/v1/messages` | `AnthropicProtocol` | Full (shield, safe cache, reduce, efficiency, cache inject, ledger) |
| `/v1/responses` | `OpenAiResponsesProtocol` | Pass-through only (identity, timing, ledger, no optimizations) |

## Architecture

```
src/protocol/
  mod.rs              — Protocol trait + ResponseHeaders (unchanged)
  anthropic/
    mod.rs            — AnthropicProtocol (unchanged)
  openai_responses/
    mod.rs            — OpenAiResponsesProtocol (NEW)
```

## Trait implementation for OpenAiResponsesProtocol

| Method | Behavior | Rationale |
|--------|----------|-----------|
| `extract_model` | Parse `"model"` from JSON body | Same JSON field name, compatible |
| `path` | Return `"/v1/responses"` | OpenAI Responses endpoint |
| `fingerprint` | Delegate to `shield::fingerprint::compute` | Same normalization + SHA-256 |
| `parse_response_headers` | Return `ResponseHeaders::default()` (zeros) | OpenAI doesn't use Anthropic cache headers |
| `inject_cache_control` | Return body unchanged | No cache breakpoint injection for pass-through |
| `is_streaming` | Check `"stream": true` in JSON body | Same boolean field |

## Routes.rs changes

A new `responses` handler function. It is a simplified version of `messages`:

**Included (same as messages):**
- Request identity (runtime_id, request_id, integration, upstream, trust domain, etc.)
- Timing (RequestTimer)
- Model extraction
- Header forwarding (strip host/content-length, add integration headers, auth)
- Streaming detection
- Upstream forwarding via `forward_to_upstream`
- Ledger recording (fire-and-forget)
- Continuity observer

**Excluded (pass-through only):**
- Shield/flight coalescing
- Safe cache check
- Reduction
- Efficiency injection
- Cache breakpoint injection
- Safe cache storage on success

## Server.rs changes

Add one route:

```rust
.route("/v1/responses", axum::routing::post(super::routes::responses))
```

## What stays unchanged

- Protocol trait definition
- AnthropicProtocol implementation
- `forward_to_upstream` helper
- `AppState`
- Health/ready endpoints
- All existing tests

## M09 readiness for future milestones

When optimization support is added for OpenAI Responses:
1. Add protocol-specific methods to the trait (e.g., cache header parsing)
2. Enable pipeline stages selectively in the `responses` handler
3. Each stage requires its own evidence gate
