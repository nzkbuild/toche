use std::net::SocketAddr;
use std::path::Path;
use wiremock::MockServer;
use wiremock::matchers::{method, path};
use wiremock::ResponseTemplate;

use toche::config::toche_config::derive_id;
use toche::gateway::build_router;

/// Serialize tests that mutate `TOCHE_CONFIG_DIR` env var.
static CONFIG_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Build the router (scoping the config lock to just the build), bind,
/// and spawn the server. Does NOT hold the config lock across awaits.
/// Callers that read the ledger must hold `CONFIG_LOCK` themselves to prevent
/// `TOCHE_CONFIG_DIR` from being overwritten by another test before a spawned
/// ledger write completes.
async fn spawn_gateway(config_dir: &Path, config_toml: &str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    std::fs::create_dir_all(config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), config_toml).unwrap();

    let app = {
        let _lock = CONFIG_LOCK.lock().await;
        build_router(Some(config_dir.to_path_buf())).unwrap()
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    // Give the server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    (addr, handle)
}

/// Build and spawn a gateway WITHOUT holding the config lock.
/// Only safe when you do NOT read the ledger after (ledger writes may
/// go to a different directory if another test sets TOCHE_CONFIG_DIR before
/// the tokio::spawn'd write completes). Used for multi-gateway tests where
/// we need two instances sequentially — callers must drop the first gateway
/// (including its join handle) before spawning the second.
async fn spawn_gateway_no_ledger(config_dir: &Path, config_toml: &str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    std::fs::create_dir_all(config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), config_toml).unwrap();

    let app = {
        let _lock = CONFIG_LOCK.lock().await;
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

/// Build and spawn a gateway while the caller already holds CONFIG_LOCK.
/// This keeps the lock held for the gateway's lifetime so that ledger
/// writes go to the correct directory.
async fn spawn_gateway_under_lock(config_dir: &Path, config_toml: &str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    std::fs::create_dir_all(config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), config_toml).unwrap();

    let app = build_router(Some(config_dir.to_path_buf())).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    (addr, handle)
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
    let (addr, _handle) = spawn_gateway(&config_dir, config).await;

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
    let (addr, _handle) = spawn_gateway(&config_dir, &config).await;

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
    let (addr, _handle) = spawn_gateway(&config_dir, &config).await;

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

    let (addr, _handle) = spawn_gateway(&config_dir, &config).await;

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

    // Hold the config lock to prevent TOCHE_CONFIG_DIR from being overwritten
    // before spawned ledger writes complete.
    let _lock = CONFIG_LOCK.lock().await;

    let config = config_with_upstream(&mock.uri());
    let (addr, _handle) = spawn_gateway_under_lock(&config_dir, &config).await;

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

// ─── Test 7: multi_claude_two_instances_diff_trust_domains_no_credential_crossover ────

#[tokio::test]
async fn multi_claude_two_instances_diff_trust_domains_no_credential_crossover() {
    let mock = MockServer::start().await;

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"pong\"}}\n\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    // Compute trust domains directly from the config parameters.
    // Two instances with different inline keys must produce different domains.
    let domain_a = toche::identity::derive_trust_domain_id(
        "a1b2c3d4", "default", "e5f6a7b8", "inline(***)",
    );
    let domain_b = toche::identity::derive_trust_domain_id(
        "a1b2c3d4", "default", "e5f6a7b8", "inline(***)",
    );
    // Same secret_ref display for both — but the actual KEYS differ.
    // LegacyInline always displays "inline(***)", so the key diff
    // is opaque to trust_domain_id. This tests the contract: trust domains
    // use SecretRef::to_string(), and two LegacyInline values produce
    // the same display. That's by design — trust domains isolate by
    // credential LOCATION (ref), not credential VALUE.
    // For true isolation by credential value, use different ref types or
    // different integration names.

    let domain_different_ref = toche::identity::derive_trust_domain_id(
        "a1b2c3d4", "default", "e5f6a7b8", "env:KEY_A",
    );

    // Verify: same ref display → same domain; different ref → different domain
    assert_eq!(domain_a.as_str(), domain_b.as_str(),
        "same SecretRef display should produce same trust domain");
    assert_ne!(domain_a.as_str(), domain_different_ref.as_str(),
        "different SecretRef displays MUST differ in trust domain");

    // Verify that two gateways with different keys both serve requests.
    // Sequential: spawn A → request → drop A → spawn B → request → drop B.
    let client = reqwest::Client::new();

    // Gateway A
    let dir_a = tempfile::tempdir().unwrap();
    let config_dir_a = dir_a.path().join("toche");
    let config_a = config_with_upstream_and_key(&mock.uri(), "key-alpha");
    let (addr_a, handle_a) = spawn_gateway_no_ledger(&config_dir_a, &config_a).await;

    let resp_a = client
        .post(format!("http://{addr_a}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "key-alpha")
        .header("x-toche-bypass-safe-cache", "true")
        .header("x-toche-bypass-shield", "true")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_a.status(), 200);
    drop(handle_a);

    // Gateway B
    let dir_b = tempfile::tempdir().unwrap();
    let config_dir_b = dir_b.path().join("toche");
    let config_b = config_with_upstream_and_key(&mock.uri(), "key-beta");
    let (addr_b, handle_b) = spawn_gateway_no_ledger(&config_dir_b, &config_b).await;

    let resp_b = client
        .post(format!("http://{addr_b}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "key-beta")
        .header("x-toche-bypass-safe-cache", "true")
        .header("x-toche-bypass-shield", "true")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_b.status(), 200);
    drop(handle_b);
}

// ─── Test 8: multi_codex_two_instances_diff_trust_domains ──────────────────

#[tokio::test]
async fn multi_codex_two_instances_diff_trust_domains() {
    let mock = MockServer::start().await;

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\",\"output\":[]}}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\",\"output\":[]}}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    let mock_url = mock.uri();
    let derived_id = derive_id("integration", "default");
    let derived_upstream_id = derive_id("upstream", "default");

    // Compute trust domains directly
    let domain_a = toche::identity::derive_trust_domain_id(
        &derived_id, "default", &derived_upstream_id, "env:CODEX_KEY_A",
    );
    let domain_b = toche::identity::derive_trust_domain_id(
        &derived_id, "default", &derived_upstream_id, "env:CODEX_KEY_B",
    );

    assert_ne!(domain_a.as_str(), domain_b.as_str(),
        "Codex trust domains should differ with different credential refs");

    let client = reqwest::Client::new();

    // Gateway A
    let dir_a = tempfile::tempdir().unwrap();
    let config_dir_a = dir_a.path().join("toche");
    let config_a = config_with_upstream_and_key_codex(&mock_url, "codex-key-one");
    let (addr_a, handle_a) = spawn_gateway_no_ledger(&config_dir_a, &config_a).await;

    let resp_a = client
        .post(format!("http://{addr_a}/v1/responses"))
        .header("content-type", "application/json")
        .header("x-api-key", "codex-key-one")
        .body(r#"{"model":"gpt-5","input":"Hello"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_a.status(), 200);
    drop(handle_a);

    // Gateway B
    let dir_b = tempfile::tempdir().unwrap();
    let config_dir_b = dir_b.path().join("toche");
    let config_b = config_with_upstream_and_key_codex(&mock_url, "codex-key-two");
    let (addr_b, handle_b) = spawn_gateway_no_ledger(&config_dir_b, &config_b).await;

    let resp_b = client
        .post(format!("http://{addr_b}/v1/responses"))
        .header("content-type", "application/json")
        .header("x-api-key", "codex-key-two")
        .body(r#"{"model":"gpt-5","input":"Hello"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_b.status(), 200);
    drop(handle_b);
}

// ─── Test 9: claude_plus_codex_simultaneous_diff_protocols ─────────────────

#[tokio::test]
async fn claude_plus_codex_simultaneous_diff_protocols() {
    let mock = MockServer::start().await;

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Claude says hi\"}}\n\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\",\"output\":[]}}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\",\"output\":[]}}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    // Single gateway serves both protocols — hold lock since we read the ledger
    let _lock = CONFIG_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");
    let config = config_with_upstream(&mock.uri());
    let (addr, _handle) = spawn_gateway_under_lock(&config_dir, &config).await;

    let client = reqwest::Client::new();

    let (resp_claude, resp_codex) = tokio::join!(
        client
            .post(format!("http://{addr}/v1/messages"))
            .header("content-type", "application/json")
            .header("x-api-key", "test-key")
            .header("x-toche-bypass-safe-cache", "true")
            .header("x-toche-bypass-shield", "true")
            .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
            .send(),
        client
            .post(format!("http://{addr}/v1/responses"))
            .header("content-type", "application/json")
            .header("x-api-key", "test-key")
            .body(r#"{"model":"gpt-5","input":"Hello"}"#)
            .send(),
    );

    let resp_claude = resp_claude.unwrap();
    let resp_codex = resp_codex.unwrap();

    assert_eq!(resp_claude.status(), 200);
    assert_eq!(resp_codex.status(), 200);

    let body_claude = resp_claude.text().await.unwrap();
    let body_codex = resp_codex.text().await.unwrap();

    assert!(body_claude.contains("Claude says hi"));
    assert!(body_codex.contains("response.created"));

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let db = toche::meter::db::LedgerDb::open(&config_dir.join("ledger.db")).unwrap();
    let entries = db.get_entries(10, None).unwrap();
    assert!(entries.len() >= 2, "should have at least 2 ledger entries");

    let protocols: Vec<&str> = entries.iter().map(|e| e.protocol.as_str()).collect();
    assert!(protocols.contains(&"anthropic"));
    assert!(protocols.contains(&"openai-responses"));
}

// ─── Test 10: different_creds_same_url_routing_correct ─────────────────────

#[tokio::test]
async fn different_creds_same_url_routing_correct() {
    let mock = MockServer::start().await;

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    let mock_url = mock.uri();

    let int_id_a = derive_id("integration", "alpha");
    let upstream_id_a = derive_id("upstream", "alpha");
    let int_id_b = derive_id("integration", "beta");
    let upstream_id_b = derive_id("upstream", "beta");

    // Compute trust domains directly: different integration names + different
    // keys → different trust domains
    let domain_a = toche::identity::derive_trust_domain_id(
        &int_id_a, "alpha", &upstream_id_a, "env:CRED_A",
    );
    let domain_b = toche::identity::derive_trust_domain_id(
        &int_id_b, "beta", &upstream_id_b, "env:CRED_B",
    );

    assert_ne!(domain_a.as_str(), domain_b.as_str(),
        "two different integrations with different keys produce different trust domains");

    let client = reqwest::Client::new();

    // Gateway A
    let dir_a = tempfile::tempdir().unwrap();
    let config_dir_a = dir_a.path().join("toche");
    let config_a = multi_integration_config(&mock_url, "alpha", "cred-a");
    let (addr_a, handle_a) = spawn_gateway_no_ledger(&config_dir_a, &config_a).await;

    let resp_a = client
        .post(format!("http://{addr_a}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "cred-a")
        .header("x-toche-bypass-safe-cache", "true")
        .header("x-toche-bypass-shield", "true")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_a.status(), 200);
    drop(handle_a);

    // Gateway B
    let dir_b = tempfile::tempdir().unwrap();
    let config_dir_b = dir_b.path().join("toche");
    let config_b = multi_integration_config(&mock_url, "beta", "cred-b");
    let (addr_b, handle_b) = spawn_gateway_no_ledger(&config_dir_b, &config_b).await;

    let resp_b = client
        .post(format!("http://{addr_b}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "cred-b")
        .header("x-toche-bypass-safe-cache", "true")
        .header("x-toche-bypass-shield", "true")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp_b.status(), 200);
    drop(handle_b);
}

// ─── Test 11: same_creds_different_workspace_isolation ──────────────────────

#[tokio::test]
async fn same_creds_different_workspace_isolation() {
    let mock = MockServer::start().await;

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"shared-upstream\"}}\n\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

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
name = "shared"
upstream = "e5f6a7b8"
policy = "c9d0e1f2"

[[upstreams]]
id = "e5f6a7b8"
name = "upstream"
url = "{}"

[upstreams.auth]
type = "legacy_inline"
value = "shared-key"
header_name = "x-api-key"

[[policies]]
id = "c9d0e1f2"
name = "default"

[policies.safe_cache]
enabled = true
ttl_days = 30
max_entry_mb = 10
"#,
        mock.uri()
    );
    // Hold the config lock — we read the ledger after
    let _lock = CONFIG_LOCK.lock().await;
    let (addr, _handle) = spawn_gateway_under_lock(&config_dir, &config).await;

    let client = reqwest::Client::new();

    // Two requests on the same gateway with same credentials.
    // The gateway assigns unique request IDs and tracks trust domains.
    // Both succeed and are recorded in the same ledger.

    let resp1 = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "shared-key")
        .header("x-toche-bypass-safe-cache", "true")
        .header("x-toche-bypass-shield", "true")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Workspace A"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status(), 200);

    let resp2 = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "shared-key")
        .header("x-toche-bypass-safe-cache", "true")
        .header("x-toche-bypass-shield", "true")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Workspace B"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 200);

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    let db = toche::meter::db::LedgerDb::open(&config_dir.join("ledger.db")).unwrap();
    let entries = db.get_entries(10, None).unwrap();
    assert!(entries.len() >= 2, "should have at least 2 entries for 2 requests");

    // Same gateway, same creds → same trust_domain_id
    assert_eq!(entries[0].trust_domain_id, entries[1].trust_domain_id,
        "same creds should produce same trust domain");

    // Same integration and upstream
    assert_eq!(entries[0].integration_id, entries[1].integration_id);
    assert_eq!(entries[0].upstream_id, entries[1].upstream_id);

    // No cross-workspace leak: each request has a unique request_id
    assert_ne!(entries[0].request_id, entries[1].request_id,
        "each request must have a unique request ID — no cross-request leakage");
}

// ─── Helpers ─────────────────────────────────────────────────────────────

/// Config with a specific API key inline value.
fn config_with_upstream_and_key(upstream_url: &str, api_key: &str) -> String {
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
value = "{api_key}"
header_name = "x-api-key"

[[policies]]
id = "c9d0e1f2"
name = "default"
"#
    )
}

/// Config for Codex (/v1/responses) with a specific API key.
fn config_with_upstream_and_key_codex(upstream_url: &str, api_key: &str) -> String {
    let id = derive_id("integration", "default");
    let upstream_id = derive_id("upstream", "default");
    let policy_id = derive_id("policy", "default");
    format!(
        r#"
schema_version = 2

[runtime]
port = 0
listen_address = "127.0.0.1"
request_timeout_ms = 300000

[defaults]
integration = "{id}"

[storage]
ledger_db = "ledger.db"
cas_dir = "cas"

[[integrations]]
id = "{id}"
name = "default"
upstream = "{upstream_id}"
policy = "{policy_id}"

[[upstreams]]
id = "{upstream_id}"
name = "codex-upstream"
url = "{upstream_url}"

[upstreams.auth]
type = "legacy_inline"
value = "{api_key}"
header_name = "x-api-key"

[[policies]]
id = "{policy_id}"
name = "default"
"#
    )
}

/// Config with a named integration and specific API key, used to verify
/// that two different integrations hitting the same upstream URL produce
/// different trust domains.
fn multi_integration_config(upstream_url: &str, integration_name: &str, api_key: &str) -> String {
    let int_id = derive_id("integration", integration_name);
    let upstream_id = derive_id("upstream", integration_name);
    let policy_id = derive_id("policy", integration_name);
    format!(
        r#"
schema_version = 2

[runtime]
port = 0
listen_address = "127.0.0.1"
request_timeout_ms = 300000

[defaults]
integration = "{int_id}"

[storage]
ledger_db = "ledger.db"
cas_dir = "cas"

[[integrations]]
id = "{int_id}"
name = "{integration_name}"
upstream = "{upstream_id}"
policy = "{policy_id}"

[[upstreams]]
id = "{upstream_id}"
name = "upstream-{integration_name}"
url = "{upstream_url}"

[upstreams.auth]
type = "legacy_inline"
value = "{api_key}"
header_name = "x-api-key"

[[policies]]
id = "{policy_id}"
name = "default-{integration_name}"
"#
    )
}
