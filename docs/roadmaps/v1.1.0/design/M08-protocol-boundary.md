# M08 — Protocol-Driver Boundary Design

## Goal

Move Anthropic-specific interpretation behind a lossless protocol interface.

## Architecture

```
src/protocol/
  mod.rs          — Protocol trait + ResponseHeaders
  anthropic/
    mod.rs        — AnthropicProtocol implementation
```

## Trait surface

| Method | Responsibility | Delegates to |
|--------|---------------|-------------|
| `extract_model` | Parse "model" from JSON | inline serde_json |
| `path` | Return "/v1/messages" | constant |
| `fingerprint` | Normalize + hash request body | `shield::fingerprint::compute` |
| `parse_response_headers` | Extract Anthropic cache header tokens | inline |
| `inject_cache_control` | Inject ephemeral cache breakpoints | `cache::inject::inject_cache_control` |
| `is_streaming` | Check for `"stream": true` | inline serde_json |

## What moved

| Before | After |
|--------|-------|
| `routes.rs::extract_model()` | `AnthropicProtocol::extract_model()` |
| `routes.rs::is_streaming_request()` | `AnthropicProtocol::is_streaming()` |
| inline `"/v1/messages"` string | `AnthropicProtocol::path()` |
| `shield::fingerprint::compute()` called directly | `protocol.fingerprint()` via trait |
| inline header parsing in `forward_to_upstream()` | `protocol.parse_response_headers()` |
| direct `cache::inject::inject_cache_control()` | `protocol.inject_cache_control()` via trait |

## What stayed

- Coalescing (shield) — in routes.rs
- Safe cache — in routes.rs
- Reduction — in routes.rs
- Efficiency injection — in routes.rs
- Cache breakpoint detection — in `cache::breakpoint`
- Ledger recording — in routes.rs
- Identity context building — in routes.rs

## Design decisions

### Trait is thin and stateless

`AnthropicProtocol` is a unit struct. All methods are pure functions of their inputs. No mutable state, no configuration. This keeps the trait easy to implement for future protocols (OpenAI Responses in M09).

### Fingerprint stays in shield module

The `shield::fingerprint::compute()` function and its normalization logic remain in place. `AnthropicProtocol::fingerprint()` delegates to it. This avoids moving tested normalization code while still routing all protocol-specific calls through the trait.

### Cache injection stays in cache module

The `cache::inject::inject_cache_control()` function and its `BreakpointPlan` parameter remain in the cache module. `AnthropicProtocol::inject_cache_control()` delegates. The protocol trait owns the "should I inject" decision; the cache module owns the "how to inject" implementation.

### forward_to_upstream now takes protocol reference

The helper function now accepts `&dyn Protocol` for URL path construction and response header parsing. This is the only function signature change — callers pass the protocol reference through.

## M09 readiness

When M09 adds OpenAI Responses support:
1. Create `src/protocol/openai_responses/mod.rs` with `OpenAiResponsesProtocol`
2. Wire it into routes.rs based on integration configuration
3. The pipeline (coalescing, safe cache, etc.) remains unchanged
4. Protocol-specific differences are isolated to the trait implementation
