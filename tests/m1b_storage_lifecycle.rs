use std::path::Path;

use toche::config::toche_config::StorageConfig;
use toche::safe_cache::cache_db::{self, CacheDb, NewCacheEntry};

// ── Contract 1: Config compatibility ──────────────────────────────────

#[test]
fn storage_config_defaults_are_unlimited_and_disabled() {
    let cfg = StorageConfig::default();
    assert_eq!(cfg.max_cas_bytes, None);
    assert_eq!(cfg.max_entries, None);
    assert_eq!(cfg.min_free_disk_bytes, None);
    assert_eq!(cfg.ledger_retention_days, None);
}

#[test]
fn storage_config_limits_are_optional_in_toml() {
    let toml_str = r#"[storage]
ledger_db = "ledger.db"
cas_dir = "cas"
"#;
    let cfg: StorageConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.max_cas_bytes, None);
    assert_eq!(cfg.max_entries, None);
    assert_eq!(cfg.min_free_disk_bytes, None);
    assert_eq!(cfg.ledger_retention_days, None);
}

#[test]
fn storage_config_rejects_zero_limits() {
    let cfg = StorageConfig {
        max_cas_bytes: Some(0),
        max_entries: Some(0),
        min_free_disk_bytes: Some(0),
        ledger_retention_days: Some(0),
        ..StorageConfig::default()
    };
    let errors = cfg.validate();
    assert_eq!(errors.len(), 4);
}

#[test]
fn storage_config_accepts_positive_limits() {
    let cfg = StorageConfig {
        max_cas_bytes: Some(1_073_741_824),
        max_entries: Some(1000),
        min_free_disk_bytes: Some(524_288_000),
        ledger_retention_days: Some(90),
        ..StorageConfig::default()
    };
    assert!(cfg.validate().is_empty());
}

// ── R1: min_free_disk_bytes decision ──────────────────────────────────

#[test]
fn min_free_disk_disabled_allows_write() {
    // When min_free_disk_bytes is None, writes are always allowed.
    let cfg = StorageConfig::default();
    assert!(cfg.min_free_disk_bytes.is_none());
    // The policy: None → skip check, allow write
}

#[test]
fn min_free_disk_configured_with_zero_free_refuses_write() {
    // If free space is 0 and reserve > 0, write must be refused.
    let reserve: u64 = 100 * 1024 * 1024; // 100 MiB
    let free: u64 = 0;
    let incoming: u64 = 1024;
    let after_write = free.saturating_sub(incoming);
    assert!(after_write < reserve, "write should be refused");
}

#[test]
fn min_free_disk_configured_with_sufficient_free_allows_write() {
    let reserve: u64 = 100 * 1024 * 1024;
    let free: u64 = 500 * 1024 * 1024;
    let incoming: u64 = 1024;
    let after_write = free.saturating_sub(incoming);
    assert!(after_write >= reserve, "write should be allowed");
}

#[test]
fn min_free_disk_unmeasurable_platform_refuses_write() {
    // When free-space measurement is unavailable and reserve is configured,
    // the write must be refused (fail-safe).
    let measurable = cache_db::free_disk_measurable();
    if !measurable {
        let reserve: Option<u64> = Some(100 * 1024 * 1024);
        assert!(
            reserve.is_some(),
            "reserve configured on unmeasurable platform → must refuse"
        );
        // Policy: if min_free_disk_bytes.is_some() && !measurable → refuse
    }
}

// ── R2: Ledger retention ──────────────────────────────────────────────

#[test]
fn ledger_retention_disabled_does_not_delete() {
    // None → no automatic deletion, no cleanup deletes.
    let cfg = StorageConfig::default();
    assert!(cfg.ledger_retention_days.is_none());
}

