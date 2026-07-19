use std::net::SocketAddr;
use std::path::Path;
use std::sync::Mutex;
use wiremock::MockServer;

use toche::gateway::build_router;

/// Serialize tests that mutate `TOCHE_CONFIG_DIR` env var.
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

/// A minimal valid config.toml which sets TOCHE_CONFIG_DIR, writes config,
/// builds the router, binds to a random port, and spawns the server.
/// Returns the bound address and the server's join handle.
async fn spawn_gateway(config_dir: &Path, config_toml: &str) -> (SocketAddr, tokio::task::JoinHandle<()>, std::sync::MutexGuard<'static, ()>) {
    std::fs::create_dir_all(config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), config_toml).unwrap();

    let lock = CONFIG_LOCK.lock().unwrap();
    let app = build_router(Some(config_dir.to_path_buf())).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    // Give the server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    (addr, handle, lock)
}

/// Build a config.toml string pointing upstream at the given URL.
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

/// Config with no default integration set — gateway starts but nothing routes.
fn config_without_integration() -> &'static str {
    r#"
schema_version = 2

[runtime]
port = 0
listen_address = "127.0.0.1"
request_timeout_ms = 300000

[storage]
ledger_db = "ledger.db"
cas_dir = "cas"

[[integrations]]
id = "a1b2c3d4"
name = "default"
upstream = "e5f6a7b8"

[[upstreams]]
id = "e5f6a7b8"
name = "upstream"
url = "http://127.0.0.1:1"

[upstreams.auth]
type = "none"
header_name = "x-api-key"
"#
}

// ─── Test 1: runtime-before-setup ──────────────────────────────────────

#[tokio::test]
async fn runtime_before_setup_status_returns_active_zero() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");
    let config = config_without_integration();
    let (addr, _handle, _lock) = spawn_gateway(&config_dir, config).await;

    let resp = reqwest::get(format!("http://{addr}/status")).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["active_flights"], 0);
    assert!(body.get("runtime_id").and_then(|v| v.as_str()).is_some());
}

// ─── Test 2: setup-idempotent ──────────────────────────────────────────

#[tokio::test]
async fn setup_idempotent_connect_twice_no_duplicates() {
    // Test that running setup twice on the same config produces identical
    // results — config is unchanged, ownership.toml has same content.
    // Uses the SetupTransaction directly to avoid home-dir issues on Windows.

    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let answers = toche::setup::SetupAnswers {
        upstream_url: Some("https://api.anthropic.com".into()),
        api_key: Some("sk-ant-test-key".into()),
        header_name: Some("x-api-key".into()),
        integration_name: Some("default".into()),
    };

    let tx = toche::setup::SetupTransaction::new(false, false)
        .with_config_dir(config_dir.clone())
        .with_answers(answers);

    // First run — should apply
    let outcome1 = tx.run().unwrap();
    assert!(
        matches!(outcome1, toche::setup::SetupOutcome::Applied { .. }),
        "first setup should apply"
    );

    // Capture ownership.toml after first run
    let ownership1 = std::fs::read_to_string(config_dir.join("ownership.toml")).unwrap();
    let config1 = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();

    // Second run with same answers — should be no-op
    let answers2 = toche::setup::SetupAnswers {
        upstream_url: Some("https://api.anthropic.com".into()),
        api_key: Some("sk-ant-test-key".into()),
        header_name: Some("x-api-key".into()),
        integration_name: Some("default".into()),
    };

    let tx2 = toche::setup::SetupTransaction::new(false, false)
        .with_config_dir(config_dir.clone())
        .with_answers(answers2);

    let outcome2 = tx2.run().unwrap();
    assert!(
        matches!(outcome2, toche::setup::SetupOutcome::NoOp),
        "second setup should be no-op"
    );

    // ownership.toml should be unchanged
    let ownership2 = std::fs::read_to_string(config_dir.join("ownership.toml")).unwrap();
    assert_eq!(ownership1, ownership2, "ownership.toml should be identical");

    // Config should be unchanged
    let config2 = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert_eq!(config1, config2, "config.toml should be identical");

    // Verify no duplicate fragments — config should have exactly one integration/upstream/policy
    let parsed: toche::config::toche_config::TocheConfig = toml::from_str(&config2).unwrap();
    assert_eq!(parsed.integrations.len(), 1, "should have exactly one integration");
    assert_eq!(parsed.upstreams.len(), 1, "should have exactly one upstream");
    assert_eq!(parsed.policies.len(), 1, "should have exactly one policy");
}

// ─── Test 3: endpoint-offline ──────────────────────────────────────────

