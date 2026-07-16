use serde_json::Value;
use sha2::{Digest, Sha256};

/// Compute a SHA-256 fingerprint of a normalized JSON request body.
///
/// Strips `stream` and all `cache_control` fields from content blocks before
/// hashing, so that these wire-format and caching annotations don't
/// change the identity of the semantic request.
///
/// Falls back to a raw-body hash if the JSON parse or canonical serialization
/// fails.
pub fn compute(body: &str) -> String {
    match compute_canonical(body) {
        Ok(fp) => fp,
        Err(_) => {
            let mut hasher = Sha256::new();
            hasher.update(body.as_bytes());
            format!("{:x}", hasher.finalize())
        }
    }
}

fn compute_canonical(body: &str) -> Result<String, ()> {
    let mut root: Value = serde_json::from_str(body).map_err(|_| ())?;
    normalize(&mut root);
    let canonical = serde_json::to_string(&root).map_err(|_| ())?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

/// Remove fields that don't affect the LLM response from a JSON value.
fn normalize(root: &mut Value) {
    // Remove stream — it's a wire-format flag, not a semantic parameter.
    if let Value::Object(map) = root {
        map.remove("stream");
    }

    // Strip cache_control from system content blocks.
    if let Some(Value::Array(blocks)) = root.get_mut("system") {
        for block in blocks {
            strip_cache_control(block);
        }
    }

    // Strip cache_control from message content blocks.
    if let Some(Value::Array(messages)) = root.get_mut("messages") {
        for msg in messages {
            if let Some(Value::Array(content)) = msg.get_mut("content") {
                for block in content {
                    strip_cache_control(block);
                }
            }
        }
    }
}

fn strip_cache_control(block: &mut Value) {
    if let Value::Object(obj) = block {
        obj.remove("cache_control");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_bodies_same_fingerprint() {
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":[{"type":"text","text":"Hello"}]}]}"#;
        assert_eq!(compute(body), compute(body));
    }

    #[test]
    fn different_models_different_fingerprint() {
        let a = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[]}"#;
        let b = r#"{"model":"claude-opus-4.8","max_tokens":1024,"messages":[]}"#;
        assert_ne!(compute(a), compute(b));
    }

    #[test]
    fn different_temperature_different_fingerprint() {
        let a = r#"{"model":"claude-sonnet-5","max_tokens":1024,"temperature":0.7,"messages":[]}"#;
        let b = r#"{"model":"claude-sonnet-5","max_tokens":1024,"temperature":0.0,"messages":[]}"#;
        assert_ne!(compute(a), compute(b));
    }

    #[test]
    fn different_tools_different_fingerprint() {
        let a = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[],"tools":[{"name":"bash","description":"Run a command","input_schema":{"type":"object","properties":{"command":{"type":"string"}}}}]}"#;
        let b = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[],"tools":[{"name":"read","description":"Read a file","input_schema":{"type":"object","properties":{"path":{"type":"string"}}}}]}"#;
        assert_ne!(compute(a), compute(b));
    }

    #[test]
    fn stream_field_stripped() {
        let a = r#"{"model":"claude-sonnet-5","max_tokens":1024,"stream":true,"messages":[]}"#;
        let b = r#"{"model":"claude-sonnet-5","max_tokens":1024,"stream":false,"messages":[]}"#;
        assert_eq!(compute(a), compute(b));
    }

    #[test]
    fn cache_control_in_system_stripped() {
        let a = r#"{"model":"claude-sonnet-5","max_tokens":1024,"system":[{"type":"text","text":"You are helpful.","cache_control":{"type":"ephemeral"}}],"messages":[]}"#;
        let b = r#"{"model":"claude-sonnet-5","max_tokens":1024,"system":[{"type":"text","text":"You are helpful."}],"messages":[]}"#;
        assert_eq!(compute(a), compute(b));
    }

    #[test]
    fn cache_control_in_messages_stripped() {
        let a = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":[{"type":"text","text":"Hello","cache_control":{"type":"ephemeral"}}]}]}"#;
        let b = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":[{"type":"text","text":"Hello"}]}]}"#;
        assert_eq!(compute(a), compute(b));
    }

    #[test]
    fn system_as_string_preserved() {
        // system as plain string — no content blocks to strip, still works
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"system":"You are helpful.","messages":[]}"#;
        let result = compute(body);
        assert!(!result.is_empty());
    }

    #[test]
    fn fingerprint_is_64_hex_chars() {
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[]}"#;
        let fp = compute(body);
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn malformed_json_falls_back_to_raw_hash() {
        let fp = compute("not valid json at all");
        assert_eq!(fp.len(), 64);
    }

    #[test]
    fn fallback_fingerprint_is_repeatable() {
        let body = "{broken";
        let fp1 = compute(body);
        let fp2 = compute(body);
        assert_eq!(fp1, fp2);
    }
}
