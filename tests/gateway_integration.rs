use axum::body::Body;
use axum::http::Request;
use axum::routing::post;
use axum::Router;

/// A minimal mock upstream that returns a text-only SSE stream.
async fn mock_upstream(_req: Request<Body>) -> axum::response::Response<Body> {
    let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-sonnet-5\"}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello from Toche!\"}}\n\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n";
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn mock_upstream_returns_sse() {
    let app = Router::new().route("/v1/messages", post(mock_upstream));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(r#"{"model":"claude-sonnet-5","max_tokens":1,"messages":[{"role":"user","content":"Hello"}]}"#)
        .send()
        .await
        .expect("request to mock upstream");

    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(text.contains("Hello from Toche!"));
    assert!(text.contains("message_stop"));
}

#[test]
fn fingerprint_is_deterministic_across_runs() {
    let body = r#"{"model":"claude-sonnet-5","max_tokens":1,"messages":[{"role":"user","content":"Hello"}]}"#;
    let fp1 = toche::shield::fingerprint::compute(body);
    let fp2 = toche::shield::fingerprint::compute(body);
    assert_eq!(fp1, fp2);
    assert_eq!(fp1.len(), 64);
}

#[test]
fn safe_cache_inspect_text_is_safe() {
    let body = r#"{"type":"message","role":"assistant","content":[{"type":"text","text":"Hello!"}],"stop_reason":"end_turn"}"#;
    let verdict = toche::safe_cache::inspect::inspect_response(body.as_bytes());
    assert!(verdict.safe);
}