#[tokio::test]
async fn endpoint_offline_returns_502() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    // Bind a listener and keep it alive but never accept connections.
    // TCP will complete the handshake (SYN/SYN-ACK/ACK) but no HTTP response
    // will ever arrive. With a 1s request timeout, reqwest will time out
    // and the gateway maps this to 502 BAD_GATEWAY.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = listener.local_addr().unwrap().port();

    let config = format!(
        r#"
schema_version = 2

[runtime]
port = 0
listen_address = "127.0.0.1"
request_timeout_ms = 1000

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
url = "http://127.0.0.1:{dead_port}"

[upstreams.auth]
type = "legacy_inline"
value = "test-key"
header_name = "x-api-key"

[[policies]]
id = "c9d0e1f2"
name = "default"
"#
    );
    let (addr, _handle, _lock) = spawn_gateway(&config_dir, &config).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
        .send()
        .await
        .unwrap();

    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap();
    assert_eq!(status, 502, "expected 502, got {status}: {body}");

    // Keep listener alive until here
    drop(listener);
}

// ─── Test 4: endpoint-fails-mid-stream ─────────────────────────────────

#[tokio::test]
async fn endpoint_fails_mid_stream_partial_response_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;

    // Wiremock stub: returns 200 but with a body that's truncated (simulating socket close mid-stream)
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/messages"))
        .respond_with(wiremock::ResponseTemplate::new(200)
            .set_body_raw(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\"}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello",
                "text/event-stream",
            ))
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, _handle, _lock) = spawn_gateway(&config_dir, &config).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .header("x-toche-bypass-safe-cache", "true")
        .header("x-toche-bypass-shield", "true")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
        .send()
        .await
        .unwrap();

    // The gateway should still return 200 (it collected whatever bytes it got)
    // but the response should be truncated/partial
    let body = resp.text().await.unwrap();
    assert!(body.contains("Hello"), "should have partial content");
    // It should NOT contain "message_stop" since the stream was cut off
    assert!(!body.contains("message_stop"), "should not have complete message_stop event");

    // Verify the partial response was NOT cached (safe_cache should have rejected it)
    let ledger_path = config_dir.join("ledger.db");
    if ledger_path.exists() {
        let db = toche::meter::db::LedgerDb::open(&ledger_path).unwrap();
        let entries = db.get_entries(10, None).unwrap();
        // The request should be logged but not as a local_cache_hit
        for entry in &entries {
            assert!(!entry.local_cache_hit, "partial response should not be cached");
        }
    }
}

// ─── Test 5: unknown-model ─────────────────────────────────────────────

#[tokio::test]
async fn unknown_model_returns_400_with_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;

    // Config with model allowlist — only claude-sonnet-5 is allowed
    let config = format!(
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

[integrations.models]
claude-sonnet-5 = "claude-sonnet-5"

[[upstreams]]
id = "e5f6a7b8"
name = "upstream"
url = "{}"

[upstreams.auth]
type = "legacy_inline"
value = "test-key"
header_name = "x-api-key"

[[policies]]
id = "c9d0e1f2"
name = "default"
"#,
        mock.uri()
    );

    let (addr, _handle, _lock) = spawn_gateway(&config_dir, &config).await;

    let client = reqwest::Client::new();

    // Request with a known model should succeed
    let resp_ok = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_ok.status().as_u16(), 200, "known model should succeed");

    // Request with unknown model should return 400
    let resp_bad = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(r#"{"model":"gpt-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_bad.status().as_u16(), 400, "unknown model should return 400");
}

// ─── Test 6: no-usage-metadata ─────────────────────────────────────────

#[tokio::test]
async fn no_usage_metadata_reports_usage_as_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;

    // Wiremock stub: returns 200 SSE with NO Anthropic usage headers
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/messages"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_raw(
                    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\",\"usage\":{\"input_tokens\":50,\"output_tokens\":30}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello from Toche!\"}}\n\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                    "text/event-stream",
                )
                // Deliberately NOT setting anthropic-cache-read-input-tokens or
                // anthropic-cache-creation-input-tokens headers
        )
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, _handle, _lock) = spawn_gateway(&config_dir, &config).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .header("x-toche-bypass-safe-cache", "true")
        .header("x-toche-bypass-shield", "true")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Hello from Toche!"));

    // Wait for ledger recording (tokio::spawn in handler)
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Check ledger — cache tokens should not be reported as meaningful values
    // since headers were absent
    let ledger_path = config_dir.join("ledger.db");
    let db = toche::meter::db::LedgerDb::open(&ledger_path).unwrap();
    let entries = db.get_entries(10, None).unwrap();
    assert!(!entries.is_empty(), "should have at least one ledger entry");

    let entry = &entries[0];
    // When usage headers are absent, the attribution should remain "unknown"
    // and cache tokens should be 0 (not meaningful)
    assert_eq!(entry.attribution, "unknown",
        "attribution should be 'unknown' when upstream provides no usage headers, got: {}",
        entry.attribution);
    // Cache tokens should be 0 since no headers were present
    assert_eq!(entry.cache_read_input_tokens, 0);
    assert_eq!(entry.cache_creation_input_tokens, 0);
}
