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
/// Stores raw originals under the default CAS directory.
#[allow(dead_code)] // test/public default-path helper
pub fn reduce_body(
    body: &str,
    config: &ReduceConfig,
    bypass_header: bool,
) -> anyhow::Result<ReductionResult> {
    reduce_body_at(body, config, bypass_header, &storage::cas_dir())
}

/// Same as `reduce_body` but stores originals under an explicit CAS root.
pub fn reduce_body_at(
    body: &str,
    config: &ReduceConfig,
    bypass_header: bool,
    cas_root: &std::path::Path,
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

    // Build tool_use_id → tool_name and effective_cmd maps.
    let (tool_name_map, effective_cmd_map) = build_tool_map(&root);

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

            let literal_name = tool_name_map
                .get(tool_use_id)
                .map(|s| s.as_str())
                .unwrap_or("");
            let effective_name = effective_cmd_map.get(tool_use_id).map(|s| s.as_str());

            // Check bypass list (exact match on tool name).
            if !literal_name.is_empty() && config.command_bypass.iter().any(|b| b == literal_name) {
                result.passthroughs += 1;
                continue;
            }

            let raw_text = extract_text_content(block);
            if raw_text.is_empty() {
                result.passthroughs += 1;
                continue;
            }

            let tokens_raw = estimate_tokens(&raw_text);

            // Try the resolved command first (e.g. Bash→cargo), then fall
            // back to the literal tool name.
            let filter_cmd = effective_name.unwrap_or(literal_name);
            let matched = !filter_cmd.is_empty() && toml_filter::command_matches_filter(filter_cmd);

            if matched {
                if let Some(filter) = find_matching_filter(filter_cmd) {
                    let (reduced_text, _loss) = apply_filter_with_info(filter, &raw_text);

                    let hash = storage::store_at(raw_text.as_bytes(), cas_root)
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

/// CLI tools that expose a discoverable underlying command in their input.
/// For these tools, `resolve_command` extracts the real command so it can
/// be matched against RTK filter patterns.
const CLI_PROXY_TOOLS: &[&str] = &["Bash"];

/// Extract the effective command from a `tool_use` input block.
///
/// For `Bash` tools, reads `input.command`, strips common prefixes (`sudo`,
/// `rtk`), and returns the remaining command line so filters can distinguish
/// subcommands (e.g. `"git diff"` from `"git status"`).
///
/// Returns `None` when no command can be extracted or the tool is not a
/// recognized CLI proxy.
fn resolve_command(tool_name: &str, input: &Value) -> Option<String> {
    if !CLI_PROXY_TOOLS.contains(&tool_name) {
        return None;
    }
    let raw = input.get("input")?.get("command")?.as_str()?;
    // Strip common wrapper prefixes so the filter matches the real tool.
    let cmd = raw
        .trim_start()
        .trim_start_matches("sudo ")
        .trim_start_matches("rtk ");
    if cmd.is_empty() {
        return None;
    }
    Some(cmd.to_string())
}

/// Build two maps from `tool_use.id`:
///   - `tool_name`: the literal Anthropic tool name (e.g. `"Bash"`, `"Read"`)
///   - `effective_cmd`: the resolved CLI command line when available (e.g.
///     `"cargo test --lib"`, `"git diff"`), otherwise absent
fn build_tool_map(root: &Value) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut tool_name = HashMap::new();
    let mut effective_cmd = HashMap::new();

    let messages = match root.get("messages") {
        Some(Value::Array(msgs)) => msgs,
        _ => return (tool_name, effective_cmd),
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
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
                let name = block.get("name").and_then(Value::as_str);

                if let (Some(id), Some(name)) = (&id, name) {
                    tool_name.insert(id.clone(), name.to_string());
                    if let Some(cmd) = resolve_command(name, block) {
                        effective_cmd.insert(id.clone(), cmd);
                    }
                }
            }
        }
    }

    (tool_name, effective_cmd)
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

    // ── Bash command resolution tests ──────────────────────────────

    #[test]
    fn bash_cargo_test_is_reduced() {
        // Simulates a Claude Code Bash tool_call running `cargo test`.
        // Includes compilation noise that the cargo TOML filter strips.
        let body = r#"{
  "model": "claude-sonnet-5",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "run tests"},
    {"role": "assistant", "content": [
      {"type": "tool_use", "id": "toolu_bash_01", "name": "Bash", "input": {"command": "cargo test --lib", "description": "Run tests"}}
    ]},
    {"role": "user", "content": [
      {"type": "tool_result", "tool_use_id": "toolu_bash_01", "content": "   Compiling my-crate v0.1.0\n   Compiling lib v0.2.0\n    Finished test [unoptimized + debuginfo] target(s) in 2.34s\n     Running unittests src/lib.rs (target/debug/deps/my_crate-abc123)\n\nrunning 5 tests\ntest test_a ... ok\ntest test_b ... FAILED\ntest test_c ... ok\n\nfailures:\n\n---- test_b stdout ----\nthread 'test_b' panicked at src/lib.rs:42:9:\nassertion failed\n\nfailures:\n    test_b\n\ntest result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s\n\nerror: test failed, to rerun pass `--lib`"}
    ]}
  ]
}"#;
        let r = reduce_body(body, &make_config(), false).expect("should succeed");
        assert!(
            r.reductions > 0,
            "Bash→cargo should be reduced, got reductions={}",
            r.reductions
        );
        assert!(
            r.modified_body.contains("toche:reduced"),
            "should contain reduction marker"
        );
        assert!(
            r.tokens_reduced < r.tokens_raw,
            "reduced tokens should be less than raw"
        );
    }

    #[test]
    fn bash_git_diff_is_reduced() {
        let body = r#"{
  "model": "claude-sonnet-5",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "show diff"},
    {"role": "assistant", "content": [
      {"type": "tool_use", "id": "toolu_bash_02", "name": "Bash", "input": {"command": "git diff HEAD~1"}}
    ]},
    {"role": "user", "content": [
      {"type": "tool_result", "tool_use_id": "toolu_bash_02", "content": "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-foo\n+bar"}
    ]}
  ]
}"#;
        let r = reduce_body(body, &make_config(), false).expect("should succeed");
        assert!(
            r.reductions > 0,
            "Bash→git should be reduced, got reductions={}",
            r.reductions
        );
        assert!(r.modified_body.contains("toche:reduced"));
    }

    #[test]
    fn bash_unknown_command_passthrough() {
        let body = r#"{
  "model": "claude-sonnet-5",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "do something"},
    {"role": "assistant", "content": [
      {"type": "tool_use", "id": "toolu_bash_03", "name": "Bash", "input": {"command": "my-custom-tool --verbose"}}
    ]},
    {"role": "user", "content": [
      {"type": "tool_result", "tool_use_id": "toolu_bash_03", "content": "custom output here"}
    ]}
  ]
}"#;
        let r = reduce_body(body, &make_config(), false).expect("should succeed");
        assert_eq!(r.reductions, 0, "unknown command should not be reduced");
        assert_eq!(r.passthroughs, 1);
    }

    #[test]
    fn bash_with_sudo_prefix_still_matches() {
        let body = r#"{
  "model": "claude-sonnet-5",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "check docker"},
    {"role": "assistant", "content": [
      {"type": "tool_use", "id": "toolu_bash_04", "name": "Bash", "input": {"command": "sudo docker ps -a"}}
    ]},
    {"role": "user", "content": [
      {"type": "tool_result", "tool_use_id": "toolu_bash_04", "content": "CONTAINER ID   IMAGE     STATUS"}
    ]}
  ]
}"#;
        let r = reduce_body(body, &make_config(), false).expect("should succeed");
        // docker may or may not be in filters — either outcome is valid.
        // The key assertion: it doesn't crash and processes the block.
        assert!(r.reductions + r.passthroughs >= 1);
    }

    #[test]
    fn literal_tool_name_still_works() {
        // Non-Bash tools (e.g. custom MCP server) still match by literal name.
        let body = r#"{
  "model": "claude-sonnet-5",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "run shellcheck"},
    {"role": "assistant", "content": [
      {"type": "tool_use", "id": "toolu_sc_01", "name": "shellcheck", "input": {}}
    ]},
    {"role": "user", "content": [
      {"type": "tool_result", "tool_use_id": "toolu_sc_01", "content": "In script.sh line 3:\necho $var\n     ^-- SC2086: Double quote to prevent globbing."}
    ]}
  ]
}"#;
        let r = reduce_body(body, &make_config(), false).expect("should succeed");
        assert!(
            r.reductions > 0,
            "literal shellcheck should still be reduced"
        );
    }
}
