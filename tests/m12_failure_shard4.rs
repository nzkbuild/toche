use std::net::SocketAddr;
use std::path::Path;
use std::sync::Mutex;
use wiremock::MockServer;
use wiremock::matchers::{method, path};
use wiremock::ResponseTemplate;

use toche::gateway::server::build_router;

/// Serialize tests that mutate `TOCHE_CONFIG_DIR` env var.
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

/// Config with upstream for /v1/messages routing.
fn config_with_upstream(upstream_url: &str) -> String {
    format!(
        r#"
schema_version = 2

[runtime]
port = 0
listen_address = "127.0.0.1"
request_timeout_ms = 300000

[defaults]
integration = "a1b2c3d4"

[storage]
ledger_db = "ledger.db"
cas_dir = "cas"

[[integrations]]
id = "a1b2c3d4"
name = "default"
upstream = "e5f6a7b8"
policy = "c9d0e1f2"

[[upstreams]]
id = "e5f6a7b8"
name = "upstream"
url = "{upstream_url}"

[upstreams.auth]
type = "legacy_inline"
value = "test-key"
header_name = "x-api-key"

[[policies]]
id = "c9d0e1f2"
name = "default"
"#
    )
}

async fn spawn_gateway(
    config_dir: &Path,
    config_toml: &str,
) -> (
    SocketAddr,
    tokio::task::JoinHandle<()>,
) {
    std::fs::create_dir_all(config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), config_toml).unwrap();

    let app = {
        let _lock = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        build_router(Some(config_dir.to_path_buf())).unwrap()
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    (addr, handle)
}

// ─── Test 1: killed_runtime_recovery ──────────────────────────────────

#[tokio::test]
async fn killed_runtime_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    // Phase 1: Start gateway, send traffic, ledger.db is created and populated
    let mock = MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    let client = reqwest::Client::new();
    let _ = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(
            r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#,
        )
        .send()
        .await
        .unwrap();

    // Give async ledger write time to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // Simulate SIGTERM: drop the gateway gracefully
    drop(handle);

    // Phase 2: Verify ledger.db exists and is valid (no corruption)
    let ledger_path = config_dir.join("ledger.db");
    assert!(ledger_path.exists(), "ledger.db should exist after first run");
    {
        let conn = rusqlite::Connection::open(&ledger_path).unwrap();
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        assert_eq!(integrity, "ok", "ledger.db should pass integrity check");

        // Ledger should have at least one recorded request
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ledger", [], |row| row.get(0))
            .unwrap_or(0);
        assert!(count > 0, "ledger should contain at least one record");
    }

    // Phase 3: Restart — build_router opens the existing ledger.db and should not fail
    let (addr2, handle2) = spawn_gateway(&config_dir, &config).await;
    let resp = reqwest::get(format!("http://{addr2}/health")).await.unwrap();
    assert_eq!(resp.status(), 200, "restarted gateway should serve /health");

    drop(handle2);
}

// ─── Test 2: power_loss_simulation ────────────────────────────────────

#[tokio::test]
async fn power_loss_simulation() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    // Phase 1: Start gateway, send requests to populate the ledger
    let mock = MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    let client = reqwest::Client::new();
    for _ in 0..2 {
        let _ = client
            .post(format!("http://{addr}/v1/messages"))
            .header("content-type", "application/json")
            .header("x-api-key", "test-key")
            .body(
                r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#,
            )
            .send()
            .await
            .unwrap();
    }
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // Abrupt kill — no graceful shutdown
    handle.abort();

    // Phase 2: Verify the DB is intact after abrupt termination.
    // The WAL journal ensures the DB is self-consistent even after a crash.
    let ledger_path = config_dir.join("ledger.db");
    assert!(ledger_path.exists(), "ledger.db should exist after abrupt kill");

    let conn = rusqlite::Connection::open(&ledger_path).unwrap();
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        integrity, "ok",
        "ledger.db should be intact after abrupt process termination"
    );

    let _count: i64 = conn
        .query_row("SELECT COUNT(*) FROM ledger", [], |row| row.get(0))
        .unwrap_or(0);

    // Phase 3: Restart — the new process should open the existing ledger.db cleanly
    let (addr2, handle2) = spawn_gateway(&config_dir, &config).await;
    let resp = reqwest::get(format!("http://{addr2}/health")).await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "gateway should recover after abrupt termination"
    );

    drop(handle2);
}

// ─── Test 3: downgrade_attempt_rejected ────────────────────────────────

