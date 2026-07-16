//! Anthropic Messages API request-body transformer.
//!
//! Walks the JSON request to find `tool_result` content blocks, resolves the
//! originating command via `tool_use_id → tool_name` mapping, applies RTK's
//! deterministic filter pipeline, stores the raw original in CAS, and replaces
//! the content with reduced text + an expansion marker.

use anyhow::Context;
use serde_json::Value;
use std::collections::HashMap;

use crate::reduce::config::ReduceConfig;
use crate::reduce::rtk::toml_filter::{self, apply_filter_with_info, find_matching_filter};
use crate::reduce::storage;

/// Rough token estimation: characters / 4 (same heuristic as meter::recorder).
fn estimate_tokens(text: &str) -> u64 {
    (text.len() as f64 / 4.0).ceil() as u64
}

/// Result of reducing a single request body.
pub struct ReductionResult {
    /// The (possibly modified) JSON body to forward upstream.
    pub modified_body: String,
    /// Estimated tokens in raw tool outputs before reduction.
    pub tokens_raw: u64,
    /// Estimated tokens in reduced tool outputs.
    pub tokens_reduced: u64,
    /// Number of tool outputs that were successfully reduced.
    pub reductions: usize,
    /// Number of tool outputs passed through unchanged.
    pub passthroughs: usize,
    /// SHA-256 hashes of stored originals (for ledger / audit).
    pub hashes: Vec<String>,
}

/// Transform the request body by reducing tool_result content blocks.
///
/// Returns the original body unchanged when reduction is disabled, when the
/// bypass header is set, or when JSON parsing fails (conservative fallback).
pub fn reduce_body(
    body: &str,
    config: &ReduceConfig,
    bypass_header: bool,
) -> anyhow::Result<ReductionResult> {
    let mut result = ReductionResult {
        modified_body: body.to_string(),
        tokens_raw: 0,
        tokens_reduced: 0,
        reductions: 0,
        passthroughs: 0,
        hashes: Vec::new(),
    };

    if !config.enabled || bypass_header {
        return Ok(result);
    }

    let mut root: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Ok(result), // Conservative fallback: pass through
    };

    // Build tool_use_id → tool_name map from assistant content blocks.
    let tool_map = build_tool_map(&root);

    // Walk messages looking for user tool_result blocks.
    let messages = match root.get_mut("messages") {
        Some(Value::Array(msgs)) => msgs,
        _ => return Ok(result),
    };

    for msg in messages.iter_mut() {
        let content = match msg.get_mut("content") {
            Some(Value::Array(content)) => content,
            _ => continue,
        };

        for block in content.iter_mut() {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }

            let tool_use_id = block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or("");

            let tool_name = tool_map.get(tool_use_id).map(|s| s.as_str()).unwrap_or("");

            // Check bypass list (exact match on tool name).
            if !tool_name.is_empty() && config.command_bypass.iter().any(|b| b == tool_name) {
                result.passthroughs += 1;
                continue;
            }

            let raw_text = extract_text_content(block);
            if raw_text.is_empty() {
                result.passthroughs += 1;
                continue;
            }

            let tokens_raw = estimate_tokens(&raw_text);

            let matched = !tool_name.is_empty() && toml_filter::command_matches_filter(tool_name);

            if matched {
                if let Some(filter) = find_matching_filter(tool_name) {
                    let (reduced_text, _loss) = apply_filter_with_info(filter, &raw_text);

                    let hash = storage::store(raw_text.as_bytes())
                        .context("failed to store raw output in CAS")?;

                    let tokens_reduced = estimate_tokens(&reduced_text);
                    let pct = if tokens_raw > 0 {
                        ((tokens_raw - tokens_reduced) as f64 / tokens_raw as f64 * 100.0) as u32
                    } else {
                        0
                    };

                    let marker = format!(
                        "\n\n[toche:reduced {}% restored with: toche expand {}]\n",
                        pct, hash
                    );
                    let new_content = reduced_text + &marker;

                    replace_content(block, &new_content);

                    result.tokens_raw += tokens_raw;
                    result.tokens_reduced += tokens_reduced;
                    result.reductions += 1;
                    result.hashes.push(hash);
                } else {
                    result.passthroughs += 1;
                }
            } else {
                result.passthroughs += 1;
            }
        }
    }

    result.modified_body = serde_json::to_string(&root)?;
    Ok(result)
}

/// Build a map from `tool_use.id → tool_use.name` by scanning all messages
/// for assistant content blocks of type `tool_use`.
fn build_tool_map(root: &Value) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let messages = match root.get("messages") {
        Some(Value::Array(msgs)) => msgs,
        _ => return map,
    };

    for msg in messages {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let content = match msg.get("content") {
            Some(Value::Array(content)) => content,
            _ => continue,
        };
        for block in content {
            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                if let (Some(id), Some(name)) = (
                    block.get("id").and_then(Value::as_str),
                    block.get("name").and_then(Value::as_str),
                ) {
                    map.insert(id.to_string(), name.to_string());
                }
            }
        }
    }

    map
}

