use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::{method, path};

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
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
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

fn claude_body(model: &str, content: &str) -> String {
    format!(
        r#"{{"model":"{model}","max_tokens":10,"messages":[{{"role":"user","content":"{content}"}}]}}"#
    )
}

fn codex_body(model: &str, input: &str) -> String {
    format!(r#"{{"model":"{model}","input":"{input}"}}"#)
}

// ─── Test 1: unknown_headers_forwarded ──────────────────────────────────

#[tokio::test]
async fn unknown_headers_forwarded() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("x-custom-foo", "bar-value")
                .append_header("x-upstream-id", "baz-42")
                .set_body_raw(
                    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                    "text/event-stream",
                ),
        )
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(claude_body("claude-sonnet-5", "Hi"))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "gateway should serve 200 even when upstream sends unknown response headers"
    );
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("message_stop"),
        "response body should contain expected SSE data"
    );

    // Health check still works
    let health = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(health.status(), 200);

    drop(handle);
}

// ─── Test 2: upstream_rejects_toche_headers ──────────────────────────────

#[tokio::test]
async fn upstream_rejects_toche_headers() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;

    // Record whether Toche headers were received by the upstream
    let received_headers = Arc::new(Mutex::new(Vec::new()));
    let received_headers_clone = received_headers.clone();

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |req: &wiremock::Request| {
            // Record which toche- headers the upstream saw
            let mut toche_headers = Vec::new();
            for (name, _) in &req.headers {
                let lower = name.as_str().to_lowercase();
                if lower.starts_with("x-toche") || lower == "x-request-id" {
                    toche_headers.push(name.to_string());
                }
            }
            if let Ok(mut h) = received_headers_clone.lock() {
                *h = toche_headers;
            }

            // Reject with 400 — upstream doesn't like these headers
            ResponseTemplate::new(400).set_body_string(
                r#"{"error":{"type":"invalid_request_error","message":"unknown header"}}"#,
            )
        })
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .header("x-toche-bypass", "true")
        .header("x-request-id", "req-12345")
        .body(claude_body("claude-sonnet-5", "test"))
        .send()
        .await
        .unwrap();

    // Gateway wraps upstream response body in SSE; HTTP status is still 200
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("invalid_request_error"),
        "gateway should relay upstream error body, got: {body}"
    );

    // Verify the upstream received the Toche headers (gateway forwarded them)
    let has_toche_bypass = {
        let h = received_headers.lock().unwrap();
        h.iter().any(|n| n.to_lowercase() == "x-toche-bypass")
    };
    assert!(
        has_toche_bypass,
        "upstream should receive x-toche-bypass header"
    );

    // Health check still works after 400 response handling
    let health = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(health.status(), 200);

    drop(handle);
}

// ─── Test 3: binary_content_passthrough ─────────────────────────────────

#[tokio::test]
async fn binary_content_passthrough() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;

    // Capture the forwarded body
    let forwarded_body = Arc::new(Mutex::new(String::new()));
    let forwarded_body_clone = forwarded_body.clone();

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |req: &wiremock::Request| {
            if let Ok(mut b) = forwarded_body_clone.lock() {
                *b = String::from_utf8_lossy(&req.body).to_string();
            }
            ResponseTemplate::new(200).set_body_raw(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            )
        })
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    // Body with non-ASCII Unicode, multi-byte sequences, RTL markers, and binary-like content
    // Avoid raw \0 (null byte) which is invalid in JSON strings.
    let exotic_content = "Hello \u{00E9}\u{00F1}\u{2603}\u{1F600} — em-dash \u{2014} \u{200F}RTL\u{200E} \u{FEFF}BOM\u{FEFF} \u{FFFD}replacement \u{00A0}nbsp\u{00A0} \u{061C}ALM\u{061C}";
    let body = claude_body("claude-sonnet-5", exotic_content);

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(body.clone())
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "gateway should handle exotic UTF-8 content"
    );

    // Verify the upstream received the exact body content (no corruption in forwarding)
    let fwd = forwarded_body.lock().unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&fwd).expect("forwarded body should be valid JSON");
    let msg_content = parsed["messages"][0]["content"]
        .as_str()
        .expect("content should be a string");
    assert!(
        msg_content.contains("\u{00E9}"),
        "forwarded body should contain é, got: {msg_content}"
    );
    assert!(
        msg_content.contains("\u{2603}"),
        "forwarded body should contain snowman, got: {msg_content}"
    );
    assert!(
        msg_content.contains("\u{1F600}"),
        "forwarded body should contain grin emoji, got: {msg_content}"
    );
    assert!(
        msg_content.contains("\u{200F}"),
        "forwarded body should contain RTL mark, got: {msg_content}"
    );

    drop(handle);
}

