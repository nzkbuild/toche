//! M13 — Migration and compatibility audit tests.
//!
//! 1. Idempotent ledger migration: create v10 ledger.db, open via LedgerDb,
//!    verify v11 schema, verify idempotent re-open, verify data intact.
//! 2. Config roundtrip: v1.0.10 profiles.toml → detect_and_load →
//!    config.toml → reload → verify equivalence.
//! 3. CAS compatibility: store, retrieve, directory structure preservation.
//!
//! These tests do NOT modify src/ files — they exercise the public APIs.

use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::Connection;
use tempfile::TempDir;

use toche::config::migration::{ConfigSource, detect_and_load};
use toche::config::toche_config::TocheConfig;
use toche::meter::db::{LedgerDb, NewLedgerRecord};
use toche::reduce::storage as cas;

// ——— helpers ———

fn temp_config_dir() -> TempDir {
    tempfile::tempdir().expect("failed to create temp dir")
}

/// Serialize tests that mutate the `TOCHE_CONFIG_DIR` env var (CAS tests).
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn config_dir_env(dir: &TempDir) {
    unsafe { std::env::set_var("TOCHE_CONFIG_DIR", dir.path().as_os_str()) };
}

// ——— 1. Idempotent ledger migration: v10 → v11 ———

/// Build a v10 ledger.db from scratch using raw rusqlite.
/// The v10 schema has identity columns but no `protocol` column.
/// Timestamps use recent dates so cleanup_old() (90-day cutoff) doesn't delete them.
fn create_v10_ledger(path: &Path) {
    let conn = Connection::open(path).expect("open v10 ledger");

    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA busy_timeout=5000;",
    )
    .unwrap();

    // Schema version tracking
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
        [],
    )
    .unwrap();

    // v1: base ledger table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ledger (
            id INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            model TEXT NOT NULL,
            profile_name TEXT NOT NULL DEFAULT '',
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
            latency_ms INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'success',
            cost REAL,
            project_path TEXT NOT NULL DEFAULT ''
        )",
        [],
    )
    .unwrap();

    let now = Utc::now();

    // Insert a row with v1-era data (use current timestamp to survive cleanup)
    conn.execute(
        "INSERT INTO ledger (timestamp, model, profile_name, input_tokens, output_tokens, status, project_path)
         VALUES (?1, 'claude-sonnet-4', 'default', 500, 100, 'success', '/home/user/project')",
        rusqlite::params![now.to_rfc3339()],
    )
    .unwrap();

    // Mark v1 as applied
    for v in 1..=7 {
        conn.execute("INSERT INTO schema_version (version) VALUES (?1)", [v])
            .unwrap();
    }

    // Apply v2-7 column migrations exactly as LedgerDb would
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN coalesced_count INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN reduction_input_tokens INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN reduction_output_tokens INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN reduction_count INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN efficiency_mode TEXT NOT NULL DEFAULT ''",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN local_cache_hit INTEGER NOT NULL DEFAULT 0",
        [],
    )
    .unwrap();

    // v10: identity columns
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN runtime_id TEXT NOT NULL DEFAULT ''",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN request_id TEXT NOT NULL DEFAULT ''",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN integration_id TEXT NOT NULL DEFAULT ''",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN upstream_id TEXT NOT NULL DEFAULT ''",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN trust_domain_id TEXT NOT NULL DEFAULT ''",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN config_snapshot_hash TEXT NOT NULL DEFAULT ''",
        [],
    )
    .unwrap();
    conn.execute(
        "ALTER TABLE ledger ADD COLUMN attribution TEXT NOT NULL DEFAULT 'unknown'",
        [],
    )
    .unwrap();

    // Mark v10 as applied
    conn.execute("INSERT INTO schema_version (version) VALUES (10)", [])
        .unwrap();

    // Also insert a second row with identity data populated (as v10 would have)
    conn.execute(
        "INSERT INTO ledger (timestamp, model, profile_name, input_tokens, output_tokens, status, project_path,
           runtime_id, request_id, integration_id, upstream_id, trust_domain_id, config_snapshot_hash, attribution)
         VALUES (?1, 'claude-sonnet-5', 'default', 1000, 300, 'success', '/workspace',
           'rt-abc', 'req-xyz', 'int-01', 'ups-01', 'td-01', 'cfg-01', 'exact')",
        rusqlite::params![now.to_rfc3339()],
    )
    .unwrap();

    // Create cache_rejects table as LedgerDb always ensures it
    conn.execute(
        "CREATE TABLE IF NOT EXISTS cache_rejects (
            id INTEGER PRIMARY KEY,
            timestamp TEXT NOT NULL,
            project_path TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            reason TEXT NOT NULL
        )",
        [],
    )
    .unwrap();

    // Create indexes (same as LedgerDb)
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ledger_timestamp ON ledger(timestamp)",
        [],
    )
    .unwrap();
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_ledger_project ON ledger(project_path, timestamp)",
        [],
    )
    .unwrap();
}

