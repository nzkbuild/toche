use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use reqwest::Client;
use tracing::{error, info};

use crate::cache;
use crate::meter::db::{LedgerDb, NewLedgerRecord};
use crate::meter::pricing::PricingMap;
use crate::meter::recorder::{current_project_path, estimate_tokens, record_request, RequestTimer};
use crate::profiles::loader::{config_dir, load_profiles};
use crate::profiles::types::CacheMode;
use crate::shield;

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

/// Result of forwarding a request to the upstream API.
struct ForwardedResponse {
    status: u16,
    body_bytes: Vec<u8>,
    cache_read_tokens: u64,
    cache_create_tokens: u64,
    coalesced_count: u64,
}

/// Forward a request to the upstream and collect the full response body.
async fn forward_to_upstream(
    client: &Client,
    upstream_url: &str,
    upstream_headers: HeaderMap,
    body: String,
) -> Result<ForwardedResponse, StatusCode> {
    let response = client
        .post(upstream_url)
        .headers(upstream_headers)
        .body(body)
        .send()
        .await
        .map_err(|e| {
            error!("Upstream request failed: {e}");
            StatusCode::BAD_GATEWAY
        })?;

    let status = response.status().as_u16();
    let cache_read: u64 = response
        .headers()
        .get("anthropic-cache-read-input-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let cache_create: u64 = response
        .headers()
        .get("anthropic-cache-creation-input-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Collect full response body
    let collected = Arc::new(Mutex::new(Vec::new()));
    let collected_for_stream = collected.clone();

    let mut stream = response.bytes_stream();
    while let Some(result) = futures::StreamExt::next(&mut stream).await {
        match result {
            Ok(bytes) => {
                if let Ok(mut buf) = collected_for_stream.lock() {
                    buf.extend_from_slice(&bytes);
                }
            }
            Err(e) => {
                error!("Stream error: {e}");
            }
        }
    }

    let body_bytes = collected.lock().unwrap_or_else(|e| e.into_inner()).clone();

    Ok(ForwardedResponse {
        status,
        body_bytes,
        cache_read_tokens: cache_read,
        cache_create_tokens: cache_create,
        coalesced_count: 0,
    })
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
                    body.clone()
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
            body.clone()
        }
    } else {
        body.clone()
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

    // Request Shield: coalesce identical in-flight requests
    let fingerprint = shield::fingerprint::compute(&modified_body);

    let shield_result = shield::coalesce::store()
        .try_acquire(&upstream_url, &fingerprint)
        .await;

    let (forwarded, shield_key_for_complete) = match shield_result {
        shield::coalesce::CoalesceResult::Coalesced { captured } => {
            info!(
                "Shield coalesced: fingerprint={}.., profile={}, model={}",
                &fingerprint[..16.min(fingerprint.len())],
                profile_name,
                model
            );
            (
                ForwardedResponse {
                    status: captured.status,
                    body_bytes: captured.body_bytes,
                    cache_read_tokens: 0,
                    cache_create_tokens: 0,
                    coalesced_count: 1,
                },
                None,
            )
        }
        shield::coalesce::CoalesceResult::Failed => {
            error!("Shield: prior request failed, rejecting");
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        shield::coalesce::CoalesceResult::Forward { key } => {
            info!("Forwarding to upstream: {upstream_url}");
            let fwd = forward_to_upstream(
                &client,
                &upstream_url,
                upstream_headers,
                modified_body,
            )
            .await?;
            (fwd, Some(key))
        }
    };

    let status_str = if forwarded.status >= 200 && forwarded.status < 300 {
        "success"
    } else {
        "error"
    };
    let latency_ms = timer.elapsed_ms();

    // Build SSE stream from collected bytes
    let body_bytes = forwarded.body_bytes;
    let stream = futures::stream::once({
        let data = String::from_utf8_lossy(&body_bytes).to_string();
        async move { Ok(Event::default().data(data)) }
    });

    // Record to ledger (fire-and-forget)
    let model_clone = model;
    let profile_clone = profile_name;
    let status_clone = status_str.to_string();
    let project_path = current_project_path();
    let cache_read = forwarded.cache_read_tokens;
    let cache_create = forwarded.cache_create_tokens;
    let coalesced_count = forwarded.coalesced_count;
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
        let output_tokens = estimate_tokens(&String::from_utf8_lossy(&body_bytes));

        // Complete shield entry so any waiters get the result
        if let Some(key) = shield_key_for_complete {
            shield::coalesce::store().complete(
                &key,
                shield::coalesce::CapturedResponse {
                    status: forwarded.status,
                    body_bytes: body_bytes.clone(),
                },
            );
        }

        let record = NewLedgerRecord {
            timestamp: chrono::Utc::now(),
            model: model_clone,
            profile_name: profile_clone,
            input_tokens,
            output_tokens,
            cache_read_input_tokens: cache_read,
            cache_creation_input_tokens: cache_create,
            coalesced_count,
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
