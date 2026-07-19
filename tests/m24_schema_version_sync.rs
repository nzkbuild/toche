use rusqlite::Connection;
use tempfile::TempDir;

use toche::continuity::checkpoint::{CheckpointDb, NewCheckpoint};
use toche::safe_cache::cache_db::{CacheDb, NewCacheEntry};

/// Simulate ledger writing version 11, then verify checkpoint opens.
#[test]
fn checkpoint_opens_after_ledger_writes_v11() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("ledger.db");

    // Simulate what the meter (ledger) does: insert version 11
    {
        let conn = Connection::open(&db_path).expect("open");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
            [],
        )
        .expect("create schema_version");
        conn.execute("INSERT INTO schema_version (version) VALUES (11)", [])
            .expect("insert v11");
    }

    // CheckpointDb should open without error
    let db = CheckpointDb::open(&db_path).expect("checkpoint opens after ledger writes v11");

    // Smoke test: insert and retrieve a checkpoint
    let id = db
        .insert(&NewCheckpoint {
            project_path: "/test/p".into(),
            task: "test task".into(),
            completed: "done".into(),
            changed_files: "src/main.rs".into(),
            verification: "passed".into(),
            open_risks: "none".into(),
            next_action: "commit".into(),
            facts_json: "{}".into(),
            model_assisted: false,
        })
        .expect("insert");
    let entry = db.get(id).expect("get").expect("entry exists");
    assert_eq!(entry.task, "test task");
}

/// Simulate ledger writing version 11, then verify cache opens.
#[test]
fn cache_opens_after_ledger_writes_v11() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("ledger.db");

    // Simulate what the meter (ledger) does: insert version 11
    {
        let conn = Connection::open(&db_path).expect("open");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
            [],
        )
        .expect("create schema_version");
        conn.execute("INSERT INTO schema_version (version) VALUES (11)", [])
            .expect("insert v11");
    }

    // CacheDb should open without error
    let db = CacheDb::open(&db_path).expect("cache opens after ledger writes v11");

    // Smoke test: insert and lookup a cache entry
    db.insert(&NewCacheEntry {
        project_path: "/test/p".into(),
        fingerprint: "a".repeat(64),
        workspace_fingerprint: "b".repeat(64),
        response_hash: "c".repeat(64),
        model: "claude-sonnet-5".into(),
        status: 200,
        tokens_input: 1000,
        tokens_output: 200,
    })
    .expect("insert");
    let entry = db
        .lookup("/test/p", &"a".repeat(64))
        .expect("lookup")
        .expect("entry exists");
    assert_eq!(entry.model, "claude-sonnet-5");
}

/// Verify the rejection guard: version > 11 should be rejected.
#[test]
fn cache_rejects_version_greater_than_11() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("ledger.db");

    {
        let conn = Connection::open(&db_path).expect("open");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
            [],
        )
        .expect("create schema_version");
        conn.execute("INSERT INTO schema_version (version) VALUES (99)", [])
            .expect("insert v99");
    }

    let result = CacheDb::open(&db_path);
    assert!(result.is_err());
    let err_msg = format!("{}", result.err().unwrap());
    assert!(
        err_msg.contains("99 > 11"),
        "expected rejection message, got: {}",
        err_msg
    );
}

/// Verify the rejection guard in checkpoint.
#[test]
fn checkpoint_rejects_version_greater_than_11() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("ledger.db");

    {
        let conn = Connection::open(&db_path).expect("open");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
            [],
        )
        .expect("create schema_version");
        conn.execute("INSERT INTO schema_version (version) VALUES (99)", [])
            .expect("insert v99");
    }

    let result = CheckpointDb::open(&db_path);
    assert!(result.is_err());
    let err_msg = format!("{}", result.err().unwrap());
    assert!(
        err_msg.contains("99 > 11"),
        "expected rejection message, got: {}",
        err_msg
    );
}

/// Verify all three modules create their tables unconditionally (no version gate).
/// Even with no schema_version rows (current_version = 0), open must succeed.
#[test]
fn opens_without_any_schema_version() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("ledger.db");

    // No schema_version table at all
    let db = CacheDb::open(&db_path).expect("cache opens with no schema_version");
    // Should be usable
    assert_eq!(db.count(None).expect("count"), 0);

    let db2 = CheckpointDb::open(&db_path).expect("checkpoint opens with no schema_version");
    let entries = db2.list("/nonexistent", 1).expect("list");
    assert!(entries.is_empty());
}
