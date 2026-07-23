use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use reqwest::Client;
use tracing::{error, info};

use crate::cache;
use crate::config::toche_config::CacheMode;
use crate::continuity;
use crate::efficiency;
use crate::gateway::bounded_body;
use crate::gateway::server::AppState;
use crate::identity::{self, IdentityContext, RequestId};
use crate::meter::db::{LedgerDb, NewLedgerRecord};
use crate::meter::pricing::PricingMap;
use crate::meter::recorder::{RequestTimer, current_project_path, estimate_tokens, record_request};
use crate::protocol::{
    Protocol, anthropic::AnthropicProtocol, openai_responses::OpenAiResponsesProtocol,
};
use crate::reduce;
use crate::safe_cache;
use crate::shield;

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

/// Forward a request to the upstream and collect the full response body,
/// enforcing `max_response_body_bytes`.
async fn forward_to_upstream(
    client: &Client,
    upstream_url: &str,
    upstream_headers: HeaderMap,
    body: String,
    protocol: &dyn Protocol,
    max_response_body_bytes: u64,
) -> Result<ForwardedResponse, axum::response::Response> {
    let response = client
        .post(upstream_url)
        .headers(upstream_headers)
        .body(body)
        .send()
        .await
        .map_err(|e| {
            error!("Upstream request failed: {e}");
            status_response(StatusCode::BAD_GATEWAY)
        })?;

    let status = response.status().as_u16();
    let resp_headers = protocol.parse_response_headers(response.headers());

    let collected = Arc::new(Mutex::new(Vec::new()));
    let collected_for_stream = collected.clone();
    let mut total: u64 = 0;

    let mut stream = response.bytes_stream();
    while let Some(result) = futures::StreamExt::next(&mut stream).await {
        match result {
            Ok(bytes) => {
                let chunk_len = bytes.len() as u64;
                if total + chunk_len > max_response_body_bytes {
                    error!(
                        "Upstream response body exceeded limit: {} + {} > {}",
                        total, chunk_len, max_response_body_bytes
                    );
                    return Err((StatusCode::BAD_GATEWAY, "502 Upstream Response Too Large")
                        .into_response());
                }
                total += chunk_len;
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
        cache_read_tokens: resp_headers.cache_read_tokens,
        cache_create_tokens: resp_headers.cache_create_tokens,
        coalesced_count: 0,
        reduction_input_tokens: 0,
        reduction_output_tokens: 0,
        reduction_count: 0,
        efficiency_tokens_added: 0,
        efficiency_mode: String::new(),
        local_cache_hit: false,
    })
}

/// Helper: convert a `StatusCode` into an axum `Response`.
fn status_response(code: StatusCode) -> axum::response::Response {
    (code, code.canonical_reason().unwrap_or("Unknown")).into_response()
}

/// Convert `StatusCode` to `axum::response::Response`.
macro_rules! bail_status {
    ($code:expr) => {
        return Err(status_response($code))
    };
}

// ---------------------------------------------------------------------------
// Shared pass-through pipeline
// ---------------------------------------------------------------------------

/// Fully-prepared context for forwarding a request upstream after common
/// resolution and header building.
struct PreparedRequest {
    client: reqwest::Client,
    upstream_url: String,
    upstream_headers: HeaderMap,
    resolved: crate::config::resolver::ResolvedIntegration,
    id_ctx: IdentityContext,
    model: String,
    input_tokens: u64,
}

/// Resolve the default integration and build the common identity context.
/// Returns 500 if no default integration is configured.
#[allow(clippy::result_large_err)]
fn resolve_and_build_context(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    protocol: &dyn Protocol,
    body_str: &str,
) -> Result<PreparedRequest, axum::response::Response> {
    let resolved = state
        .default_integration
        .clone()
        .ok_or_else(|| status_response(StatusCode::INTERNAL_SERVER_ERROR))?;

    let model = protocol.extract_model(body_str);
    let input_tokens = estimate_tokens(body_str);

    let upstream_url = format!(
        "{}{}",
        resolved.upstream_url.trim_end_matches('/'),
        protocol.path()
    );

    let workspace_path = current_project_path();
    let workspace_id = if workspace_path.is_empty() {
        None
    } else {
        Some(identity::workspace_id_from_path(&workspace_path))
    };

    let secret_ref_display = resolved.auth.secret_ref.to_string();
    let trust_domain_id = identity::derive_trust_domain_id(
        &resolved.id,
        &resolved.name,
        &resolved.id,
        &secret_ref_display,
    );

    let external_request_id = identity::extract_external_request_id(headers);
    let conversation_id = identity::extract_conversation_id(headers);

    let id_ctx = IdentityContext {
        runtime_id: state.runtime_id.clone(),
        request_id: RequestId::new(),
        external_request_id,
        integration_id: resolved.id.clone(),
        integration_name: resolved.name.clone(),
        upstream_id: resolved.id.clone(),
        upstream_name: resolved.name.clone(),
        trust_domain_id: trust_domain_id.clone(),
        instance_id: None,
        conversation_id,
        workspace_id,
        policy_ids: vec![],
        config_snapshot_hash: state.config_snapshot_hash.clone(),
        attribution: identity::Attribution::Unknown,
    };

    let client = state.http_client.clone();

    let mut upstream_headers = HeaderMap::new();
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if matches!(name_str.as_str(), "host" | "content-length") {
            continue;
        }
        upstream_headers.insert(name.clone(), value.clone());
    }

    for (header_name, header_value) in &resolved.upstream_headers {
        upstream_headers.insert(
            axum::http::HeaderName::from_bytes(header_name.as_bytes())
                .map_err(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR))?,
            axum::http::HeaderValue::from_str(header_value)
                .map_err(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR))?,
        );
    }

    if let Some(value) = &resolved.auth.value {
        upstream_headers.insert(
            axum::http::HeaderName::from_bytes(resolved.auth.header_name.as_bytes())
                .map_err(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR))?,
            axum::http::HeaderValue::from_str(value)
                .map_err(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR))?,
        );
    }

    Ok(PreparedRequest {
        client,
        upstream_url,
        upstream_headers,
        resolved,
        id_ctx,
        model,
        input_tokens,
    })
}

