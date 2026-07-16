use std::path::Path;

use toche::safe_cache::cache_db::{CacheDb, NewCacheEntry};
use toche::safe_cache::inspect;
use toche::safe_cache::workspace;

#[test]
fn text_response_is_cached_and_replayed() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let ws_fp = "w".repeat(64);
    let project = "test-project";
    let fingerprint = "a".repeat(64);

    // Verify no entry exists
    assert!(db.lookup(project, &fingerprint).unwrap().is_none());

    // Insert a text-only response
    db.insert(&NewCacheEntry {
        project_path: project.into(),
        fingerprint: fingerprint.clone(),
        workspace_fingerprint: ws_fp.clone(),
        response_hash: "text_response_hash".repeat(4),
        model: "claude-sonnet-5".into(),
        status: 200,
        tokens_input: 1000,
        tokens_output: 200,
    })
    .unwrap();

    // Lookup should return the entry
    let entry = db
        .lookup(project, &fingerprint)
        .unwrap()
        .expect("should find");
    assert_eq!(entry.workspace_fingerprint, ws_fp);
    assert_eq!(entry.hit_count, 1);

    // Touch to simulate cache hit
    db.touch(project, &fingerprint).unwrap();
    let entry = db
        .lookup(project, &fingerprint)
        .unwrap()
        .expect("should still find");
    assert_eq!(entry.hit_count, 2);
}

#[test]
fn tool_use_response_inspector_rejects() {
    let body = r#"{"type":"message","role":"assistant","content":[{"type":"tool_use","id":"tu_1","name":"read","input":{}}],"stop_reason":"tool_use"}"#;
    let verdict = inspect::inspect_response(body.as_bytes());
    assert!(!verdict.safe);
    assert!(verdict.tool_use_count > 0);
}

#[test]
fn workspace_fingerprint_mismatch_blocks_replay() {
    let current_ws = workspace::compute_workspace_fingerprint();
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let project = "test-project";
    let fingerprint = "b".repeat(64);

    // Insert with an intentionally wrong workspace fingerprint
    db.insert(&NewCacheEntry {
        project_path: project.into(),
        fingerprint: fingerprint.clone(),
        workspace_fingerprint: "deliberately_wrong_fingerprint".repeat(2),
        response_hash: "hash_old".repeat(56),
        model: "claude-sonnet-5".into(),
        status: 200,
        tokens_input: 100,
        tokens_output: 50,
    })
    .unwrap();

    let entry = db
        .lookup(project, &fingerprint)
        .unwrap()
        .expect("should exist");
    assert_ne!(
        entry.workspace_fingerprint, current_ws,
        "stored fingerprint should differ from current workspace fingerprint"
    );
}

#[test]
fn different_fingerprints_different_entries() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");

    let ws_fp = "w".repeat(64);
    let fp_model_a = "ma".repeat(32);
    let fp_model_b = "mb".repeat(32);

    db.insert(&NewCacheEntry {
        project_path: "project".into(),
        fingerprint: fp_model_a.clone(),
        workspace_fingerprint: ws_fp.clone(),
        response_hash: "hash_a".repeat(56),
        model: "claude-sonnet-5".into(),
        status: 200,
        tokens_input: 100,
        tokens_output: 50,
    })
    .unwrap();

    db.insert(&NewCacheEntry {
        project_path: "project".into(),
        fingerprint: fp_model_b.clone(),
        workspace_fingerprint: ws_fp.clone(),
        response_hash: "hash_b".repeat(56),
        model: "claude-opus-4-8".into(),
        status: 200,
        tokens_input: 200,
        tokens_output: 100,
    })
    .unwrap();

    let a = db.lookup("project", &fp_model_a).unwrap().expect("exists");
    let b = db.lookup("project", &fp_model_b).unwrap().expect("exists");
    assert_eq!(a.model, "claude-sonnet-5");
    assert_eq!(b.model, "claude-opus-4-8");
    assert_ne!(a.fingerprint, b.fingerprint);
}

#[test]
fn ttl_eviction_with_zero_days() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let ws_fp = "w".repeat(64);

    db.insert(&NewCacheEntry {
        project_path: "/p".into(),
        fingerprint: "e".repeat(64),
        workspace_fingerprint: ws_fp,
        response_hash: "r".repeat(64),
        model: "claude-sonnet-5".into(),
        status: 200,
        tokens_input: 100,
        tokens_output: 50,
    })
    .unwrap();

    // TTL=365 days should not evict just-inserted entries
    let removed = db.evict_expired(365).unwrap();
    assert_eq!(removed, 0);
}

#[test]
fn clear_removes_entries() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let ws_fp = "w".repeat(64);

    for i in 0..3 {
        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: format!("{}", i).repeat(32),
            workspace_fingerprint: ws_fp.clone(),
            response_hash: format!("r{}", i).repeat(31),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();
    }

    let removed = db.clear(Some("/p")).unwrap();
    assert_eq!(removed, 3);
    assert_eq!(db.count(Some("/p")).unwrap(), 0);
}

#[test]
fn empty_response_not_cached() {
    let verdict = inspect::inspect_response(b"");
    assert!(!verdict.safe);
    assert!(verdict.reason.contains("empty"));
}

#[test]
fn sse_response_safety_detection() {
    // SSE with tool_use in content_block_start — should be unsafe
    let sse_tool = "\
event: content_block_start
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"read\",\"input\":{}}}
";
    let verdict = inspect::inspect_response(sse_tool.as_bytes());
    assert!(!verdict.safe);
    assert!(verdict.tool_use_count > 0);

    // SSE with only text — should be safe
    let sse_text = "\
event: message_start
data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\"}}

event: content_block_start
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}

event: content_block_delta
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello!\"}}

event: message_delta
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}
";
    let verdict = inspect::inspect_response(sse_text.as_bytes());
    assert!(verdict.safe);
}

#[test]
fn inspect_sse_stop_reason_tool_use() {
    let sse = "\
event: message_delta
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null}}
";
    let verdict = inspect::inspect_response(sse.as_bytes());
    assert!(!verdict.safe);
    assert!(verdict.reason.contains("stop_reason"));
}
