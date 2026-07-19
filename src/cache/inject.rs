use serde_json::{Value, json};

use super::breakpoint::BreakpointPlan;

/// Inject `cache_control: {"type": "ephemeral"}` into the request body at the
/// positions specified by the breakpoint plan.
///
/// Returns the modified JSON string, or the original body text if the plan
/// has no breakpoints.
pub fn inject_cache_control(original_body: &str, plan: &BreakpointPlan) -> Result<String, String> {
    if !plan.has_breakpoints() {
        return Ok(original_body.to_string());
    }

    let mut root: Value =
        serde_json::from_str(original_body).map_err(|e| format!("Failed to parse body: {e}"))?;

    let cache_control = json!({"type": "ephemeral"});

    // Inject into system content block
    if let Some(idx) = plan.system_block_index {
        if let Some(blocks) = root.get_mut("system").and_then(|s| s.as_array_mut()) {
            if let Some(block) = blocks.get_mut(idx) {
                if let Some(obj) = block.as_object_mut() {
                    obj.insert("cache_control".to_string(), cache_control.clone());
                }
            }
        }
    }

    // Inject into message content blocks
    if let Some(messages) = root.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for &(msg_idx, block_idx) in &plan.message_blocks {
            if let Some(msg) = messages.get_mut(msg_idx) {
                if let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
                    if let Some(block) = content.get_mut(block_idx) {
                        if let Some(obj) = block.as_object_mut() {
                            obj.insert("cache_control".to_string(), cache_control.clone());
                        }
                    }
                }
            }
        }
    }

    serde_json::to_string(&root).map_err(|e| format!("Failed to serialize modified body: {e}"))
}

#[cfg(test)]
mod tests {
    use crate::config::toche_config::CacheBreakpoint;

    use super::super::breakpoint::find_breakpoints;
    use super::*;

    #[test]
    fn test_system_breakpoint_injected() {
        let body = r#"{
            "model": "claude-sonnet-5",
            "system": [{"type": "text", "text": "You are helpful."}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]}
            ]
        }"#;
        let plan = find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
        let modified = inject_cache_control(body, &plan).unwrap();
        assert!(
            modified.contains("cache_control"),
            "Modified body must contain cache_control"
        );
        assert_ne!(modified, body, "Modified body must differ from original");

        let parsed: Value = serde_json::from_str(&modified).unwrap();
        let system_last = &parsed["system"][0].as_object().unwrap();
        assert!(
            system_last.contains_key("cache_control"),
            "System block must have cache_control"
        );
        assert_eq!(system_last["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_empty_plan_returns_original() {
        let body = r#"{"model": "gpt-4", "messages": []}"#;
        let plan = BreakpointPlan {
            system_block_index: None,
            message_blocks: vec![],
        };
        let modified = inject_cache_control(body, &plan).unwrap();
        assert_eq!(modified, body);
    }

    #[test]
    fn test_no_side_effects_on_other_fields() {
        let body = r#"{
            "model": "claude-sonnet-5",
            "max_tokens": 1024,
            "temperature": 0.7,
            "system": [{"type": "text", "text": "You are helpful."}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]}
            ]
        }"#;
        let plan = find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
        let modified = inject_cache_control(body, &plan).unwrap();
        let parsed: Value = serde_json::from_str(&modified).unwrap();
        assert_eq!(parsed["model"], "claude-sonnet-5");
        assert_eq!(parsed["max_tokens"], 1024);
        assert_eq!(parsed["temperature"], json!(0.7));
        assert_eq!(parsed["messages"][0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_message_breakpoint_injected() {
        let body = r#"{
            "model": "claude-sonnet-5",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "First"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "Second"}]},
                {"role": "user", "content": [{"type": "text", "text": "Third"}]}
            ]
        }"#;
        let plan = find_breakpoints(body, &CacheBreakpoint::SystemOnly).unwrap();
        let plan = BreakpointPlan {
            system_block_index: plan.system_block_index,
            message_blocks: vec![(2, 0)],
        };
        let modified = inject_cache_control(body, &plan).unwrap();
        let parsed: Value = serde_json::from_str(&modified).unwrap();
        let block = &parsed["messages"][2]["content"][0].as_object().unwrap();
        assert!(block.contains_key("cache_control"));
    }
}
