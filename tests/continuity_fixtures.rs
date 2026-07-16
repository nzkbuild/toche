use std::path::Path;

use toche::continuity::checkpoint::{CheckpointDb, NewCheckpoint};
use toche::continuity::observer;

#[test]
fn save_and_show_checkpoint() {
    let db = CheckpointDb::open(Path::new(":memory:")).expect("open db");

    let id = db
        .insert(&NewCheckpoint {
            project_path: "/test/project".into(),
            task: "Implement session continuity".into(),
            completed: "DB schema\nCLI scaffold".into(),
            changed_files: "src/continuity/checkpoint.rs\nsrc/cli/checkpoint.rs".into(),
            verification: String::new(),
            open_risks: String::new(),
            next_action: "Wire observer into gateway".into(),
            facts_json: "{}".into(),
            model_assisted: false,
        })
        .unwrap();

    assert!(id > 0);

    let latest = db
        .latest("/test/project")
        .unwrap()
        .expect("should exist");
    assert_eq!(latest.task, "Implement session continuity");
    assert_eq!(latest.completed, "DB schema\nCLI scaffold");
    assert_eq!(latest.next_action, "Wire observer into gateway");
    assert!(!latest.created_at.is_empty());
}

#[test]
fn list_respects_limit() {
    let db = CheckpointDb::open(Path::new(":memory:")).expect("open db");

    for i in 0..5 {
        db.insert(&NewCheckpoint {
            project_path: "/p".into(),
            task: format!("Task {}", i),
            completed: String::new(),
            changed_files: String::new(),
            verification: String::new(),
            open_risks: String::new(),
            next_action: String::new(),
            facts_json: "{}".into(),
            model_assisted: false,
        })
        .unwrap();
    }

    let entries = db.list("/p", 3).unwrap();
    assert_eq!(entries.len(), 3);
    // Most recent first (highest IDs)
    assert_eq!(entries[0].task, "Task 4");
}

#[test]
fn delete_removes_checkpoint() {
    let db = CheckpointDb::open(Path::new(":memory:")).expect("open db");

    let id = db
        .insert(&NewCheckpoint {
            project_path: "/p".into(),
            task: "Delete me".into(),
            completed: String::new(),
            changed_files: String::new(),
            verification: String::new(),
            open_risks: String::new(),
            next_action: String::new(),
            facts_json: "{}".into(),
            model_assisted: false,
        })
        .unwrap();

    assert!(db.delete(id).unwrap());
    assert!(db.latest("/p").unwrap().is_none());
}

#[test]
fn checkpoints_are_project_scoped() {
    let db = CheckpointDb::open(Path::new(":memory:")).expect("open db");

    db.insert(&NewCheckpoint {
        project_path: "/project-a".into(),
        task: "A task".into(),
        completed: String::new(),
        changed_files: String::new(),
        verification: String::new(),
        open_risks: String::new(),
        next_action: String::new(),
        facts_json: "{}".into(),
        model_assisted: false,
    })
    .unwrap();

    db.insert(&NewCheckpoint {
        project_path: "/project-b".into(),
        task: "B task".into(),
        completed: String::new(),
        changed_files: String::new(),
        verification: String::new(),
        open_risks: String::new(),
        next_action: String::new(),
        facts_json: "{}".into(),
        model_assisted: false,
    })
    .unwrap();

    let a_entries = db.list("/project-a", 10).unwrap();
    assert_eq!(a_entries.len(), 1);
    assert_eq!(a_entries[0].task, "A task");

    let b_entries = db.list("/project-b", 10).unwrap();
    assert_eq!(b_entries.len(), 1);
    assert_eq!(b_entries[0].task, "B task");
}

#[test]
fn model_assisted_flag_persists() {
    let db = CheckpointDb::open(Path::new(":memory:")).expect("open db");

    let id = db
        .insert(&NewCheckpoint {
            project_path: "/p".into(),
            task: "AI-generated summary".into(),
            completed: String::new(),
            changed_files: String::new(),
            verification: String::new(),
            open_risks: String::new(),
            next_action: String::new(),
            facts_json: "{}".into(),
            model_assisted: true,
        })
        .unwrap();

    let entry = db.get(id).unwrap().expect("should exist");
    assert!(entry.model_assisted);
}

#[test]
fn observer_extracts_file_reads_from_tool_use() {
    let body = r#"{
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-5",
        "content": [
            {"type": "tool_use", "id": "tu_1", "name": "read", "input": {"file_path": "src/main.rs"}},
            {"type": "tool_use", "id": "tu_2", "name": "read", "input": {"file_path": "src/lib.rs"}}
        ],
        "stop_reason": "tool_use"
    }"#;

    let facts = observer::extract_facts(body.as_bytes());
    assert_eq!(facts.files_read.len(), 2);
    assert!(facts.files_read.contains(&"src/main.rs".to_string()));
    assert!(facts.files_read.contains(&"src/lib.rs".to_string()));
    assert_eq!(facts.models_used, vec!["claude-sonnet-5"]);
}

#[test]
fn observer_extracts_commands_from_bash() {
    let body = r#"{
        "type": "message",
        "role": "assistant",
        "content": [
            {"type": "tool_use", "id": "tu_1", "name": "bash", "input": {"command": "cargo test --lib"}}
        ],
        "stop_reason": "tool_use"
    }"#;

    let facts = observer::extract_facts(body.as_bytes());
    assert_eq!(facts.commands_run, vec!["cargo test --lib"]);
}

#[test]
fn observer_handles_text_only_response() {
    let body = r#"{"type":"message","role":"assistant","content":[{"type":"text","text":"Hello!"}],"stop_reason":"end_turn"}"#;
    let facts = observer::extract_facts(body.as_bytes());
    assert!(facts.files_read.is_empty());
    assert!(facts.files_written.is_empty());
    assert!(facts.commands_run.is_empty());
}

#[test]
fn observer_handles_empty_body() {
    let facts = observer::extract_facts(b"");
    assert!(facts.files_read.is_empty());
    assert!(facts.commands_run.is_empty());
}

#[test]
fn observer_handles_sse_tool_use() {
    let sse = "\
event: content_block_start
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"write\",\"input\":{\"file_path\":\"test.txt\",\"content\":\"hello\"}}}
";
    let facts = observer::extract_facts(sse.as_bytes());
    assert_eq!(facts.files_written, vec!["test.txt"]);
}

#[test]
fn observer_truncates_multiline_commands() {
    let body = r#"{
        "type": "message",
        "role": "assistant",
        "content": [
            {"type": "tool_use", "id": "tu_1", "name": "bash", "input": {"command": "git commit -m 'msg'\ngit push\n"}}
        ],
        "stop_reason": "tool_use"
    }"#;

    let facts = observer::extract_facts(body.as_bytes());
    assert_eq!(facts.commands_run.len(), 1);
    assert_eq!(facts.commands_run[0], "git commit -m 'msg'");
}

#[test]
fn save_checkpoint_merges_facts() {
    // observer::drain_facts returns accumulated facts
    // Fresh observer should return empty
    let facts = observer::drain_facts();
    assert!(facts.files_read.is_empty());
    assert!(facts.files_written.is_empty());
}
