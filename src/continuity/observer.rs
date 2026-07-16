use std::sync::Mutex;

/// Facts collected from observed API responses during a session.
#[derive(Debug, Clone, Default)]
pub struct SessionFacts {
    pub files_read: Vec<String>,
    pub files_written: Vec<String>,
    pub commands_run: Vec<String>,
    pub models_used: Vec<String>,
}

/// Global observer accumulator. The gateway pushes response body bytes through
/// `observe_response`, and CLI commands drain facts via `drain_facts`.
static OBSERVER: std::sync::LazyLock<Mutex<SessionFacts>> =
    std::sync::LazyLock::new(|| Mutex::new(SessionFacts::default()));

/// Feed a response body to the observer, extracting file/command facts.
/// Best-effort: if parsing fails, nothing is recorded.
pub fn observe_response(body_bytes: &[u8]) {
    let facts = extract_facts(body_bytes);
    if facts.is_empty() {
        return;
    }
    if let Ok(mut guard) = OBSERVER.lock() {
        for f in facts.files_read {
            if !guard.files_read.contains(&f) {
                guard.files_read.push(f);
            }
        }
        for f in facts.files_written {
            if !guard.files_written.contains(&f) {
                guard.files_written.push(f);
            }
        }
        for c in facts.commands_run {
            if !guard.commands_run.contains(&c) {
                guard.commands_run.push(c);
            }
        }
        for m in facts.models_used {
            if !guard.models_used.contains(&m) {
                guard.models_used.push(m);
            }
        }
    }
}

/// Drain accumulated facts and reset the observer.
pub fn drain_facts() -> SessionFacts {
    OBSERVER
        .lock()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or_default()
}

/// Parse a response body and extract file and command facts from tool_use blocks.
/// Handles both standard JSON responses and SSE streaming responses.
pub fn extract_facts(body_bytes: &[u8]) -> SessionFacts {
    if body_bytes.is_empty() {
        return SessionFacts::default();
    }

    let text = String::from_utf8_lossy(body_bytes);
    let mut facts = SessionFacts::default();

    if text.contains("event: ") || text.starts_with("data:") {
        extract_sse_facts(&text, &mut facts);
    } else {
        extract_json_facts(&text, &mut facts);
    }

    facts
}

fn extract_sse_facts(text: &str, facts: &mut SessionFacts) {
    for line in text.lines() {
        let data = if let Some(d) = line.strip_prefix("data: ") {
            d
        } else {
            continue;
        };
        if data == "[DONE]" {
            continue;
        }
        extract_tool_use(data, facts);
    }
}

fn extract_json_facts(text: &str, facts: &mut SessionFacts) {
    let root: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Extract model name
    if let Some(model) = root
        .get("message")
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str())
    {
        facts.models_used.push(model.to_string());
    }
    if let Some(model) = root.get("model").and_then(|v| v.as_str()) {
        if !facts.models_used.contains(&model.to_string()) {
            facts.models_used.push(model.to_string());
        }
    }

    // Extract tool_use blocks
    if let Some(blocks) = root.get("content").and_then(|v| v.as_array()) {
        for block in blocks {
            let typ = block
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if typ != "tool_use" {
                continue;
            }
            extract_tool_use_from_block(block, facts);
        }
    }
}

fn extract_tool_use(data: &str, facts: &mut SessionFacts) {
    let obj: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Check content_block_start wrapper
    if let Some(cb) = obj.get("content_block") {
        if let Some(typ) = cb.get("type").and_then(|v| v.as_str()) {
            if typ == "tool_use" {
                extract_tool_use_from_block(cb, facts);
            }
        }
        return;
    }

    // Direct tool_use block in content array
    extract_tool_use_from_block(&obj, facts);
}

