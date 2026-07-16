use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use reqwest::Client;
use tracing::{error, info};

use crate::cache;
use crate::continuity;
use crate::efficiency;
use crate::meter::db::{LedgerDb, NewLedgerRecord};
use crate::meter::pricing::PricingMap;
use crate::meter::recorder::{RequestTimer, current_project_path, estimate_tokens, record_request};
use crate::profiles::loader::{config_dir, load_profiles};
use crate::profiles::types::CacheMode;
use crate::reduce;
use crate::safe_cache;
use crate::shield;

/// Parse the "model" field from an Anthropic Messages API JSON body.
fn extract_model(body: &str) -> String {
    // Fast path: for the common case of a top-level "model" key, use
    // serde_json Value for structural correctness (handles escapes, nested
    // objects, and the key appearing inside string values correctly).
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("model")?.as_str().map(String::from))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Check whether a bypass header is set to "true" (case-insensitive).
fn is_bypassed(headers: &HeaderMap, name: &str) -> bool {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_lowercase())
        .as_deref()
        == Some("true")
}

/// Result of forwarding a request to the upstream API.
struct ForwardedResponse {
    status: u16,
    body_bytes: Vec<u8>,
    cache_read_tokens: u64,
    cache_create_tokens: u64,
    coalesced_count: u64,
    reduction_input_tokens: u64,
    reduction_output_tokens: u64,
    reduction_count: u64,
    efficiency_tokens_added: u64,
    efficiency_mode: String,
    local_cache_hit: bool,
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
        reduction_input_tokens: 0,
        reduction_output_tokens: 0,
        reduction_count: 0,
        efficiency_tokens_added: 0,
        efficiency_mode: String::new(),
        local_cache_hit: false,
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

    // Step 1: Request Shield — coalesce identical in-flight requests on the
    // raw body (before any reduction/cache modifications) for maximum
    // coalescing.
    let bypass_all = is_bypassed(&headers, "x-toche-bypass");
    let bypass_shield = bypass_all || is_bypassed(&headers, "x-toche-bypass-shield");
    let bypass_safe_cache = bypass_all || is_bypassed(&headers, "x-toche-bypass-safe-cache");
    let bypass_reduce = bypass_all || is_bypassed(&headers, "x-toche-bypass-reduce");
    let bypass_efficiency = bypass_all || is_bypassed(&headers, "x-toche-bypass-efficiency");
    let bypass_cache = bypass_all || is_bypassed(&headers, "x-toche-bypass-cache");

    let fingerprint = shield::fingerprint::compute(&body);

    let shield_result = if bypass_shield {
        shield::coalesce::CoalesceResult::Forward { key: String::new() }
    } else {
        shield::coalesce::store()
            .try_acquire(&upstream_url, &fingerprint)
            .await
    };

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
                    reduction_input_tokens: 0,
                    reduction_output_tokens: 0,
                    reduction_count: 0,
                    efficiency_tokens_added: 0,
                    efficiency_mode: String::new(),
                    local_cache_hit: false,
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

            // Step 1.5: Persistent safe cache check (after shield, before reduce).
            let safe_cache_cfg = profile.safe_cache.as_ref();

            let mut cache_hit: Option<ForwardedResponse> = None;