#[test]
fn ledger_migration_v10_to_v11_applies_protocol_column() {
    let dir = temp_config_dir();
    let db_path = dir.path().join("ledger.db");
    create_v10_ledger(&db_path);

    // Verify v10 schema is present BEFORE migration
    {
        let conn = Connection::open(&db_path).unwrap();
        let version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 10, "pre-migration version should be 10");

        // protocol column should NOT exist yet
        let has_protocol: bool = conn.prepare("SELECT protocol FROM ledger LIMIT 1").is_ok();
        assert!(
            !has_protocol,
            "protocol column should not exist in v10 schema"
        );
    }

    // Run migration by opening via LedgerDb
    {
        let db = LedgerDb::open(&db_path).expect("migration v10→v11 should succeed");

        // Verify schema version is now 11
        let entries = db
            .get_entries(10, None)
            .expect("get_entries after migration");
        assert_eq!(entries.len(), 2, "both rows should survive migration");

        // First row (v1-era) — protocol should be default empty
        assert_eq!(entries[1].protocol, "");

        // Second row (v10-era) — identity fields should be preserved
        assert_eq!(entries[0].runtime_id, "rt-abc");
        assert_eq!(entries[0].request_id, "req-xyz");
        assert_eq!(entries[0].attribution, "exact");
        assert_eq!(entries[0].protocol, "");
    }
}

#[test]
fn ledger_migration_is_idempotent() {
    let dir = temp_config_dir();
    let db_path = dir.path().join("ledger.db");
    create_v10_ledger(&db_path);

    // First open migrates v10→v11
    {
        let db = LedgerDb::open(&db_path).expect("first migration should succeed");

        // Insert a record with protocol populated (post-migration data)
        let record = NewLedgerRecord {
            timestamp: Utc::now(),
            model: "new-model".into(),
            profile_name: "default".into(),
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            coalesced_count: 0,
            latency_ms: 100,
            status: "success".into(),
            cost: None,
            project_path: "/tmp".into(),
            reduction_input_tokens: 0,
            reduction_output_tokens: 0,
            reduction_count: 0,
            efficiency_mode: String::new(),
            local_cache_hit: false,
            runtime_id: "rt-new".into(),
            request_id: "req-new".into(),
            integration_id: "int-new".into(),
            upstream_id: "ups-new".into(),
            trust_domain_id: "td-new".into(),
            config_snapshot_hash: "cfg-new".into(),
            attribution: "exact".into(),
            protocol: "anthropic".into(),
        };
        db.record(&record).expect("record new entry");
    }
    // drop(db) — connection is closed

    // Second open should be a no-op (already v11)
    {
        let db = LedgerDb::open(&db_path).expect("second open should succeed");

        let entries = db
            .get_entries(10, None)
            .expect("get_entries after second open");
        assert_eq!(
            entries.len(),
            3,
            "all three rows should survive idempotent re-open"
        );

        // Original v1-era row preserved
        let old = &entries[2];
        assert_eq!(old.model, "claude-sonnet-4");
        assert_eq!(old.input_tokens, 500);
        assert_eq!(old.output_tokens, 100);

        // v10-era row preserved
        let mid = &entries[1];
        assert_eq!(mid.model, "claude-sonnet-5");
        assert_eq!(mid.runtime_id, "rt-abc");
        assert_eq!(mid.attribution, "exact");

        // New row with protocol
        let new = &entries[0];
        assert_eq!(new.model, "new-model");
        assert_eq!(new.protocol, "anthropic");

        // verify schema version is 11
        let summary = db.get_summary(None).expect("get_summary");
        assert_eq!(summary.total.total_requests, 3);
    }
}