fn extract_tool_use_from_block(block: &serde_json::Value, facts: &mut SessionFacts) {
    let name = block
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let input = block.get("input");

    match name {
        "read" | "Read" | "View" => {
            if let Some(path) = input
                .and_then(|i| i.get("file_path").or_else(|| i.get("filePath")))
                .and_then(|v| v.as_str())
            {
                facts.files_read.push(path.to_string());
            }
        }
        "write" | "Write" | "Edit" | "edit" => {
            if let Some(path) = input
                .and_then(|i| i.get("file_path").or_else(|| i.get("filePath")))
                .and_then(|v| v.as_str())
            {
                facts.files_written.push(path.to_string());
            }
        }
        "bash" | "Bash" | "PowerShell" | "Shell" => {
            if let Some(cmd) = input
                .and_then(|i| i.get("command"))
                .and_then(|v| v.as_str())
            {
                // Take first line of multi-line commands
                let first_line = cmd.lines().next().unwrap_or(cmd).to_string();
                facts.commands_run.push(first_line);
            }
        }
        _ => {}
    }
}

impl SessionFacts {
    fn is_empty(&self) -> bool {
        self.files_read.is_empty()
            && self.files_written.is_empty()
            && self.commands_run.is_empty()
            && self.models_used.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_file_reads_from_json() {
        let body = r#"{
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-5",
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "read", "input": {"file_path": "src/main.rs"}}
            ],
            "stop_reason": "tool_use"
        }"#;
        let facts = extract_facts(body.as_bytes());
        assert_eq!(facts.files_read, vec!["src/main.rs"]);
        assert_eq!(facts.models_used, vec!["claude-sonnet-5"]);
    }

    #[test]
    fn extracts_file_writes_from_json() {
        let body = r#"{
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "write", "input": {"file_path": "lib.rs", "content": "pub fn x() {}"}}
            ],
            "stop_reason": "tool_use"
        }"#;
        let facts = extract_facts(body.as_bytes());
        assert_eq!(facts.files_written, vec!["lib.rs"]);
    }

    #[test]
    fn extracts_commands_from_json() {
        let body = r#"{
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "bash", "input": {"command": "cargo test\ncargo build"}}
            ],
            "stop_reason": "tool_use"
        }"#;
        let facts = extract_facts(body.as_bytes());
        assert_eq!(facts.commands_run, vec!["cargo test"]);
    }

    #[test]
    fn extracts_from_sse() {
        let sse = "\
event: content_block_start
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"read\",\"input\":{\"file_path\":\"Cargo.toml\"}}}
";
        let facts = extract_facts(sse.as_bytes());
        assert_eq!(facts.files_read, vec!["Cargo.toml"]);
    }

    #[test]
    fn deduplicates_facts() {
        let body = r#"{"type":"message","role":"assistant","content":[
            {"type":"tool_use","id":"tu_1","name":"read","input":{"file_path":"src/a.rs"}},
            {"type":"tool_use","id":"tu_2","name":"read","input":{"file_path":"src/a.rs"}}
        ],"stop_reason":"end_turn"}"#;
        let facts = extract_facts(body.as_bytes());
        // extract_facts doesn't dedupe; merging via observe_response does
        assert_eq!(facts.files_read.len(), 2);
    }

    #[test]
    fn handles_empty_body() {
        let facts = extract_facts(b"");
        assert!(facts.is_empty());
    }

    #[test]
    fn handles_non_tool_response() {
        let body = r#"{"type":"message","role":"assistant","content":[{"type":"text","text":"Hello!"}],"stop_reason":"end_turn"}"#;
        let facts = extract_facts(body.as_bytes());
        assert!(facts.files_read.is_empty());
        assert!(facts.files_written.is_empty());
        assert!(facts.commands_run.is_empty());
    }

    #[test]
    fn drain_resets_observer() {
        // Check drain returns empty on fresh observer
        let facts = drain_facts();
        assert!(facts.files_read.is_empty());

        // Feed some data
        observe_response(b"empty");
        let facts = drain_facts();
        assert!(facts.files_read.is_empty());

        // Observer should be empty again after drain
        let facts2 = drain_facts();
        assert!(facts2.files_read.is_empty());
    }
}