            if !bypass_safe_cache {
                if let Some(cfg) = safe_cache_cfg {
                    if cfg.enabled {
                        let project = current_project_path();
                        let ws_fp = safe_cache::workspace::compute_workspace_fingerprint();
                        let db_path = config_dir().join("ledger.db");
                        if let Ok(cache_db) = safe_cache::cache_db::CacheDb::open(&db_path) {
                            let _ = cache_db.evict_expired(cfg.ttl_days);
                            let _ = cache_db.evict_expired_rejects(cfg.ttl_days);
                            if let Ok(Some(entry)) = cache_db.lookup(&project, &fingerprint) {
                                if entry.workspace_fingerprint != ws_fp {
                                    let _ = cache_db.insert_reject(
                                        &project,
                                        &fingerprint,
                                        "workspace fingerprint mismatch",
                                    );
                                } else {
                                    match reduce::storage::retrieve(&entry.response_hash) {
                                        Ok(bytes) => {
                                            let _ = cache_db.touch(&project, &fingerprint);
                                            info!(
                                                "Safe cache hit: fingerprint={}.., project={}, model={}",
                                                &fingerprint[..16.min(fingerprint.len())],
                                                project,
                                                model
                                            );
                                            cache_hit = Some(ForwardedResponse {
                                                status: entry.status as u16,
                                                body_bytes: bytes,
                                                cache_read_tokens: 0,
                                                cache_create_tokens: 0,
                                                coalesced_count: 0,
                                                reduction_input_tokens: 0,
                                                reduction_output_tokens: 0,
                                                reduction_count: 0,
                                                efficiency_tokens_added: 0,
                                                efficiency_mode: String::new(),
                                                local_cache_hit: true,
                                            });
                                        }
                                        Err(_) => {
                                            let _ = cache_db.insert_reject(
                                                &project,
                                                &fingerprint,
                                                "CAS blob not found",
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(cached) = cache_hit {
                // Complete shield slot so waiters get the cached response
                shield::coalesce::store().complete(
                    &key,
                    shield::coalesce::CapturedResponse {
                        status: cached.status,
                        body_bytes: cached.body_bytes.clone(),
                    },
                );
                (cached, None)
            } else {
                // Step 2: Reduce tool output in the request body (after shield,
                // before cache injection — deterministic reduction is cache-friendly).
                let default_reduce = reduce::config::ReduceConfig::default();
                let reduce_cfg = profile.reduce.as_ref().unwrap_or(&default_reduce);

                let reduction = reduce::transform::reduce_body(&body, reduce_cfg, bypass_reduce)
                    .unwrap_or_else(|e| {
                        error!("Reduce: falling back to original body: {e}");
                        reduce::transform::ReductionResult {
                            modified_body: body.clone(),
                            tokens_raw: 0,
                            tokens_reduced: 0,
                            reductions: 0,
                            passthroughs: 0,
                            hashes: Vec::new(),
                        }
                    });

                let reduced_body = reduction.modified_body;

                // Step 2.5: Efficiency profile injection (after reduce, before cache).
                let default_efficiency = efficiency::config::EfficiencyConfig::default();
                let efficiency_cfg = profile.efficiency.as_ref().unwrap_or(&default_efficiency);

                let instruction = if bypass_efficiency {
                    None
                } else {
                    efficiency::instructions::instruction_for_mode(&efficiency_cfg.mode)
                };

                let efficiency_result =
                    efficiency::inject::inject_efficiency(&reduced_body, instruction)
                        .unwrap_or_else(|e| {
                            error!("Efficiency injection failed, forwarding reduced body: {e}");
                            efficiency::inject::InjectionResult {
                                modified_body: reduced_body.clone(),
                                tokens_added: 0,
                            }
                        });

                let efficiency_mode_name = match efficiency_cfg.mode {
                    efficiency::config::EfficiencyMode::Normal => String::new(),
                    efficiency::config::EfficiencyMode::Concise => "concise".to_string(),
                    efficiency::config::EfficiencyMode::Careful => "careful".to_string(),
                };
                let efficiency_tokens_added = efficiency_result.tokens_added;
                let body_after_efficiency = efficiency_result.modified_body;

                if !efficiency_mode_name.is_empty() {
                    info!(
                        "Efficiency: {} mode active, {} tokens injected, profile={}, model={}",
                        efficiency_mode_name, efficiency_tokens_added, profile_name, model
                    );
                }

                // Step 3: Cache breakpoint injection on the efficiency-modified body.
                let final_body = if let Some(ref cache_cfg) = profile.cache {
                    if cache_cfg.enabled && !bypass_cache {
                        let plan_result = cache::breakpoint::find_breakpoints(
                            &body_after_efficiency,
                            &cache_cfg.breakpoint,
                        );
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
                                            + if bp.system_block_index.is_some() {
                                                1
                                            } else {
                                                0
                                            },
                                        bp.system_block_index.is_some(),
                                        bp.message_blocks.len(),
                                        profile_name,
                                        model
                                    );
                                }
                                body_after_efficiency
                            }
                            CacheMode::Auto => {
                                let count = bp.message_blocks.len()
                                    + if bp.system_block_index.is_some() {
                                        1
                                    } else {
                                        0
                                    };
                                info!(
                                    "Cache auto: {} breakpoints injected, profile={}, model={}",
                                    count, profile_name, model
                                );
                                cache::inject::inject_cache_control(&body_after_efficiency, &bp)
                                    .unwrap_or_else(|e| {
                                        error!("Cache injection failed: {e}, forwarding body");
                                        body_after_efficiency
                                    })
                            }
                        }
                    } else {
                        body_after_efficiency
                    }
                } else {
                    body_after_efficiency
                };

                let mut fwd =
                    forward_to_upstream(&client, &upstream_url, upstream_headers, final_body)
                        .await?;
                fwd.reduction_input_tokens = reduction.tokens_raw;
                fwd.reduction_output_tokens = reduction.tokens_reduced;
                fwd.reduction_count = reduction.reductions as u64;
                fwd.efficiency_tokens_added = efficiency_tokens_added;
                fwd.efficiency_mode = efficiency_mode_name;

                // On cache miss with safe response, store for future reuse.
                if !bypass_safe_cache {
                    if let Some(cfg) = safe_cache_cfg {
                        if cfg.enabled && fwd.status >= 200 && fwd.status < 300 {
                            let verdict = safe_cache::inspect::inspect_response(&fwd.body_bytes);
                            if verdict.safe && (fwd.body_bytes.len() as u64) <= cfg.max_entry_bytes
                            {
                                if let Ok(hash) = reduce::storage::store(&fwd.body_bytes) {
                                    let ws_fp =
                                        safe_cache::workspace::compute_workspace_fingerprint();
                                    let db_path = config_dir().join("ledger.db");
                                    if let Ok(cache_db) =
                                        safe_cache::cache_db::CacheDb::open(&db_path)
                                    {
                                        let _ =
                                            cache_db.insert(&safe_cache::cache_db::NewCacheEntry {
                                                project_path: current_project_path(),
                                                fingerprint: fingerprint.clone(),
                                                workspace_fingerprint: ws_fp,
                                                response_hash: hash,
                                                model: model.clone(),
                                                status: fwd.status as i32,
                                                tokens_input: input_tokens,
                                                tokens_output: estimate_tokens(
                                                    &String::from_utf8_lossy(&fwd.body_bytes),
                                                ),
                                            });
                                    }
                                }
                            } else if !verdict.safe && !verdict.reason.is_empty() {
                                let db_path = config_dir().join("ledger.db");
                                if let Ok(cache_db) = safe_cache::cache_db::CacheDb::open(&db_path)
                                {
                                    let _ = cache_db.insert_reject(
                                        &current_project_path(),
                                        &fingerprint,
                                        &verdict.reason,
                                    );
                                }
                            }
                        }
                    }
                }

                (fwd, if key.is_empty() { None } else { Some(key) })
            }
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

    // Feed response to session observer for fact collection
    continuity::observer::observe_response(&body_bytes);

    // Record to ledger (fire-and-forget)
    let model_clone = model;
    let profile_clone = profile_name;
    let status_clone = status_str.to_string();
    let project_path = current_project_path();
    let cache_read = forwarded.cache_read_tokens;
    let cache_create = forwarded.cache_create_tokens;
    let coalesced_count = forwarded.coalesced_count;
    let reduction_input = forwarded.reduction_input_tokens;
    let reduction_output = forwarded.reduction_output_tokens;
    let reduction_count = forwarded.reduction_count;
    let efficiency_mode = forwarded.efficiency_mode;
    let _efficiency_tokens_added = forwarded.efficiency_tokens_added;
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
            reduction_input_tokens: reduction_input,
            reduction_output_tokens: reduction_output,
            reduction_count,
            efficiency_mode,
            local_cache_hit: forwarded.local_cache_hit,
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

    #[test]
    fn test_extract_model_key_in_string_value_not_matched() {
        // The word "model" appearing inside a message string must not be
        // mistaken for the top-level "model" key.
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":"which model to use"}]}"#;
        assert_eq!(extract_model(body), "claude-sonnet-5");
    }

    #[test]
    fn test_extract_model_with_escaped_quotes() {
        let body = r#"{"model":"claude-sonnet-5","messages":[{"role":"user","content":"he said: \"which model?\""}]}"#;
        assert_eq!(extract_model(body), "claude-sonnet-5");
    }
}