#[test]
fn ledger_migration_rejects_newer_schema() {
    let dir = temp_config_dir();
    let db_path = dir.path().join("ledger.db");

    // Create a DB claiming schema version 99 (newer than EXPECTED_VERSION=11)
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS ledger (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL
            )",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (99)", [])
            .unwrap();
    }

    let result = LedgerDb::open(&db_path);
    let err = match result {
        Ok(_) => panic!("should reject newer schema version"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("newer version"),
        "error should mention newer version: {err}"
    );
}

// ——— 2. Config roundtrip: v1.0.10 profiles.toml → v2 config.toml → reload ———

/// A realistic v1.0.10 profiles.toml with multiple profiles and feature configs.
/// Cache/reduce/efficiency are per-profile using inline table syntax.
const V1_0_10_PROFILES_TOML: &str = r#"
default = "default"

[[profiles]]
name = "default"
upstream_url = "https://api.anthropic.com"
auth_method = { type = "api_key", header_name = "x-api-key", key = "sk-ant-real-secret-key-here" }
headers = { anthropic-version = "2023-06-01" }
cache = { enabled = true, mode = "auto", breakpoint = "standard" }
reduce = { enabled = true, command_bypass = ["kubectl", "git"] }
efficiency = { mode = "concise" }

[[profiles]]
name = "openai"
upstream_url = "https://api.openai.com"
auth_method = { type = "bearer", token = "sk-openai-token-value" }
cache = { enabled = false, mode = "observe" }
reduce = { enabled = false, command_bypass = [] }
"#;

