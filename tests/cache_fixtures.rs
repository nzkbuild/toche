use toche::cache::breakpoint;
use toche::cache::inject;
use toche::config::toche_config::CacheBreakpoint;

#[test]
fn repeated_prefix_yields_cache_breakpoints() {
    let body = r#"{
        "model": "claude-sonnet-5",
        "system": [{"type": "text", "text": "You are a helpful assistant."}],
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
            {"role": "assistant", "content": [{"type": "text", "text": "Hi there!"}]},
            {"role": "user", "content": [{"type": "text", "text": "What is Rust?"}]}
        ]
    }"#;

    let plan = breakpoint::find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
    assert!(
        plan.system_block_index.is_some() || !plan.message_blocks.is_empty(),
        "Should find at least one breakpoint"
    );

    let modified = inject::inject_cache_control(body, &plan).unwrap();
    assert!(
        modified.contains("cache_control"),
        "Modified body must contain cache_control"
    );
    assert_ne!(modified, body, "Modified body must differ from original");

    let parsed: serde_json::Value = serde_json::from_str(&modified).unwrap();
    let system_last = &parsed["system"][0].as_object().unwrap();
    assert!(
        system_last.contains_key("cache_control"),
        "Last system block must have cache_control"
    );
}

#[test]
fn unsupported_upstream_untouched() {
    let body = r#"{"model": "gpt-4", "messages": []}"#;
    let plan = breakpoint::find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
    assert!(plan.system_block_index.is_none());
    assert!(plan.message_blocks.is_empty());
    let modified = inject::inject_cache_control(body, &plan).unwrap();
    assert!(!modified.contains("cache_control"));
}

#[test]
fn tool_messages_break_cache_run() {
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

    let plan = breakpoint::find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
    let modified = inject::inject_cache_control(body, &plan).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&modified).unwrap();

    // Verify no cache_control on the tool_use message (index 1)
    let tool_use_block = &parsed["messages"][1]["content"][0];
    assert!(
        !tool_use_block
            .as_object()
            .unwrap()
            .contains_key("cache_control"),
        "Tool_use content block must not have cache_control"
    );
}

#[test]
fn observe_mode_breakpoints_are_non_destructive() {
    // find_breakpoints takes &str — it cannot mutate the input by construction.
    // This test validates that the plan for a real-world conversation is sensible.
    let body = r#"{
        "model": "claude-sonnet-5",
        "system": [{"type": "text", "text": "You are a coding assistant."}],
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "Write a function"}]},
            {"role": "assistant", "content": [{"type": "text", "text": "Here is the function:"}]},
            {"role": "user", "content": [{"type": "text", "text": "Add docs"}]}
        ]
    }"#;

    let plan = breakpoint::find_breakpoints(body, &CacheBreakpoint::Standard).unwrap();
    // System block gets a breakpoint, and the last (non-tool) message gets one
    assert!(plan.system_block_index.is_some());
    assert_eq!(plan.message_blocks, vec![(2, 0)]);
}
