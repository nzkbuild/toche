/// Result of inspecting a response body for cache eligibility.
pub struct SafetyVerdict {
    /// Whether this response is safe to cache and replay.
    pub safe: bool,
    /// Why the response was deemed unsafe (empty if safe).
    pub reason: String,
    /// Number of tool_use blocks found (0 if safe).
    pub tool_use_count: usize,
}

/// Inspect an Anthropic Messages API response body and determine if it is
/// safe to cache. Unsafe responses are those containing tool_use content
/// blocks or a tool_use stop_reason.
///
/// Handles both standard JSON responses and SSE (Server-Sent Events)
/// streaming responses.
pub fn inspect_response(body_bytes: &[u8]) -> SafetyVerdict {
    if body_bytes.is_empty() {
        return SafetyVerdict {
            safe: false,
            reason: "empty body".into(),
            tool_use_count: 0,
        };
    }

    let text = String::from_utf8_lossy(body_bytes);

    // Check for SSE streaming format — scan for tool_use in content_block
    // data lines. The pattern we look for in SSE text:
    //   data: {..."content_block":{"type":"tool_use"...}...}
    // Or more practically, scan for "tool_use" within the SSE text.
    //
    // We look for the literal JSON substring that indicates a tool_use
    // content block: "type":"tool_use" appearing in a content_block context.
    let is_sse = text.contains("event: ") || text.starts_with("data:");

    if is_sse {
        return inspect_sse(&text);
    }

    // Try standard (non-streaming) JSON response
    inspect_json(&text)
}

fn inspect_sse(text: &str) -> SafetyVerdict {
    let mut tool_use_count = 0;
    let mut has_tool_use_stop = false;

    for line in text.lines() {
        let data = if let Some(d) = line.strip_prefix("data: ") {
            d
        } else {
            continue;
        };

        if data == "[DONE]" {
            continue;
        }

        // Check for content_block_start with tool_use type
        if data.contains("\"content_block\":{\"type\":\"tool_use\"") {
            tool_use_count += 1;
        }

        // Check for stop_reason: tool_use in message_delta
        if data.contains("\"stop_reason\":\"tool_use\"") {
            has_tool_use_stop = true;
        }
    }

    if tool_use_count > 0 {
        SafetyVerdict {
            safe: false,
            reason: format!("SSE response contains {} tool_use block(s)", tool_use_count),
            tool_use_count,
        }
    } else if has_tool_use_stop {
        SafetyVerdict {
            safe: false,
            reason: "stop_reason is tool_use".into(),
            tool_use_count: 0,
        }
    } else if tool_use_count == 0 && !has_tool_use_stop && text.contains("\"type\":\"text\"") {
        SafetyVerdict {
            safe: true,
            reason: String::new(),
            tool_use_count: 0,
        }
    } else {
        // SSE response we can't classify — conservative: deny
        SafetyVerdict {
            safe: false,
            reason: "SSE response format not recognized as safe".into(),
            tool_use_count: 0,
        }
    }
}

fn inspect_json(text: &str) -> SafetyVerdict {
    let root: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => {
            return SafetyVerdict {
                safe: false,
                reason: "invalid JSON".into(),
                tool_use_count: 0,
            };
        }
    };

    // Count tool_use content blocks first
    let content = root.get("content").and_then(|v| v.as_array());
    let mut tool_use_count = 0;

    if let Some(blocks) = content {
        for block in blocks {
            if let Some(typ) = block.get("type").and_then(|v| v.as_str()) {
                if typ == "tool_use" {
                    tool_use_count += 1;
                }
            }
        }

        if tool_use_count > 0 {
            return SafetyVerdict {
                safe: false,
                reason: format!("contains {} tool_use block(s)", tool_use_count),
                tool_use_count,
            };
        }
    }

    // Check stop_reason after counting tool_use blocks
    if let Some(stop_reason) = root.get("stop_reason").and_then(|v| v.as_str()) {
        if stop_reason == "tool_use" {
            return SafetyVerdict {
                safe: false,
                reason: "stop_reason is tool_use".into(),
                tool_use_count: 0,
            };
        }
    }

    // If we got here and there's no content array, check if this is a message_start
    // or other meta event type that shouldn't be cached
    if content.is_none() {
        let event_type = root.get("type").and_then(|v| v.as_str());
        if event_type == Some("message_start")
            || event_type == Some("content_block_start")
            || event_type == Some("ping")
        {
            return SafetyVerdict {
                safe: false,
                reason: format!("non-cacheable event type: {:?}", event_type),
                tool_use_count: 0,
            };
        }
        // Other unrecognized JSON — conservative deny
        return SafetyVerdict {
            safe: false,
            reason: "unrecognized response format".into(),
            tool_use_count: 0,
        };
    }

    // Content array exists and has no tool_use blocks
    if content.map(|c| c.is_empty()).unwrap_or(true) {
        return SafetyVerdict {
            safe: false,
            reason: "no content blocks in response".into(),
            tool_use_count: 0,
        };
    }

    SafetyVerdict {
        safe: true,
        reason: String::new(),
        tool_use_count: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only_response_is_safe() {
        let body = r#"{"type":"message","role":"assistant","content":[{"type":"text","text":"Hello!"}],"stop_reason":"end_turn"}"#;
        let verdict = inspect_response(body.as_bytes());
        assert!(verdict.safe, "text-only response should be safe, got: {}", verdict.reason);
    }

    #[test]
    fn tool_use_response_is_unsafe() {
        let body = r#"{"type":"message","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"read","input":{}}],"stop_reason":"tool_use"}"#;
        let verdict = inspect_response(body.as_bytes());
        assert!(!verdict.safe);
        assert!(verdict.tool_use_count > 0);
    }

    #[test]
    fn stop_reason_tool_use_is_unsafe() {
        // Even if content is empty (edge case), tool_use stop_reason is unsafe
        let body = r#"{"type":"message","role":"assistant","content":[],"stop_reason":"tool_use"}"#;
        let verdict = inspect_response(body.as_bytes());
        assert!(!verdict.safe);
    }

    #[test]
    fn empty_body_is_unsafe() {
        let verdict = inspect_response(b"");
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("empty"));
    }

    #[test]
    fn invalid_json_is_unsafe() {
        let verdict = inspect_response(b"not json at all");
        assert!(!verdict.safe);
    }

    #[test]
    fn sse_text_only_is_safe() {
        let sse = "\
event: message_start
data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\"}}

event: content_block_start
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}

event: content_block_delta
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello!\"}}

event: content_block_stop
data: {\"type\":\"content_block_stop\",\"index\":0}

event: message_delta
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}}

event: message_stop
data: {\"type\":\"message_stop\"}
";
        let verdict = inspect_response(sse.as_bytes());
        assert!(verdict.safe, "text-only SSE should be safe, got: {}", verdict.reason);
    }

    #[test]
    fn sse_with_tool_use_detected() {
        let sse = "\
event: message_start
data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\"}}

event: content_block_start
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"read\",\"input\":{}}}

event: content_block_stop
data: {\"type\":\"content_block_stop\",\"index\":0}

event: message_delta
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null}}

event: message_stop
data: {\"type\":\"message_stop\"}
";
        let verdict = inspect_response(sse.as_bytes());
        assert!(!verdict.safe, "SSE with tool_use should be unsafe");
        assert!(verdict.tool_use_count > 0);
    }
}