/// Acquire a timed concurrency permit. Returns 503 on timeout or failure.
async fn acquire_concurrency_permit(
    state: &Arc<AppState>,
) -> Result<tokio::sync::OwnedSemaphorePermit, axum::response::Response> {
    tokio::time::timeout(
        std::time::Duration::from_millis(state.upstream_permit_timeout_ms),
        state.upstream_semaphore.clone().acquire_owned(),
    )
    .await
    .map_err(|_| {
        error!("Timed out waiting for concurrency permit");
        status_response(StatusCode::SERVICE_UNAVAILABLE)
    })?
    .map_err(|_| status_response(StatusCode::SERVICE_UNAVAILABLE))
}

/// Record a forwarded response to the ledger (fire-and-forget) and optionally
/// complete a coalesce entry so waiters receive the result.
#[allow(clippy::too_many_arguments)]
fn record_and_complete(
    state: &Arc<AppState>,
    fwd_status: u16,
    fwd_cache_read_tokens: u64,
    fwd_cache_create_tokens: u64,
    fwd_coalesced_count: u64,
    fwd_reduction_input_tokens: u64,
    fwd_reduction_output_tokens: u64,
    fwd_reduction_count: u64,
    fwd_efficiency_mode: String,
    fwd_local_cache_hit: bool,
    body_bytes: Vec<u8>,
    model: String,
    integration_name: String,
    input_tokens: u64,
    latency_ms: u64,
    id_ctx: IdentityContext,
    protocol_name: &'static str,
    shield_key_for_complete: Option<String>,
) {
    let model_clone = model;
    let integration_name_clone = integration_name;
    let status_str = if (200..300).contains(&fwd_status) {
        "success"
    } else {
        "error"
    };
    let project_path = current_project_path();
    let cache_read = fwd_cache_read_tokens;
    let cache_create = fwd_cache_create_tokens;
    let coalesced_count = fwd_coalesced_count;
    let reduction_input = fwd_reduction_input_tokens;
    let reduction_output = fwd_reduction_output_tokens;
    let reduction_count = fwd_reduction_count;
    let efficiency_mode = fwd_efficiency_mode.clone();
    let id_ctx_for_ledger = id_ctx.clone();
    let coalesce_store = Arc::clone(&state.coalesce_store);
    let storage_ledger_db = state.storage_ledger_db.clone();

    tokio::spawn(async move {
        let db = match LedgerDb::open(&storage_ledger_db) {
            Ok(db) => db,
            Err(e) => {
                error!("Failed to open ledger DB: {e}");
                return;
            }
        };
        let pricing = PricingMap::load_embedded();
        let output_tokens = estimate_tokens(&String::from_utf8_lossy(&body_bytes));

        if let Some(key) = shield_key_for_complete {
            coalesce_store.complete(
                &key,
                shield::coalesce::CapturedResponse {
                    status: fwd_status,
                    body_bytes: body_bytes.clone(),
                },
            );
        }

        let record = NewLedgerRecord {
            timestamp: chrono::Utc::now(),
            model: model_clone,
            profile_name: integration_name_clone,
            input_tokens,
            output_tokens,
            cache_read_input_tokens: cache_read,
            cache_creation_input_tokens: cache_create,
            coalesced_count,
            latency_ms,
            status: status_str.to_string(),
            cost: None,
            project_path,
            reduction_input_tokens: reduction_input,
            reduction_output_tokens: reduction_output,
            reduction_count,
            efficiency_mode,
            local_cache_hit: fwd_local_cache_hit,
            runtime_id: id_ctx_for_ledger.runtime_id.as_str().to_string(),
            request_id: id_ctx_for_ledger.request_id.as_str().to_string(),
            integration_id: id_ctx_for_ledger.integration_id,
            upstream_id: id_ctx_for_ledger.upstream_id,
            trust_domain_id: id_ctx_for_ledger.trust_domain_id.as_str().to_string(),
            config_snapshot_hash: id_ctx_for_ledger.config_snapshot_hash,
            attribution: id_ctx_for_ledger.attribution.to_string(),
            protocol: protocol_name.to_string(),
        };
        if let Err(e) = record_request(&db, &pricing, record) {
            error!("Failed to record to ledger: {e}");
        }
    });
}