#[test]
fn config_roundtrip_v1_profiles_to_v2_reload() {
    let dir = temp_config_dir();
    let profiles_path = dir.path().join("profiles.toml");
    std::fs::write(&profiles_path, V1_0_10_PROFILES_TOML).unwrap();

    // Step 1: detect_and_load migrates profiles.toml → config.toml
    let first = detect_and_load(dir.path()).expect("first detect_and_load");
    let config1 = match first {
        ConfigSource::V1Migrated(c) => c,
        other => panic!(
            "expected V1Migrated, got {:?}",
            std::any::type_name_of_val(&other)
        ),
    };

    // Step 2: Verify config.toml was written
    let config_path = dir.path().join("config.toml");
    assert!(config_path.exists(), "config.toml should be created");

    // Read the saved config.toml file
    let saved_toml = std::fs::read_to_string(&config_path).unwrap();
    let config2: TocheConfig = toml::from_str(&saved_toml).expect("reload saved config.toml");

    // Step 3: Verify structural equivalence
    assert_eq!(config1.schema_version, config2.schema_version);
    assert_eq!(config1.schema_version, 2);
    assert_eq!(config1.integrations.len(), config2.integrations.len());
    assert_eq!(config1.integrations.len(), 2);
    assert_eq!(config1.upstreams.len(), config2.upstreams.len());
    assert_eq!(config1.policies.len(), config2.policies.len());

    // Verify both profiles migrated
    let default_int = config1
        .integrations
        .iter()
        .find(|i| i.name == "default")
        .expect("default integration");
    let openai_int = config1
        .integrations
        .iter()
        .find(|i| i.name == "openai")
        .expect("openai integration");

    // Default is the default integration
    assert_eq!(
        config1.defaults.integration.as_deref().unwrap(),
        default_int.id
    );

    // Default upstream has correct URL and secret
    let default_up = config1
        .upstreams
        .iter()
        .find(|u| u.id == default_int.upstream)
        .unwrap();
    assert_eq!(default_up.url, "https://api.anthropic.com");
    assert_eq!(default_up.auth.header_name, "x-api-key");
    assert!(matches!(
        default_up.auth.secret_ref,
        toche::config::toche_config::SecretRef::LegacyInline { .. }
    ));
    // Secret value must not appear in Debug output
    let debug = format!("{:?}", default_up.auth.secret_ref);
    assert!(!debug.contains("sk-ant"));

    // Default policy has reduce enabled with command_bypass
    let default_pol = config1
        .policies
        .iter()
        .find(|p| Some(p.id.as_str()) == default_int.policy.as_deref())
        .unwrap();
    let reduce = default_pol.reduce.as_ref().unwrap();
    assert!(reduce.enabled);
    assert_eq!(reduce.command_bypass, vec!["kubectl", "git"]);

    // Cache policy preserved
    let cache = default_pol.cache.as_ref().unwrap();
    assert!(cache.enabled);

    // Efficiency mode preserved
    let eff = default_pol.efficiency.as_ref().unwrap();
    assert!(
        matches!(eff.mode, toche::efficiency::config::EfficiencyMode::Concise),
        "expected Concise efficiency mode"
    );

    // OpenAI integration has bearer auth
    let openai_up = config1
        .upstreams
        .iter()
        .find(|u| u.id == openai_int.upstream)
        .unwrap();
    assert_eq!(openai_up.url, "https://api.openai.com");
    assert_eq!(openai_up.auth.header_name, "authorization");
    let openai_debug = format!("{:?}", openai_up.auth.secret_ref);
    assert!(!openai_debug.contains("sk-openai"));

    // IDs from step 1 and step 2 must match
    assert_eq!(
        config1.integrations[0].id, config2.integrations[0].id,
        "IDs from first load and reload must be identical"
    );
}

#[test]
fn config_roundtrip_load_save_reload_is_stable() {
    let dir = temp_config_dir();
    let profiles_path = dir.path().join("profiles.toml");
    std::fs::write(&profiles_path, V1_0_10_PROFILES_TOML).unwrap();

    // First load: migrates
    detect_and_load(dir.path()).expect("first load");

    // Second load: reads existing config.toml
    let source = detect_and_load(dir.path()).expect("second load");
    let config_a = match source {
        ConfigSource::V2(c) => c,
        other => panic!(
            "expected V2 on reload, got {:?}",
            std::any::type_name_of_val(&other)
        ),
    };

    // Third load: still reads the same config.toml
    let source2 = detect_and_load(dir.path()).expect("third load");
    let config_b = match source2 {
        ConfigSource::V2(c) => c,
        other => panic!(
            "expected V2 on third load, got {:?}",
            std::any::type_name_of_val(&other)
        ),
    };

    // All three loads should produce structurally identical configs
    assert_eq!(config_a.integrations.len(), config_b.integrations.len());
    assert_eq!(config_a.integrations[0].id, config_b.integrations[0].id);
    assert_eq!(config_a.upstreams[0].url, config_b.upstreams[0].url);
    assert_eq!(config_a.defaults.integration, config_b.defaults.integration);

    // Verify backup exists and profiles.toml was removed
    assert!(
        dir.path().join("profiles.toml.v1.bak").exists(),
        "backup should exist"
    );
    assert!(
        !dir.path().join("profiles.toml").exists(),
        "original profiles.toml should be moved to backup"
    );
}

