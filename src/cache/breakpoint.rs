use serde_json::Value;

use crate::profiles::types::CacheBreakpoint;

/// Describes where cache_control breakpoints should be placed in a request body.
#[derive(Debug, Clone, PartialEq)]
pub struct BreakpointPlan {
    /// Index of the system content block to mark (if system is a content-block array).
    pub system_block_index: Option<usize>,
    /// Pairs of (message_index, content_block_index) to mark in the messages array.
    pub message_blocks: Vec<(usize, usize)>,
}

impl BreakpointPlan {
    pub fn has_breakpoints(&self) -> bool {
        self.system_block_index.is_some() || !self.message_blocks.is_empty()
    }
}

/// Find where cache breakpoints should be placed in an Anthropic Messages request body.
///
/// Breakpoints mark the end of a cacheable prefix. Anthropic caches from the start
/// of the conversation through the content block carrying `cache_control`.
pub fn find_breakpoints(body: &str, policy: &CacheBreakpoint) -> Result<BreakpointPlan, String> {
    let root: Value =
        serde_json::from_str(body).map_err(|e| format!("Failed to parse request body: {e}"))?;

    let mut plan = BreakpointPlan {
        system_block_index: None,
        message_blocks: Vec::new(),
    };

    // Cache system prompt — last content block of the system array
    if let Some(system) = root.get("system") {
        if let Some(blocks) = system.as_array() {
            if !blocks.is_empty() {
                plan.system_block_index = Some(blocks.len() - 1);
            }
        }
    }

    match policy {
        CacheBreakpoint::SystemOnly => return Ok(plan),
        CacheBreakpoint::Standard => {}
    }

    // Walk messages array, cache through longest prefix before first tool interaction.
    // A tool interaction is: assistant message with tool_use content, or user message
    // with tool_result content.
    let messages = match root.get("messages") {
        Some(Value::Array(arr)) => arr,
        _ => return Ok(plan),
    };

    let mut last_non_tool_content_idx: Option<(usize, usize)> = None;

    for (mi, msg) in messages.iter().enumerate() {
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("");

        let content = match msg.get("content") {
            Some(Value::Array(arr)) => arr,
            _ => continue,
        };

        if content.is_empty() {
            continue;
        }

        let is_tool_use = role == "assistant"
            && content
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"));
        let is_tool_result = role == "user"
            && content
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"));

        if is_tool_use || is_tool_result {
            break; // Stop caching at first tool interaction
        }

        last_non_tool_content_idx = Some((mi, content.len() - 1));
    }

    if let Some(idx) = last_non_tool_content_idx {
        plan.message_blocks.push(idx);
    }

    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_and_messages_standard() {
        let body = r#"{
            "model": "claude-sonnet-5",
            "system": [{"type": "text", "text": "You are a helpful assistant."}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "Hi!"}]},
                {"role": "user", "content": [{"type": "text", "text": "What is Rust?"}]}
            ]
        }"#;
        let plan = find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
        assert_eq!(plan.system_block_index, Some(0));
        assert_eq!(plan.message_blocks, vec![(2, 0)]);
    }

    #[test]
    fn test_tool_interaction_breaks_cache() {
        let body = r#"{
            "model": "claude-sonnet-5",
            "system": [{"type": "text", "text": "You are a coder."}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "ls"}]},
                {"role": "assistant", "content": [{"type": "tool_use", "name": "bash", "id": "1", "input": {}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "1", "content": "file.txt"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "Found file.txt"}]}
            ]
        }"#;
        let plan = find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
        assert_eq!(plan.system_block_index, Some(0));
        // First tool_use breaks the run at message index 0 (the "ls" user message)
        assert_eq!(plan.message_blocks, vec![(0, 0)]);
    }

    #[test]
    fn test_system_only() {
        let body = r#"{
            "model": "claude-sonnet-5",
            "system": [{"type": "text", "text": "You are a helpful assistant."}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "Hi!"}]}
            ]
        }"#;
        let plan = find_breakpoints(body, &CacheBreakpoint::SystemOnly).unwrap();
        assert_eq!(plan.system_block_index, Some(0));
        assert!(plan.message_blocks.is_empty());
    }

    #[test]
    fn test_empty_messages() {
        let body = r#"{"model": "claude-sonnet-5", "messages": []}"#;
        let plan = find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
        assert!(!plan.has_breakpoints());
    }

    #[test]
    fn test_no_system() {
        let body = r#"{
            "model": "claude-sonnet-5",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]}
            ]
        }"#;
        let plan = find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
        assert_eq!(plan.system_block_index, None);
        assert_eq!(plan.message_blocks, vec![(0, 0)]);
    }

    #[test]
    fn test_system_as_string_not_array() {
        // system as a plain string: no individual content blocks to mark
        let body = r#"{
            "model": "claude-sonnet-5",
            "system": "You are helpful.",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]}
            ]
        }"#;
        let plan = find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
        assert_eq!(plan.system_block_index, None);
        assert_eq!(plan.message_blocks, vec![(0, 0)]);
    }
}