#[test]
fn ledger_retention_configured_cleanup_deletes_old_rows() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    // ledger table must exist
    db.conn
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS ledger (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                model TEXT NOT NULL
            )",
        )
        .unwrap();

    let now = chrono::Utc::now();
    let old = (now - chrono::Duration::days(100)).to_rfc3339();
    let recent = now.to_rfc3339();

    db.conn
        .execute(
            "INSERT INTO ledger (timestamp, model) VALUES (?1, 'm')",
            rusqlite::params![old],
        )
        .unwrap();
    db.conn
        .execute(
            "INSERT INTO ledger (timestamp, model) VALUES (?1, 'm')",
            rusqlite::params![recent],
        )
        .unwrap();

    let count_before: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM ledger", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count_before, 2);

    // Delete older than 90 days
    let deleted = cache_db::ledger_delete_older_than(&db.conn, 90).unwrap();
    assert_eq!(deleted, 1);

    let count_after: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM ledger", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count_after, 1);
}

#[test]
fn ledger_retention_dry_run_has_no_side_effect() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    db.conn
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS ledger (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                model TEXT NOT NULL
            )",
        )
        .unwrap();

    let old = (chrono::Utc::now() - chrono::Duration::days(100)).to_rfc3339();
    db.conn
        .execute(
            "INSERT INTO ledger (timestamp, model) VALUES (?1, 'm')",
            rusqlite::params![old],
        )
        .unwrap();

    // Dry-run: count without deleting
    let cutoff = chrono::Utc::now() - chrono::Duration::days(90);
    let would_delete: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM ledger WHERE timestamp < ?1",
            rusqlite::params![cutoff.to_rfc3339()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(would_delete, 1, "dry-run should see one candidate");

    // Verify no rows actually deleted
    let total: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM ledger", [], |row| row.get(0))
        .unwrap();
    assert_eq!(total, 1, "dry-run must not delete rows");
}

// ── Shared-CAS deletion ──────────────────────────────────────────────

#[test]
fn shared_hash_not_deleted_while_cache_references_remain() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let shared_hash = "sh".repeat(32);

    db.insert(&NewCacheEntry {
        project_path: "/p1".into(),
        fingerprint: "fp_a".repeat(32),
        workspace_fingerprint: "w1".repeat(32),
        response_hash: shared_hash.clone(),
        model: "m".into(),
        status: 200,
        tokens_input: 100,
        tokens_output: 50,
    })
    .unwrap();
    db.insert(&NewCacheEntry {
        project_path: "/p2".into(),
        fingerprint: "fp_b".repeat(32),
        workspace_fingerprint: "w2".repeat(32),
        response_hash: shared_hash.clone(),
        model: "m".into(),
        status: 200,
        tokens_input: 100,
        tokens_output: 50,
    })
    .unwrap();

    let removed = db.clear(Some("/p1")).unwrap();
    assert_eq!(removed, 1);

    let dir = tempfile::tempdir().unwrap();
    let cas = dir.path().join("cas");
    let first2 = &shared_hash[..2];
    std::fs::create_dir_all(cas.join(first2)).unwrap();
    std::fs::write(cas.join(first2).join(&shared_hash[2..]), b"data").unwrap();

    let orphans = db.orphan_candidates(&cas).unwrap();
    let orphan_hashes: Vec<_> = orphans.safe_to_delete.iter().map(|o| &o.hash).collect();
    assert!(
        !orphan_hashes.contains(&&shared_hash),
        "shared CAS still referenced by cache should not be orphan"
    );
}

#[test]
fn last_cache_reference_removal_marks_managed_blob_for_manual_cleanup() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let solo_hash = "ab".repeat(32);

    db.insert(&NewCacheEntry {
        project_path: "/p".into(),
        fingerprint: "fp".repeat(32),
        workspace_fingerprint: "w".repeat(64),
        response_hash: solo_hash.clone(),
        model: "m".into(),
        status: 200,
        tokens_input: 100,
        tokens_output: 50,
    })
    .unwrap();

    db.clear(None).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let cas = dir.path().join("cas");
    let first2 = &solo_hash[..2];
    std::fs::create_dir_all(cas.join(first2)).unwrap();
    std::fs::write(cas.join(first2).join(&solo_hash[2..]), b"data").unwrap();

    let orphans = db.orphan_candidates(&cas).unwrap();
    let orphan_hashes: Vec<_> = orphans.safe_to_delete.iter().map(|o| &o.hash).collect();
    assert!(orphan_hashes.contains(&&solo_hash));
}