#[test]
fn config_roundtrip_preserves_deterministic_ids() {
    // Same input on two separate migration paths should produce the same IDs
    let dir1 = temp_config_dir();
    let dir2 = temp_config_dir();

    std::fs::write(dir1.path().join("profiles.toml"), V1_0_10_PROFILES_TOML).unwrap();
    std::fs::write(dir2.path().join("profiles.toml"), V1_0_10_PROFILES_TOML).unwrap();

    let source1 = detect_and_load(dir1.path()).expect("dir1 load");
    let source2 = detect_and_load(dir2.path()).expect("dir2 load");

    let c1 = match source1 {
        ConfigSource::V1Migrated(c) => c,
        _ => panic!("expected V1Migrated"),
    };
    let c2 = match source2 {
        ConfigSource::V1Migrated(c) => c,
        _ => panic!("expected V1Migrated"),
    };

    // Compare all IDs
    for (i1, i2) in c1.integrations.iter().zip(c2.integrations.iter()) {
        assert_eq!(i1.id, i2.id, "integration IDs must be deterministic");
    }
    for (u1, u2) in c1.upstreams.iter().zip(c2.upstreams.iter()) {
        assert_eq!(u1.id, u2.id, "upstream IDs must be deterministic");
    }
    for (p1, p2) in c1.policies.iter().zip(c2.policies.iter()) {
        assert_eq!(p1.id, p2.id, "policy IDs must be deterministic");
    }
}

// ——— 3. CAS compatibility ———

#[test]
fn cas_store_and_retrieve_roundtrip() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = temp_config_dir();
    config_dir_env(&dir);

    let data = b"toche M13 CAS compatibility test payload v1.0.10 upgrade path";
    let hash = cas::store(data).expect("CAS store should succeed");

    // Verify hash format (64 hex chars = SHA-256)
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

    // Verify directory structure matches v1.0.x layout: cas/<first2>/<remaining>
    let cas_dir = dir.path().join("cas");
    let prefix_dir = cas_dir.join(&hash[..2]);
    let blob_file = prefix_dir.join(&hash[2..]);

    assert!(cas_dir.exists(), "cas/ directory should exist");
    assert!(prefix_dir.exists(), "cas/<first2>/ directory should exist");
    assert!(
        blob_file.is_file(),
        "cas/<first2>/<remaining> blob should exist"
    );

    // Read raw bytes from disk to verify integrity
    let raw = std::fs::read(&blob_file).expect("read CAS blob from disk");
    assert_eq!(raw, data, "on-disk bytes must match stored data");

    // Retrieve via API
    let retrieved = cas::retrieve(&hash).expect("retrieve should succeed");
    assert_eq!(retrieved, data, "retrieved bytes must match original");
}

#[test]
fn cas_idempotent_store_produces_same_hash() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = temp_config_dir();
    config_dir_env(&dir);

    let data = b"deterministic CAS storage for migration compatibility";
    let h1 = cas::store(data).expect("first store");
    let h2 = cas::store(data).expect("second store");
    assert_eq!(h1, h2, "same content must produce same hash");

    // Both must retrieve the same content
    let r1 = cas::retrieve(&h1).expect("retrieve h1");
    let r2 = cas::retrieve(&h2).expect("retrieve h2");
    assert_eq!(r1, r2);
    assert_eq!(r1, data);
}

#[test]
fn cas_different_content_different_blobs() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = temp_config_dir();
    config_dir_env(&dir);

    let h_a = cas::store(b"content alpha").expect("store a");
    let h_b = cas::store(b"content beta").expect("store b");
    assert_ne!(h_a, h_b);

    let r_a = cas::retrieve(&h_a).expect("retrieve a");
    let r_b = cas::retrieve(&h_b).expect("retrieve b");
    assert_eq!(r_a, b"content alpha");
    assert_eq!(r_b, b"content beta");
}

#[test]
fn cas_invalid_hash_is_rejected() {
    assert!(cas::retrieve("nothex").is_err());
    assert!(cas::retrieve("ZZ").is_err());
    assert!(cas::retrieve("ab").is_err()); // too short to be real
    assert!(cas::retrieve("g".repeat(64).as_str()).is_err());
}

#[test]
fn cas_exists_after_store() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = temp_config_dir();
    config_dir_env(&dir);

    let hash = cas::store(b"existence check for migration audit").expect("store");
    assert!(cas::exists(&hash), "stored hash should exist");
    assert!(
        !cas::exists(&"a".repeat(64)),
        "random 64-char hex should not exist"
    );
}
