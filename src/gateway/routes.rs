use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use futures::stream::StreamExt;
use reqwest::Client;
use tracing::{error, info};

use crate::cache;
use crate::meter::db::{LedgerDb, NewLedgerRecord};
use crate::meter::pricing::PricingMap;
use crate::meter::recorder::{current_project_path, estimate_tokens, record_request, RequestTimer};
use crate::profiles::loader::{config_dir, load_profiles};
use crate::profiles::types::CacheMode;

/// Parse the "model" field from an Anthropic Messages API JSON body.
fn extract_model(body: &str) -> String {
    let mut in_key = false;
    let mut key_buf = String::new();

    for ch in body.chars() {
        if ch == '"' {
            if in_key {
                if key_buf == "model" {
                    let after_key = body.split(&format!("\"{}\"", key_buf)).nth(1).unwrap_or("");
                    let after_colon = after_key.trim_start().strip_prefix(':').unwrap_or(after_key);
                    let value_start = after_colon.trim_start();
                    if let Some(rest) = value_start.strip_prefix('"') {
                        let end = rest.find('"').unwrap_or(rest.len());
                        return rest[..end].to_string();
                    }
                }
                key_buf.clear();
                in_key = false;
            } else {
                in_key = true;
            }
        } else if in_key {
            key_buf.push(ch);
        }
    }
    "unknown".to_string()
}

pub async fn messages(
    headers: HeaderMap,
    body: String,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let timer = RequestTimer::start();

    let profiles = load_profiles().map_err(|e| {
        error!("Failed to load profiles: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let profile = profiles.default_profile().ok_or_else(|| {
        error!("No default profile configured");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let model = extract_model(&body);
    let profile_name = profile.name.clone();
    let input_tokens = estimate_tokens(&body);

    // Cache coordination: evaluate breakpoints before forwarding
    let modified_body = if let Some(ref cache_cfg) = profile.cache {
        if cache_cfg.enabled {
            let plan_result =
                cache::breakpoint::find_breakpoints(&body, &cache_cfg.breakpoint);
            let bp = plan_result.unwrap_or_else(|e| {
                error!("Breakpoint detection failed: {e}");
                cache::breakpoint::BreakpointPlan {
                    system_block_index: None,
                    message_blocks: Vec::new(),
                }
            });
            match cache_cfg.mode {
                CacheMode::Observe => {
                    if bp.has_breakpoints() {
                        info!(
                            "Cache observe: {} breakpoints found (system={}, messages={}), profile={}, model={}",
                            bp.message_blocks.len()
                                + if bp.system_block_index.is_some() { 1 } else { 0 },
                            bp.system_block_index.is_some(),
                            bp.message_blocks.len(),
                            profile_name,
                            model
                        );
                    }
                    body.clone() // forward original body unchanged
                }
                CacheMode::Auto => {
                    let count = bp.message_blocks.len()
                        + if bp.system_block_index.is_some() { 1 } else { 0 };
                    info!(
                        "Cache auto: {} breakpoints injected, profile={}, model={}",
                        count, profile_name, model
                    );
                    cache::inject::inject_cache_control(&body, &bp)
                        .unwrap_or_else(|e| {
                            error!("Cache injection failed: {e}, forwarding original");
                            body.clone()
                        })
                }
            }
        } else {
            body.clone() // cache disabled for this profile
        }
    } else {
        body.clone() // no cache config: pass through unchanged
    };

    let upstream_url = format!("{}/v1/messages", profile.upstream_url.trim_end_matches('/'));

    let client = Client::builder()
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut upstream_headers = HeaderMap::new();
    for (name, value) in &headers {
        let name_str = name.as_str().to_lowercase();
        if matches!(name_str.as_str(), "host" | "content-length") {
            continue;
        }
        upstream_headers.insert(name.clone(), value.clone());
    }

    for (header_name, header_value) in &profile.headers {
        upstream_headers.insert(
            axum::http::HeaderName::from_bytes(header_name.as_bytes())
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            axum::http::HeaderValue::from_str(header_value)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        );
    }

    info!("Forwarding to upstream: {upstream_url}");

    let response = client
        .post(&upstream_url)
        .headers(upstream_headers)
        .body(modified_body)
        .send()
        .await
        .map_err(|e| {
            error!("Upstream request failed: {e}");
            StatusCode::BAD_GATEWAY
        })?;

    let http_status = response.status();
    let status_str = if http_status.is_success() {
        "success"
    } else {
        error!("Upstream returned {http_status}");
        "error"
    };
    let latency_ms = timer.elapsed_ms();

    // Parse cache usage from upstream response headers
    let upstream_cache_read: u64 = response
        .headers()
        .get("anthropic-cache-read-input-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let upstream_cache_create: u64 = response
        .headers()
        .get("anthropic-cache-creation-input-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Collect stream bytes for token estimation, then pass through
    let collected_bytes = Arc::new(Mutex::new(Vec::new()));
    let collected_for_stream = collected_bytes.clone();

    let stream = response.bytes_stream().map({
        move |result| match result {
            Ok(ref bytes) => {
                if let Ok(mut buf) = collected_for_stream.lock() {
                    buf.extend_from_slice(bytes);
                }
                Ok(Event::default().data(String::from_utf8_lossy(bytes).to_string()))
            }
            Err(e) => {
                error!("Stream error: {e}");
                Ok(Event::default().data(format!("Stream error: {e}")))
            }
        }
    });

    // Record to ledger (fire-and-forget — ledger failure must not affect streaming)
    let model_clone = model;
    let profile_clone = profile_name;
    let status_clone = status_str.to_string();
    let collected_clone = collected_bytes.clone();
    let project_path = current_project_path();
    tokio::spawn(async move {
        let db_path = config_dir().join("ledger.db");
        let db = match LedgerDb::open(&db_path) {
            Ok(db) => db,
            Err(e) => {
                error!("Failed to open ledger DB: {e}");
                return;
            }
        };
        let pricing = PricingMap::load_embedded();
        // Brief delay so the stream has started accumulating bytes
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let locked = collected_clone.lock().unwrap_or_else(|e| e.into_inner());
        let output_text = String::from_utf8_lossy(&locked).to_string();
        let output_tokens = estimate_tokens(&output_text);
        let record = NewLedgerRecord {
            timestamp: chrono::Utc::now(),
            model: model_clone,
            profile_name: profile_clone,
            input_tokens,
            output_tokens,
            cache_read_input_tokens: upstream_cache_read,
            cache_creation_input_tokens: upstream_cache_create,
            latency_ms,
            status: status_clone,
            cost: None,
            project_path,
        };
        if let Err(e) = record_request(&db, &pricing, record) {
            error!("Failed to record to ledger: {e}");
        }
    });

    Ok(Sse::new(stream))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_model_present() {
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[]}"#;
        assert_eq!(extract_model(body), "claude-sonnet-5");
    }

    #[test]
    fn test_extract_model_with_slash() {
        let body = r#"{"model":"cx/gpt-5.6-sol","messages":[]}"#;
        assert_eq!(extract_model(body), "cx/gpt-5.6-sol");
    }

    #[test]
    fn test_extract_model_missing() {
        let body = r#"{"max_tokens":1024}"#;
        assert_eq!(extract_model(body), "unknown");
    }

    #[test]
    fn test_extract_model_empty_body() {
        assert_eq!(extract_model(""), "unknown");
    }

    #[test]
    fn test_extract_model_date_suffix_preserved() {
        let body = r#"{"model":"claude-sonnet-5-20251001","messages":[]}"#;
        assert_eq!(extract_model(body), "claude-sonnet-5-20251001");
    }
}