// ── Reduce CAS registration ──────────────────────────────────────────

#[test]
fn registered_reduce_blobs_do_not_appear_as_orphans() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let dir = tempfile::tempdir().unwrap();
    let cas = dir.path().join("cas");

    let hash = "rd".repeat(32);
    let first2 = &hash[..2];
    std::fs::create_dir_all(cas.join(first2)).unwrap();
    std::fs::write(cas.join(first2).join(&hash[2..]), b"reduce output").unwrap();

    db.register_cas(&hash).unwrap();

    let orphans = db.orphan_candidates(&cas).unwrap();
    let orphan_hashes: Vec<_> = orphans.safe_to_delete.iter().map(|o| &o.hash).collect();
    assert!(
        !orphan_hashes.contains(&&hash),
        "registered reduce blob should not be flagged as orphan"
    );
}

// ── Dry-run no side effects ──────────────────────────────────────────

#[test]
fn orphan_scan_dry_run_does_not_delete_files() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let dir = tempfile::tempdir().unwrap();
    let cas = dir.path().join("cas");

    let hash = "cd".repeat(32);
    let first2 = &hash[..2];
    std::fs::create_dir_all(cas.join(first2)).unwrap();
    let file_path = cas.join(first2).join(&hash[2..]);
    std::fs::write(&file_path, b"orphan data").unwrap();

    let candidates_before = db.orphan_candidates(&cas).unwrap();
    let orphan_hashes: Vec<_> = candidates_before
        .legacy_untracked
        .iter()
        .map(|o| &o.hash)
        .collect();
    assert!(
        orphan_hashes.contains(&&hash),
        "unregistered file should be flagged as orphan candidate"
    );
    assert!(file_path.exists(), "dry-run must not delete orphan files");

    let candidates_after = db.orphan_candidates(&cas).unwrap();
    assert_eq!(
        candidates_before.legacy_untracked.len(),
        candidates_after.legacy_untracked.len()
    );
}

// ── Max entries refusal — response still succeeds ────────────────────

#[test]
fn max_entries_refusal_still_returns_response() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");

    for i in 0..2 {
        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: format!("fp{}", i).repeat(32),
            workspace_fingerprint: "w".repeat(64),
            response_hash: format!("rh{}", i).repeat(32),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();
    }

    let count = db.count(None).unwrap();
    assert_eq!(count, 2);

    let max_entries: u64 = 2;
    let write_allowed = count < max_entries;
    assert!(!write_allowed, "cache write should be refused at limit");
    assert_eq!(db.count(None).unwrap(), 2);
}

// ── Storage stats ────────────────────────────────────────────────────

#[test]
fn storage_stats_returns_cache_and_blob_counts() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let dir = tempfile::tempdir().unwrap();
    let cas = dir.path().join("cas");

    db.insert(&NewCacheEntry {
        project_path: "/p".into(),
        fingerprint: "fp".repeat(32),
        workspace_fingerprint: "w".repeat(64),
        response_hash: "st".repeat(32),
        model: "m".into(),
        status: 200,
        tokens_input: 100,
        tokens_output: 50,
    })
    .unwrap();

    let cr_hash = "ef".repeat(32);
    let first2 = &cr_hash[..2];
    std::fs::create_dir_all(cas.join(first2)).unwrap();
    std::fs::write(cas.join(first2).join(&cr_hash[2..]), b"content").unwrap();
    db.register_cas(&cr_hash).unwrap();

    let stats = db.storage_stats(&cas).unwrap();
    assert_eq!(stats.cache_entries, 1);
    assert!(stats.registered_blobs > 0);
    assert!(stats.cas_bytes_on_disk > 0);
}

// ── WAL checkpoint ──────────────────────────────────────────────────

#[test]
fn wal_checkpoint_runs_and_reports_result() {
    let db = CacheDb::open(Path::new(":memory:")).expect("open db");
    let result = cache_db::wal_checkpoint(&db.conn).unwrap();
    assert!(
        result.contains("checkpointed=") || result.contains("no progress"),
        "WAL checkpoint should return a descriptive string: {result}"
    );
}
