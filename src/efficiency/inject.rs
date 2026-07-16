//! Inject efficiency instructions into Anthropic Messages API request bodies.
//!
//! Appends an instruction text block to the last system content block.
//! If the system prompt is a plain string, it is converted to a content-block array.

use anyhow::Context;
use serde_json::Value;

/// The result of an efficiency injection.
pub struct InjectionResult {
    /// The modified JSON request body.
    pub modified_body: String,
    /// Estimated tokens added by the instruction text (chars / 4).
    pub tokens_added: u64,
}

/// Inject an efficiency instruction into the system prompt of the request body.
///
/// Returns the original body unchanged if `instruction` is `None` or if the
/// body has no `"system"` key. Fallible only on JSON parse errors.
pub fn inject_efficiency(body: &str, instruction: Option<&str>) -> anyhow::Result<InjectionResult> {
    let instruction = match instruction {
        None => {
            return Ok(InjectionResult {
                modified_body: body.to_string(),
                tokens_added: 0,
            });
        }
        Some(text) => text,
    };

    let mut root: Value = serde_json::from_str(body)
        .context("Failed to parse request body for efficiency injection")?;

    let block = serde_json::json!({"type": "text", "text": instruction});

    let mut did_modify = false;

    if let Some(system) = root.get_mut("system") {
        if let Some(blocks) = system.as_array_mut() {
            blocks.push(block);
            did_modify = true;
        } else if system.is_string() {
            let original_text = system.as_str().unwrap_or("").to_string();
            *system = serde_json::json!([
                {"type": "text", "text": original_text},
                block,
            ]);
            did_modify = true;
        }
    }

    if !did_modify {
        return Ok(InjectionResult {
            modified_body: body.to_string(),
            tokens_added: 0,
        });
    }

    let tokens_added = (instruction.len() as f64 / 4.0).ceil() as u64;

    let modified_body =
        serde_json::to_string(&root).context("Failed to serialize modified request body")?;

    Ok(InjectionResult {
        modified_body,
        tokens_added,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_instruction_returns_unchanged() {
        let body = r#"{"model": "claude-sonnet-5", "messages": []}"#;
        let result = inject_efficiency(body, None).unwrap();
        assert_eq!(result.modified_body, body);
        assert_eq!(result.tokens_added, 0);
    }

    #[test]
    fn no_system_key_returns_unchanged() {
        let body =
            r#"{"model": "claude-sonnet-5", "messages": [{"role": "user", "content": "hi"}]}"#;
        let result = inject_efficiency(body, Some("be concise")).unwrap();
        assert_eq!(result.modified_body, body);
        assert_eq!(result.tokens_added, 0);
    }

    #[test]
    fn system_as_array_gets_instruction_appended() {
        let body = r#"{"model": "claude-sonnet-5", "system": [{"type": "text", "text": "You are helpful."}], "messages": [{"role": "user", "content": "hi"}]}"#;
        let result = inject_efficiency(body, Some("BE CONCISE")).unwrap();
        assert_ne!(result.modified_body, body);
        let parsed: Value = serde_json::from_str(&result.modified_body).unwrap();
        let system = parsed["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert_eq!(system[1]["type"], "text");
        assert_eq!(system[1]["text"], "BE CONCISE");
        assert!(result.tokens_added > 0);
    }

    #[test]
    fn system_as_string_converted_to_array() {
        let body = r#"{"model": "claude-sonnet-5", "system": "You are helpful.", "messages": [{"role": "user", "content": "hi"}]}"#;
        let result = inject_efficiency(body, Some("BE CONCISE")).unwrap();
        assert_ne!(result.modified_body, body);
        let parsed: Value = serde_json::from_str(&result.modified_body).unwrap();
        let system = parsed["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert_eq!(system[0]["type"], "text");
        assert_eq!(system[0]["text"], "You are helpful.");
        assert_eq!(system[1]["type"], "text");
        assert_eq!(system[1]["text"], "BE CONCISE");
        assert!(result.tokens_added > 0);
    }

    #[test]
    fn deterministic_output() {
        let body = r#"{"model": "claude-sonnet-5", "system": [{"type": "text", "text": "You are helpful."}], "messages": []}"#;
        let r1 = inject_efficiency(body, Some("be concise")).unwrap();
        let r2 = inject_efficiency(body, Some("be concise")).unwrap();
        assert_eq!(r1.modified_body, r2.modified_body);
        assert_eq!(r1.tokens_added, r2.tokens_added);
    }

    #[test]
    fn invalid_json_errors() {
        let result = inject_efficiency("not json", Some("be concise"));
        assert!(result.is_err());
    }

    #[test]
    fn instruction_block_has_correct_shape() {
        let body = r#"{"model": "claude-sonnet-5", "system": [{"type": "text", "text": "orig"}], "messages": []}"#;
        let result = inject_efficiency(body, Some("injected")).unwrap();
        let parsed: Value = serde_json::from_str(&result.modified_body).unwrap();
        let last_block = &parsed["system"].as_array().unwrap()[1];
        assert_eq!(last_block["type"], "text");
        assert_eq!(last_block["text"], "injected");
        assert!(
            last_block.as_object().unwrap().len() == 2,
            "only type and text keys"
        );
    }
}