// ─── Test 4: already_reduced_content ─────────────────────────────────────

#[tokio::test]
async fn already_reduced_content() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;

    // Capture the body the gateway forwards to upstream (after reduce pass)
    let forwarded_body = Arc::new(Mutex::new(String::new()));
    let forwarded_body_clone = forwarded_body.clone();

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |req: &wiremock::Request| {
            if let Ok(mut b) = forwarded_body_clone.lock() {
                *b = String::from_utf8_lossy(&req.body).to_string();
            }
            ResponseTemplate::new(200).set_body_raw(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            )
        })
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    // Already minimal content — a single short message with no tool results
    let body =
        r#"{"model":"claude-sonnet-5","max_tokens":5,"messages":[{"role":"user","content":"Hi"}]}"#;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(body)
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        200,
        "reduce pass should not crash on minimal content"
    );
    let response_text = resp.text().await.unwrap();
    assert!(
        response_text.contains("message_stop"),
        "response should contain SSE data"
    );

    // The forwarded body should still be valid JSON (reduce didn't corrupt it)
    let fwd = forwarded_body.lock().unwrap();
    let _: serde_json::Value =
        serde_json::from_str(&fwd).expect("forwarded body after reduce should be valid JSON");

    drop(handle);
}

// ─── Test 5: multi_claude_concurrent_flights ──────────────────────────────

#[tokio::test]
async fn multi_claude_concurrent_flights() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;

    // Use a counter to verify upstream call count
    let upstream_hits = Arc::new(AtomicU64::new(0));
    let upstream_hits_clone = upstream_hits.clone();

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |_: &wiremock::Request| {
            let n = upstream_hits_clone.fetch_add(1, Ordering::SeqCst) + 1;
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(400))
                .set_body_raw(
                    format!(
                        "event: message_stop\ndata: {{\"type\":\"message_stop\",\"call\":{n}}}\n"
                    ),
                    "text/event-stream",
                )
        })
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    // Same body for both requests — identical fingerprint, non-streaming
    let body1 = claude_body("claude-sonnet-5", "Say hello");
    let body2 = body1.clone();

    let client = reqwest::Client::new();
    let url = format!("http://{addr}/v1/messages");

    // Fire both concurrently
    let (resp_a, resp_b) = tokio::join!(
        client
            .post(&url)
            .header("content-type", "application/json")
            .header("x-api-key", "test-key")
            .body(body1)
            .send(),
        client
            .post(&url)
            .header("content-type", "application/json")
            .header("x-api-key", "test-key")
            .body(body2)
            .send(),
    );

    let resp_a = resp_a.unwrap();
    let resp_b = resp_b.unwrap();

    assert_eq!(
        resp_a.status(),
        200,
        "first concurrent claude request should succeed"
    );
    assert_eq!(
        resp_b.status(),
        200,
        "second concurrent claude request should succeed"
    );

    // Coalescing should reduce upstream hits to exactly 1
    let hits = upstream_hits.load(Ordering::SeqCst);
    assert_eq!(
        hits, 1,
        "coalescing should collapse two identical requests into one upstream call, got {hits}"
    );

    // Both responses should contain the same call number (both coalesced to same upstream call)
    let body_a = resp_a.text().await.unwrap();
    let body_b = resp_b.text().await.unwrap();
    assert_eq!(
        body_a, body_b,
        "coalesced responses should have identical bodies"
    );

    drop(handle);
}

// ─── Test 6: multi_codex_concurrent_flights ───────────────────────────────