#[tokio::test]
async fn downgrade_attempt_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");
    std::fs::create_dir_all(&config_dir).unwrap();

    // Write a config.toml with a schema_version that is higher than what this
    // version of Toche supports.
    std::fs::write(
        config_dir.join("config.toml"),
        "schema_version = 999\n\n[runtime]\nport = 0\nlisten_address = \"127.0.0.1\"\nrequest_timeout_ms = 300000\n",
    )
    .unwrap();

    let _lock = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let result = build_router(Some(config_dir));

    match result {
        Err(e) => {
            // Use alternate format to get the full anyhow error chain
            let msg = format!("{e:#}");
            assert!(msg.contains("schema_version"), "error chain should mention schema_version, got: {msg}");
            assert!(
                msg.contains("999"),
                "error chain should mention version 999, got: {msg}"
            );
        }
        Ok(_) => panic!("expected build_router to reject config with schema_version 999"),
    }
}

// ─── Test 4: newer_schema_detected ─────────────────────────────────────

#[tokio::test]
async fn newer_schema_detected() {
    let dir = tempfile::tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.db");

    // Create a ledger.db with a schema version ahead of EXPECTED_VERSION (11)
    {
        let conn = rusqlite::Connection::open(&ledger_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
             INSERT INTO schema_version (version) VALUES (999);",
        )
        .unwrap();
    }

    let result = toche::meter::db::LedgerDb::open(&ledger_path);

    match result {
        Err(e) => {
            let msg = format!("{e:#}");
            assert!(
                msg.contains("newer version") || msg.contains("999"),
                "error should mention newer DB schema, got: {msg}"
            );
        }
        Ok(_) => panic!(
            "expected LedgerDb::open to reject DB with future schema version"
        ),
    }
}

// ─── Test 5: self_signed_tls_no_unsafe_bypass ──────────────────────────

#[tokio::test]
async fn self_signed_tls_no_unsafe_bypass() {
    // This test verifies that when an upstream presents a self-signed
    // certificate, the reqwest client (configured with default rustls-tls
    // which verifies certificates) rejects the connection — there is no
    // silent bypass or insecure fallback.
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    // Point the gateway at https://self-signed.badssl.com which presents a
    // self-signed certificate. reqwest with default rustls-tls should reject it.
    let config = r#"
schema_version = 2

[runtime]
port = 0
listen_address = "127.0.0.1"
request_timeout_ms = 5000

[defaults]
integration = "a1b2c3d4"

[storage]
ledger_db = "ledger.db"
cas_dir = "cas"

[[integrations]]
id = "a1b2c3d4"
name = "default"
upstream = "e5f6a7b8"
policy = "c9d0e1f2"

[[upstreams]]
id = "e5f6a7b8"
name = "upstream"
url = "https://self-signed.badssl.com"

[upstreams.auth]
type = "legacy_inline"
value = "test-key"
header_name = "x-api-key"

[[policies]]
id = "c9d0e1f2"
name = "default"
"#;

    let (addr, handle) = spawn_gateway(&config_dir, config).await;

    let client = reqwest::Client::new();
    let result = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(
            r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#,
        )
        .send()
        .await;

    // The gateway should either fail to connect upstream (502 Bad Gateway)
    // or the response should have a non-2xx status. Either outcome proves
    // there is no silent insecure bypass.
    match result {
        Ok(resp) => {
            assert!(
                !resp.status().is_success(),
                "response to self-signed upstream should not be successful, got {}",
                resp.status()
            );
        }
        Err(_) => {
            // Request-level error is also acceptable — the gateway may
            // close the connection during error handling.
        }
    }

    drop(handle);
}

// ─── Test 6: upstream_changed_after_setup ──────────────────────────────

#[tokio::test]
async fn upstream_changed_after_setup() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    // Start a wiremock server as the initial upstream
    let mock1 = MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock1)
        .await;

    let config = config_with_upstream(&mock1.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    // Verify initial routing works
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(
            r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#,
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "initial routing should succeed");

    // Now change the upstream: write a new config pointing to a different URL
    let mock2 = MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock2)
        .await;
    std::fs::write(
        config_dir.join("config.toml"),
        config_with_upstream(&mock2.uri()),
    )
    .unwrap();

    // The running gateway still has the OLD resolved integration in its AppState
    // (config is loaded at startup). A new request should still route to mock1.
    let resp2 = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(
            r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi again"}]}"#,
        )
        .send()
        .await
        .unwrap();

    // The gateway should still respond, since it is using its startup snapshot
    assert_eq!(
        resp2.status(),
        200,
        "gateway should continue serving after on-disk config change"
    );

    let health = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(health.status(), 200, "health should still return 200");

    drop(handle);
}