// ---------------------------------------------------------------------------
// Route: Anthropic Messages
// ---------------------------------------------------------------------------

pub async fn messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, axum::response::Response> {
    let timer = RequestTimer::start();

    // Shared: read body with size limit
    let body_bytes =
        bounded_body::read_body_limited(&headers, body, state.max_request_body_bytes).await?;
    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    let protocol = AnthropicProtocol;

    // Shared: resolve integration, build headers and identity context
    let prep = resolve_and_build_context(&state, &headers, &protocol, &body_str)?;

    // Anthropic-specific: model whitelist enforcement
    if !prep.resolved.models.is_empty() && !prep.resolved.models.contains_key(&prep.model) {
        error!(
            "Model '{}' not configured for integration '{}'. Allowed: {:?}",
            prep.model,
            prep.resolved.name,
            prep.resolved.models.keys().collect::<Vec<_>>()
        );
        bail_status!(StatusCode::BAD_REQUEST);
    }

    info!(
        "Request {}: integration={}, model={}",
        prep.id_ctx.request_id, prep.resolved.name, prep.model
    );

    // Anthropic-specific: bypass headers
    let bypass_all = is_bypassed(&headers, "x-toche-bypass");
    let bypass_shield = bypass_all || is_bypassed(&headers, "x-toche-bypass-shield");
    let bypass_safe_cache = bypass_all || is_bypassed(&headers, "x-toche-bypass-safe-cache");
    let bypass_reduce = bypass_all || is_bypassed(&headers, "x-toche-bypass-reduce");
    let bypass_efficiency = bypass_all || is_bypassed(&headers, "x-toche-bypass-efficiency");
    let bypass_cache = bypass_all || is_bypassed(&headers, "x-toche-bypass-cache");

    let is_streaming = protocol.is_streaming(&body_str);

    // Anthropic-specific: fingerprint + coalescing
    let fingerprint = protocol.fingerprint(&body_str);
    let policy_hash = identity::compute_policy_hash(
        prep.resolved.cache.as_ref().is_some_and(|c| c.enabled),
        prep.resolved
            .cache
            .as_ref()
            .map_or("observe", |c| match c.mode {
                CacheMode::Observe => "observe",
                CacheMode::Auto => "auto",
            }),
        prep.resolved
            .cache
            .as_ref()
            .map_or("standard", |c| match c.breakpoint {
                crate::config::toche_config::CacheBreakpoint::Standard => "standard",
                crate::config::toche_config::CacheBreakpoint::SystemOnly => "system_only",
            }),
        prep.resolved.reduce.is_some(),
        prep.resolved
            .efficiency
            .as_ref()
            .map_or("normal", |e| match e.mode {
                crate::efficiency::config::EfficiencyMode::Normal => "normal",
                crate::efficiency::config::EfficiencyMode::Concise => "concise",
                crate::efficiency::config::EfficiencyMode::Careful => "careful",
            }),
        prep.resolved.safe_cache.as_ref().is_some_and(|s| s.enabled),
    );

    let shield_result = if bypass_shield || is_streaming {
        shield::coalesce::CoalesceResult::Forward { key: String::new() }
    } else {
        state
            .coalesce_store
            .try_acquire(
                &prep.upstream_url,
                &fingerprint,
                prep.id_ctx.trust_domain_id.as_str(),
                &policy_hash,
            )
            .await
    };

    let (forwarded, shield_key_for_complete) = match shield_result {
        shield::coalesce::CoalesceResult::Coalesced { captured } => {
            info!(
                "Shield coalesced: fingerprint={}.., integration={}, model={}, domain={}",
                &fingerprint[..16.min(fingerprint.len())],
                prep.resolved.name,
                prep.model,
                prep.id_ctx.trust_domain_id.as_str()
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
            bail_status!(StatusCode::SERVICE_UNAVAILABLE);
        }
        shield::coalesce::CoalesceResult::Forward { key } => {
            info!("Forwarding to upstream: {}", prep.upstream_url);

            // Anthropic-specific: safe-cache lookup
            let safe_cache_cfg = prep.resolved.safe_cache.as_ref();
            let mut cache_hit: Option<ForwardedResponse> = None;

            if !bypass_safe_cache {
                if let Some(cfg) = safe_cache_cfg {
                    if cfg.enabled {
                        let project = current_project_path();
                        let ws_fp = safe_cache::workspace::compute_workspace_fingerprint();
                        let db_path = &state.storage_ledger_db;
                        if let Ok(cache_db) = safe_cache::cache_db::CacheDb::open(db_path) {
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
                                    match reduce::storage::retrieve_at(
                                        &entry.response_hash,
                                        &state.storage_cas_dir,
                                    ) {
                                        Ok(bytes) => {
                                            let _ = cache_db.touch(&project, &fingerprint);
                                            info!(
                                                "Safe cache hit: fingerprint={}.., project={}, model={}",
                                                &fingerprint[..16.min(fingerprint.len())],
                                                project,
                                                prep.model
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
                state.coalesce_store.complete(
                    &key,
                    shield::coalesce::CapturedResponse {
                        status: cached.status,
                        body_bytes: cached.body_bytes.clone(),
                    },
                );
                (cached, None)
            } else {
                // Anthropic-specific: reduce
                let default_reduce = reduce::config::ReduceConfig::default();
                let reduce_cfg = prep.resolved.reduce.as_ref().unwrap_or(&default_reduce);

                let reduction = reduce::transform::reduce_body_at(
                    &body_str,
                    reduce_cfg,
                    bypass_reduce,
                    &state.storage_cas_dir,
                )
                .unwrap_or_else(|e| {
                    error!("Reduce: falling back to original body: {e}");
                    reduce::transform::ReductionResult {
                        modified_body: body_str.clone(),
                        tokens_raw: 0,
                        tokens_reduced: 0,
                        reductions: 0,
                        passthroughs: 0,
                        hashes: Vec::new(),
                    }
                });

                let reduced_body = reduction.modified_body;

                // Anthropic-specific: efficiency
                let default_efficiency = efficiency::config::EfficiencyConfig::default();
                let efficiency_cfg = prep
                    .resolved
                    .efficiency
                    .as_ref()
                    .unwrap_or(&default_efficiency);

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
                        "Efficiency: {} mode active, {} tokens injected, integration={}, model={}",
                        efficiency_mode_name,
                        efficiency_tokens_added,
                        prep.resolved.name,
                        prep.model
                    );
                }

                // Anthropic-specific: cache control
                let final_body = if let Some(ref cache_cfg) = prep.resolved.cache {
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
                                        "Cache observe: {} breakpoints found (system={}, messages={}), integration={}, model={}",
                                        bp.message_blocks.len()
                                            + if bp.system_block_index.is_some() {
                                                1
                                            } else {
                                                0
                                            },
                                        bp.system_block_index.is_some(),
                                        bp.message_blocks.len(),
                                        prep.resolved.name,
                                        prep.model
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
                                    "Cache auto: {} breakpoints injected, integration={}, model={}",
                                    count, prep.resolved.name, prep.model
                                );
                                protocol
                                    .inject_cache_control(&body_after_efficiency, &bp)
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

                // Anthropic-specific: register reduce CAS hashes
                if !reduction.hashes.is_empty() {
                    let db_path = &state.storage_ledger_db;
                    if let Ok(cache_db) = safe_cache::cache_db::CacheDb::open(db_path) {
                        let mut seen = std::collections::HashSet::new();
                        for hash in &reduction.hashes {
                            if seen.insert(hash) {
                                let _ = cache_db.register_cas(hash);
                            }
                        }
                    }
                }

                // Shared: acquire concurrency permit
                let _permit = acquire_concurrency_permit(&state).await?;

                // Shared: forward to upstream
                let mut fwd = forward_to_upstream(
                    &prep.client,
                    &prep.upstream_url,
                    prep.upstream_headers,
                    final_body,
                    &protocol,
                    state.max_response_body_bytes,
                )
                .await?;
                fwd.reduction_input_tokens = reduction.tokens_raw;
                fwd.reduction_output_tokens = reduction.tokens_reduced;
                fwd.reduction_count = reduction.reductions as u64;
                fwd.efficiency_tokens_added = efficiency_tokens_added;
                fwd.efficiency_mode = efficiency_mode_name;

                // Anthropic-specific: safe-cache persist
                if !bypass_safe_cache {
                    if let Some(cfg) = safe_cache_cfg {
                        if cfg.enabled && fwd.status >= 200 && fwd.status < 300 {
                            let verdict = safe_cache::inspect::inspect_response(&fwd.body_bytes);
                            if verdict.safe && (fwd.body_bytes.len() as u64) <= cfg.max_entry_bytes
                            {
                                let storage_cfg = &state.storage_config;
                                let limits_configured = storage_cfg.max_entries.is_some()
                                    || storage_cfg.max_cas_bytes.is_some()
                                    || storage_cfg.min_free_disk_bytes.is_some();

                                let cache_db_result =
                                    safe_cache::cache_db::CacheDb::open(&state.storage_ledger_db);

                                let within_limits = match cache_db_result {
                                    Err(e) if limits_configured => {
                                        tracing::warn!(
                                            "Storage limit: cannot open cache DB ({}), skipping cache write",
                                            e
                                        );
                                        false
                                    }
                                    Err(_) => true,
                                    Ok(cache_db) => {
                                        let _ = cache_db.evict_expired(cfg.ttl_days);
                                        let _ = cache_db.evict_expired_rejects(cfg.ttl_days);

                                        let mut ok = true;

                                        if let Some(max_entries) = storage_cfg.max_entries {
                                            match cache_db.count(None) {
                                                Ok(count) => {
                                                    if count >= max_entries {
                                                        let existing = cache_db.lookup(
                                                            &current_project_path(),
                                                            &fingerprint,
                                                        );
                                                        match existing {
                                                            Ok(Some(_)) => {}
                                                            _ => {
                                                                tracing::warn!(
                                                                    "Storage limit: max_entries ({}) reached ({}), skipping cache write",
                                                                    max_entries,
                                                                    count
                                                                );
                                                                ok = false;
                                                            }
                                                        }
                                                    }
                                                }
                                                Err(_) => {
                                                    tracing::warn!(
                                                        "Storage limit: cannot read cache count, skipping cache write"
                                                    );
                                                    ok = false;
                                                }
                                            }
                                        }

                                        if ok {
                                            if let Some(max_cas_bytes) = storage_cfg.max_cas_bytes {
                                                let cas_dir = &state.storage_cas_dir;
                                                match cache_db.storage_stats(cas_dir) {
                                                    Ok(stats) => {
                                                        let projected =
                                                            stats.cas_bytes_on_disk.saturating_add(
                                                                fwd.body_bytes.len() as u64,
                                                            );
                                                        if projected > max_cas_bytes {
                                                            tracing::warn!(
                                                                "Storage limit: max_cas_bytes ({}) would be exceeded, skipping cache write",
                                                                max_cas_bytes
                                                            );
                                                            ok = false;
                                                        }
                                                    }
                                                    Err(_) => {
                                                        tracing::warn!(
                                                            "Storage limit: cannot read storage stats, skipping cache write"
                                                        );
                                                        ok = false;
                                                    }
                                                }
                                            }
                                        }

                                        if ok {
                                            if let Some(min_free) = storage_cfg.min_free_disk_bytes
                                            {
                                                let cas_dir = &state.storage_cas_dir;
                                                if safe_cache::cache_db::free_disk_measurable() {
                                                    match safe_cache::cache_db::free_bytes_under(
                                                        cas_dir,
                                                    ) {
                                                        Some(free)
                                                            if free.saturating_sub(
                                                                fwd.body_bytes.len() as u64,
                                                            ) >= min_free => {}
                                                        Some(free) => {
                                                            tracing::warn!(
                                                                "Storage limit: min_free_disk_bytes ({}) would not be met ({} free - {} incoming), skipping cache write",
                                                                min_free,
                                                                free,
                                                                fwd.body_bytes.len()
                                                            );
                                                            ok = false;
                                                        }
                                                        None => {
                                                            tracing::warn!(
                                                                "Storage limit: min_free_disk_bytes ({}) configured but free space cannot be measured, skipping cache write",
                                                                min_free
                                                            );
                                                            ok = false;
                                                        }
                                                    }
                                                } else {
                                                    tracing::warn!(
                                                        "Storage limit: min_free_disk_bytes ({}) configured but free space measurement not available on this platform, skipping cache write",
                                                        min_free
                                                    );
                                                    ok = false;
                                                }
                                            }
                                        }
                                        ok
                                    }
                                };

                                if within_limits {
                                    let stored_hash = reduce::storage::store_new_at(
                                        &fwd.body_bytes,
                                        &state.storage_cas_dir,
                                    );
                                    let ws_fp =
                                        safe_cache::workspace::compute_workspace_fingerprint();
                                    let db_path = &state.storage_ledger_db;
                                    match (
                                        &stored_hash,
                                        safe_cache::cache_db::CacheDb::open(db_path),
                                    ) {
                                        (Ok((hash, created)), Ok(cache_db)) => {
                                            if cache_db
                                                .insert(&safe_cache::cache_db::NewCacheEntry {
                                                    project_path: current_project_path(),
                                                    fingerprint: fingerprint.clone(),
                                                    workspace_fingerprint: ws_fp,
                                                    response_hash: hash.clone(),
                                                    model: prep.model.clone(),
                                                    status: fwd.status as i32,
                                                    tokens_input: prep.input_tokens,
                                                    tokens_output: estimate_tokens(
                                                        &String::from_utf8_lossy(&fwd.body_bytes),
                                                    ),
                                                })
                                                .is_err()
                                                && *created
                                            {
                                                let _ = reduce::storage::delete_at(
                                                    hash,
                                                    &state.storage_cas_dir,
                                                );
                                            }
                                        }
                                        _ => {
                                            if let Ok((hash, true)) = stored_hash {
                                                let _ = reduce::storage::delete_at(
                                                    &hash,
                                                    &state.storage_cas_dir,
                                                );
                                            }
                                        }
                                    }
                                }
                            } else if !verdict.safe && !verdict.reason.is_empty() {
                                let db_path = &state.storage_ledger_db;
                                if let Ok(cache_db) = safe_cache::cache_db::CacheDb::open(db_path) {
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

    let latency_ms = timer.elapsed_ms();

    // Shared: wrap as SSE stream
    let body_bytes_out = forwarded.body_bytes;
    let stream = futures::stream::once({
        let data = String::from_utf8_lossy(&body_bytes_out).to_string();
        async move { Ok(Event::default().data(data)) }
    });

    // Shared: observe response for continuity
    continuity::observer::observe_response(&body_bytes_out);

    // Shared: record to ledger + optionally complete coalesce
    record_and_complete(
        &state,
        forwarded.status,
        forwarded.cache_read_tokens,
        forwarded.cache_create_tokens,
        forwarded.coalesced_count,
        forwarded.reduction_input_tokens,
        forwarded.reduction_output_tokens,
        forwarded.reduction_count,
        forwarded.efficiency_mode,
        forwarded.local_cache_hit,
        body_bytes_out,
        prep.model,
        prep.resolved.name,
        prep.input_tokens,
        latency_ms,
        prep.id_ctx,
        protocol.name(),
        shield_key_for_complete,
    );

    Ok(Sse::new(stream))
}

// ---------------------------------------------------------------------------
// Route: OpenAI Responses
// ---------------------------------------------------------------------------

pub async fn responses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, axum::response::Response> {
    let timer = RequestTimer::start();

    // Shared: read body with size limit
    let body_bytes =
        bounded_body::read_body_limited(&headers, body, state.max_request_body_bytes).await?;
    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    let protocol = OpenAiResponsesProtocol;

    // Shared: resolve integration, build headers and identity context
    let prep = resolve_and_build_context(&state, &headers, &protocol, &body_str)?;

    info!(
        "Request {} (responses): integration={}, model={}",
        prep.id_ctx.request_id, prep.resolved.name, prep.model
    );

    // Responses: no model whitelist enforcement, no bypass headers, no
    // fingerprint, no coalescing, no safe-cache, no reduce, no efficiency,
    // no cache control.

    // Shared: acquire concurrency permit
    let _permit = acquire_concurrency_permit(&state).await?;

    info!("Forwarding (responses) to upstream: {}", prep.upstream_url);

    // Shared: forward to upstream
    let fwd = forward_to_upstream(
        &prep.client,
        &prep.upstream_url,
        prep.upstream_headers,
        body_str,
        &protocol,
        state.max_response_body_bytes,
    )
    .await?;

    let latency_ms = timer.elapsed_ms();

    // Shared: wrap as SSE stream
    let body_bytes_out = fwd.body_bytes;
    let stream = futures::stream::once({
        let data = String::from_utf8_lossy(&body_bytes_out).to_string();
        async move { Ok(Event::default().data(data)) }
    });

    // Shared: observe response for continuity
    continuity::observer::observe_response(&body_bytes_out);

    // Shared: record to ledger (no coalesce completion for Responses)
    record_and_complete(
        &state,
        fwd.status,
        fwd.cache_read_tokens,
        fwd.cache_create_tokens,
        fwd.coalesced_count,
        fwd.reduction_input_tokens,
        fwd.reduction_output_tokens,
        fwd.reduction_count,
        fwd.efficiency_mode,
        fwd.local_cache_hit,
        body_bytes_out,
        prep.model,
        prep.resolved.name,
        prep.input_tokens,
        latency_ms,
        prep.id_ctx,
        protocol.name(),
        None,
    );

    Ok(Sse::new(stream))
}