#[tokio::test]
async fn multi_codex_concurrent_flights() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;

    // Counter — /v1/responses does NOT coalesce, so expect 2 hits
    let upstream_hits = Arc::new(AtomicU64::new(0));
    let upstream_hits_clone = upstream_hits.clone();

    wiremock::Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(move |_: &wiremock::Request| {
            let n = upstream_hits_clone.fetch_add(1, Ordering::SeqCst) + 1;
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_raw(
                    format!("event: response.completed\ndata: {{\"type\":\"response.completed\",\"call\":{n}}}\n"),
                    "text/event-stream",
                )
        })
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    let body1 = codex_body("gpt-5.6", "Hello");
    let body2 = body1.clone();

    let client = reqwest::Client::new();
    let url = format!("http://{addr}/v1/responses");

    let (resp_a, resp_b) = tokio::join!(
        client
            .post(&url)
            .header("content-type", "application/json")
            .header("x-api-key", "test-key")
            .body(body1)
            .send(),
        client
            .post(&url)
            .header("content-type", "application/json")
            .header("x-api-key", "test-key")
            .body(body2)
            .send(),
    );

    let resp_a = resp_a.unwrap();
    let resp_b = resp_b.unwrap();

    assert_eq!(
        resp_a.status(),
        200,
        "first concurrent codex request should succeed"
    );
    assert_eq!(
        resp_b.status(),
        200,
        "second concurrent codex request should succeed"
    );

    // /v1/responses has no coalescing — both should be independently forwarded
    let hits = upstream_hits.load(Ordering::SeqCst);
    assert_eq!(
        hits, 2,
        "codex route does not coalesce; both requests should hit upstream independently"
    );

    drop(handle);
}

// ─── Test 7: claude_codex_concurrent_no_cross_protocol_coalesce ───────────

#[tokio::test]
async fn claude_codex_concurrent_no_cross_protocol_coalesce() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;

    let claude_hits = Arc::new(AtomicU64::new(0));
    let codex_hits = Arc::new(AtomicU64::new(0));
    let claude_hits_clone = claude_hits.clone();
    let codex_hits_clone = codex_hits.clone();

    // Wiremock handler for /v1/messages (Claude)
    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |_: &wiremock::Request| {
            let n = claude_hits_clone.fetch_add(1, Ordering::SeqCst) + 1;
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_raw(
                    format!("event: message_stop\ndata: {{\"type\":\"message_stop\",\"proto\":\"claude\",\"call\":{n}}}\n"),
                    "text/event-stream",
                )
        })
        .mount(&mock)
        .await;

    // Wiremock handler for /v1/responses (Codex)
    wiremock::Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(move |_: &wiremock::Request| {
            let n = codex_hits_clone.fetch_add(1, Ordering::SeqCst) + 1;
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_raw(
                    format!("event: response.completed\ndata: {{\"type\":\"response.completed\",\"proto\":\"codex\",\"call\":{n}}}\n"),
                    "text/event-stream",
                )
        })
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, handle) = spawn_gateway(&config_dir, &config).await;

    let claude_body = claude_body("claude-sonnet-5", "Hello from Claude");
    let codex_bdy = codex_body("gpt-5.6", "Hello from Codex");

    let client = reqwest::Client::new();
    let claude_url = format!("http://{addr}/v1/messages");
    let codex_url = format!("http://{addr}/v1/responses");

    // Fire Claude and Codex requests simultaneously
    let (claude_resp, codex_resp) = tokio::join!(
        client
            .post(&claude_url)
            .header("content-type", "application/json")
            .header("x-api-key", "test-key")
            .body(claude_body)
            .send(),
        client
            .post(&codex_url)
            .header("content-type", "application/json")
            .header("x-api-key", "test-key")
            .body(codex_bdy)
            .send(),
    );

    let claude_resp = claude_resp.unwrap();
    let codex_resp = codex_resp.unwrap();

    assert_eq!(claude_resp.status(), 200, "claude request should succeed");
    assert_eq!(codex_resp.status(), 200, "codex request should succeed");

    let claude_text = claude_resp.text().await.unwrap();
    let codex_text = codex_resp.text().await.unwrap();

    assert!(
        claude_text.contains("claude"),
        "claude response should contain protocol marker"
    );
    assert!(
        codex_text.contains("codex"),
        "codex response should contain protocol marker"
    );

    // Each protocol hits upstream independently — no cross-protocol coalescing
    let c_hits = claude_hits.load(Ordering::SeqCst);
    let x_hits = codex_hits.load(Ordering::SeqCst);
    assert_eq!(
        c_hits, 1,
        "claude upstream should receive exactly 1 hit, got {c_hits}"
    );
    assert_eq!(
        x_hits, 1,
        "codex upstream should receive exactly 1 hit, got {x_hits}"
    );

    // Verify the protocol markers are different (proving no cross-protocol response sharing)
    assert_ne!(
        claude_text, codex_text,
        "claude and codex responses should differ — no cross-protocol coalescing"
    );

    drop(handle);
}