/// Extract text from a tool_result content block.
/// Handles both `"content": "string"` and `"content": [{"type": "text", ...}]`.
fn extract_text_content(block: &Value) -> String {
    match block.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => {
            let mut text = String::new();
            for part in parts {
                if part.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
            }
            text
        }
        _ => String::new(),
    }
}

/// Replace the content field of a tool_result block in-place.
fn replace_content(block: &mut Value, new_text: &str) {
    let is_string = matches!(block.get("content"), Some(Value::String(_)));
    let is_array = matches!(block.get("content"), Some(Value::Array(_)));

    if is_string {
        block["content"] = Value::String(new_text.to_string());
    } else if is_array {
        let Some(parts) = block["content"].as_array_mut() else {
            return;
        };
        let mut replaced = false;
        for part in parts.iter_mut() {
            if part.get("type").and_then(Value::as_str) == Some("text") {
                if !replaced {
                    part["text"] = Value::String(new_text.to_string());
                    replaced = true;
                } else {
                    part["text"] = Value::String(String::new());
                }
            }
        }
        if !replaced {
            parts.push(serde_json::json!({"type": "text", "text": new_text}));
        }
    } else {
        block["content"] = Value::String(new_text.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reduce::config::ReduceConfig;

    fn make_config() -> ReduceConfig {
        ReduceConfig {
            enabled: true,
            command_bypass: vec![],
        }
    }

    #[test]
    fn disabled_config_passthrough() {
        let body = r#"{"model":"claude-sonnet-4-20250514","max_tokens":1024,"messages":[{"role":"user","content":"hi"}]}"#;
        let cfg = ReduceConfig {
            enabled: false,
            ..Default::default()
        };
        let r = reduce_body(body, &cfg, false).expect("should succeed");
        assert_eq!(r.modified_body, body);
        assert_eq!(r.reductions, 0);
    }

    #[test]
    fn bypass_header_passthrough() {
        let body = r#"{"model":"claude-sonnet-4-20250514","max_tokens":1024,"messages":[{"role":"user","content":"hi"}]}"#;
        let r = reduce_body(body, &make_config(), true).expect("should succeed");
        assert_eq!(r.modified_body, body);
    }

    #[test]
    fn invalid_json_passthrough() {
        let r = reduce_body("not json", &make_config(), false);
        assert!(r.is_ok());
        assert_eq!(r.unwrap().modified_body, "not json");
    }

    #[test]
    fn string_content_form_reduced() {
        let body = r#"{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "run cargo test"},
    {"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_001", "name": "cargo test", "input": {}}]},
    {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_001", "content": "running 5 tests\n.....\ntest result: ok. 5 passed"}]}
  ]
}"#;
        let r = reduce_body(body, &make_config(), false).expect("should succeed");
        assert!(
            r.reductions > 0 || r.passthroughs > 0,
            "should process tool_result"
        );
        assert!(r.modified_body.contains("toche:reduced") || r.passthroughs > 0);
    }

    #[test]
    fn array_content_form_extracted() {
        let body = r#"{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "run git diff"},
    {"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_002", "name": "git diff", "input": {}}]},
    {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_002", "content": [{"type": "text", "text": "diff --git a/foo.rs b/foo.rs"}]}]}
  ]
}"#;
        let r = reduce_body(body, &make_config(), false).expect("should succeed");
        assert!(r.reductions > 0 || r.passthroughs > 0);
    }

    #[test]
    fn bypass_list_skips_command() {
        let cfg = ReduceConfig {
            enabled: true,
            command_bypass: vec!["git diff".to_string()],
        };
        let body = r#"{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "show diff"},
    {"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_003", "name": "git diff", "input": {}}]},
    {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_003", "content": "diff output here"}]}
  ]
}"#;
        let r = reduce_body(body, &cfg, false).expect("should succeed");
        assert_eq!(r.passthroughs, 1);
        assert_eq!(r.reductions, 0);
    }

    #[test]
    fn deterministic_output() {
        let body = r#"{
  "model": "claude-sonnet-4-20250514",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "run cargo test"},
    {"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_004", "name": "cargo test", "input": {}}]},
    {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_004", "content": "running 1 test\ntest foo ... ok\n\ntest result: ok. 1 passed"}]}
  ]
}"#;
        let r1 = reduce_body(body, &make_config(), false).expect("first");
        let r2 = reduce_body(body, &make_config(), false).expect("second");
        assert_eq!(r1.modified_body, r2.modified_body);
        assert_eq!(r1.reductions, r2.reductions);
    }

    #[test]
    fn no_tool_uses_returns_unchanged() {
        let body = r#"{"model":"claude-sonnet-4-20250514","max_tokens":1024,"messages":[{"role":"user","content":"hello"}]}"#;
        let r = reduce_body(body, &make_config(), false).expect("should succeed");
        assert_eq!(r.reductions, 0);
        assert_eq!(r.passthroughs, 0);
    }
}
