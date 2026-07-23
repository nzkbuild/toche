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

pub async fn messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, axum::response::Response> {
    let timer = RequestTimer::start();
    let request_id = RequestId::new();

    // Read body with size limit — fails with 413 before String/JSON parsing
    let body_bytes =
        bounded_body::read_body_limited(&headers, body, state.max_request_body_bytes).await?;
    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    let resolved = state
        .default_integration
        .clone()
        .ok_or_else(|| status_response(StatusCode::INTERNAL_SERVER_ERROR))?;

    let protocol = AnthropicProtocol;

    let model = protocol.extract_model(&body_str);
    let integration_name = resolved.name.clone();
    let input_tokens = estimate_tokens(&body_str);

    // Model validation: if integration has a whitelist, reject unknown models
    if !resolved.models.is_empty() && !resolved.models.contains_key(&model) {
        error!(
            "Model '{}' not configured for integration '{}'. Allowed: {:?}",
            model,
            integration_name,
            resolved.models.keys().collect::<Vec<_>>()
        );
        bail_status!(StatusCode::BAD_REQUEST);
    }

    let upstream_url = format!(
        "{}{}",
        resolved.upstream_url.trim_end_matches('/'),
        protocol.path()
    );

    // Build identity context
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

    let policy_hash = identity::compute_policy_hash(
        resolved.cache.as_ref().is_some_and(|c| c.enabled),
        resolved.cache.as_ref().map_or("observe", |c| match c.mode {
            crate::config::toche_config::CacheMode::Observe => "observe",
            crate::config::toche_config::CacheMode::Auto => "auto",
        }),
        resolved
            .cache
            .as_ref()
            .map_or("standard", |c| match c.breakpoint {
                crate::config::toche_config::CacheBreakpoint::Standard => "standard",
                crate::config::toche_config::CacheBreakpoint::SystemOnly => "system_only",
            }),
        resolved.reduce.is_some(),
        resolved
            .efficiency
            .as_ref()
            .map_or("normal", |e| match e.mode {
                crate::efficiency::config::EfficiencyMode::Normal => "normal",
                crate::efficiency::config::EfficiencyMode::Concise => "concise",
                crate::efficiency::config::EfficiencyMode::Careful => "careful",
            }),
        resolved.safe_cache.as_ref().is_some_and(|s| s.enabled),
    );

    let external_request_id = identity::extract_external_request_id(&headers);
    let conversation_id = identity::extract_conversation_id(&headers);

    let id_ctx = IdentityContext {
        runtime_id: state.runtime_id.clone(),
        request_id: request_id.clone(),
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

    info!(
        "Request {}: integration={}, model={}",
        request_id, integration_name, model
    );

    // Use the shared client from AppState
    let client = &state.http_client;

    let mut upstream_headers = HeaderMap::new();
    for (name, value) in &headers {
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

    // Add auth header if configured
    if let Some(value) = &resolved.auth.value {
        upstream_headers.insert(
            axum::http::HeaderName::from_bytes(resolved.auth.header_name.as_bytes())
                .map_err(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR))?,
            axum::http::HeaderValue::from_str(value)
                .map_err(|_| status_response(StatusCode::INTERNAL_SERVER_ERROR))?,
        );
    }

    let bypass_all = is_bypassed(&headers, "x-toche-bypass");
    let bypass_shield = bypass_all || is_bypassed(&headers, "x-toche-bypass-shield");
    let bypass_safe_cache = bypass_all || is_bypassed(&headers, "x-toche-bypass-safe-cache");
    let bypass_reduce = bypass_all || is_bypassed(&headers, "x-toche-bypass-reduce");
    let bypass_efficiency = bypass_all || is_bypassed(&headers, "x-toche-bypass-efficiency");
    let bypass_cache = bypass_all || is_bypassed(&headers, "x-toche-bypass-cache");

    let is_streaming = protocol.is_streaming(&body_str);

    let fingerprint = protocol.fingerprint(&body_str);
    let shield_result = if bypass_shield || is_streaming {
        shield::coalesce::CoalesceResult::Forward { key: String::new() }
    } else {
        state
            .coalesce_store
            .try_acquire(
                &upstream_url,
                &fingerprint,
                id_ctx.trust_domain_id.as_str(),
                &policy_hash,
            )
            .await
    };

    let (forwarded, shield_key_for_complete) = match shield_result {
        shield::coalesce::CoalesceResult::Coalesced { captured } => {
            info!(
                "Shield coalesced: fingerprint={}.., integration={}, model={}, domain={}",
                &fingerprint[..16.min(fingerprint.len())],
                integration_name,
                model,
                id_ctx.trust_domain_id.as_str()
            );
            // Coalesced waiter uses NO semaphore permit
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
            info!("Forwarding to upstream: {upstream_url}");

            // Step 1.5: Persistent safe cache check (after shield, before reduce).
            let safe_cache_cfg = resolved.safe_cache.as_ref();

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
                state.coalesce_store.complete(
                    &key,
                    shield::coalesce::CapturedResponse {
                        status: cached.status,
                        body_bytes: cached.body_bytes.clone(),
                    },
                );
                (cached, None)
            } else {
                let default_reduce = reduce::config::ReduceConfig::default();
                let reduce_cfg = resolved.reduce.as_ref().unwrap_or(&default_reduce);

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

                let default_efficiency = efficiency::config::EfficiencyConfig::default();
                let efficiency_cfg = resolved.efficiency.as_ref().unwrap_or(&default_efficiency);

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
                        efficiency_mode_name, efficiency_tokens_added, integration_name, model
                    );
                }

                let final_body = if let Some(ref cache_cfg) = resolved.cache {
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
                                        integration_name,
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
                                    "Cache auto: {} breakpoints injected, integration={}, model={}",
                                    count, integration_name, model
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

                // Register reduce CAS hashes in the registry before
                // forwarding upstream so references exist even if the
                // upstream call fails mid-flight.  Deduplicate so the
                // same hash appearing multiple times counts once.
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

                // Acquire a timed permit only while the provider request is active.
                // Safe-cache hits and coalesced waiters consume zero permits.
                let _permit = tokio::time::timeout(
                    std::time::Duration::from_millis(state.upstream_permit_timeout_ms),
                    state.upstream_semaphore.acquire(),
                )
                .await
                .map_err(|_| {
                    error!("Timed out waiting for concurrency permit");
                    status_response(StatusCode::SERVICE_UNAVAILABLE)
                })?
                .map_err(|_| status_response(StatusCode::SERVICE_UNAVAILABLE))?;

                let mut fwd = forward_to_upstream(
                    client,
                    &upstream_url,
                    upstream_headers,
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

                // On cache miss with safe response, store for future reuse.
                // Oversized responses (from forward_to_upstream returning error)
                // never reach this block since they return Err beforehand.
                if !bypass_safe_cache {
                    if let Some(cfg) = safe_cache_cfg {
                        if cfg.enabled && fwd.status >= 200 && fwd.status < 300 {
                            let verdict = safe_cache::inspect::inspect_response(&fwd.body_bytes);
                            if verdict.safe && (fwd.body_bytes.len() as u64) <= cfg.max_entry_bytes
                            {
                                // Check storage limits before writing. Run a
                                // lightweight cleanup pass first so we don't
                                // refuse if expired data is reclaimable.
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

                                        // max_entries check — allows replacement
                                        // of an existing (project,fingerprint) row.
                                        if let Some(max_entries) = storage_cfg.max_entries {
                                            match cache_db.count(None) {
                                                Ok(count) => {
                                                    if count >= max_entries {
                                                        let existing = cache_db.lookup(
                                                            &current_project_path(),
                                                            &fingerprint,
                                                        );
                                                        match existing {
                                                            Ok(Some(_)) => {
                                                                // replace existing entry
                                                            }
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

                                        // max_cas_bytes check — includes incoming response
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

                                        // min_free_disk_bytes check with incoming response
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
                                                            ) >= min_free =>
                                                        {
                                                            // OK
                                                        }
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
                                                    model: model.clone(),
                                                    status: fwd.status as i32,
                                                    tokens_input: input_tokens,
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
    let integration_name_clone = integration_name;
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

        // Complete shield entry so any waiters get the result
        if let Some(key) = shield_key_for_complete {
            coalesce_store.complete(
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
            profile_name: integration_name_clone,
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
            runtime_id: id_ctx_for_ledger.runtime_id.as_str().to_string(),
            request_id: id_ctx_for_ledger.request_id.as_str().to_string(),
            integration_id: id_ctx_for_ledger.integration_id,
            upstream_id: id_ctx_for_ledger.upstream_id,
            trust_domain_id: id_ctx_for_ledger.trust_domain_id.as_str().to_string(),
            config_snapshot_hash: id_ctx_for_ledger.config_snapshot_hash,
            attribution: id_ctx_for_ledger.attribution.to_string(),
            protocol: "anthropic".to_string(),
        };
        if let Err(e) = record_request(&db, &pricing, record) {
            error!("Failed to record to ledger: {e}");
        }
    });

    Ok(Sse::new(stream))
}

pub async fn responses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, axum::response::Response> {
    let timer = RequestTimer::start();
    let request_id = RequestId::new();

    // Read body with size limit
    let body_bytes =
        bounded_body::read_body_limited(&headers, body, state.max_request_body_bytes).await?;
    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    let resolved = state
        .default_integration
        .clone()
        .ok_or_else(|| status_response(StatusCode::INTERNAL_SERVER_ERROR))?;

    let protocol = OpenAiResponsesProtocol;

    let model = protocol.extract_model(&body_str);
    let integration_name = resolved.name.clone();
    let input_tokens = estimate_tokens(&body_str);

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

    let external_request_id = identity::extract_external_request_id(&headers);
    let conversation_id = identity::extract_conversation_id(&headers);

    let id_ctx = IdentityContext {
        runtime_id: state.runtime_id.clone(),
        request_id: request_id.clone(),
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

    info!(
        "Request {} (responses): integration={}, model={}",
        request_id, integration_name, model
    );

    let client = &state.http_client;

    let mut upstream_headers = HeaderMap::new();
    for (name, value) in &headers {
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

    let _is_streaming = protocol.is_streaming(&body_str);

    // Acquire a timed concurrency permit
    let _permit = tokio::time::timeout(
        std::time::Duration::from_millis(state.upstream_permit_timeout_ms),
        state.upstream_semaphore.acquire(),
    )
    .await
    .map_err(|_| {
        error!("Timed out waiting for concurrency permit (responses)");
        status_response(StatusCode::SERVICE_UNAVAILABLE)
    })?
    .map_err(|_| status_response(StatusCode::SERVICE_UNAVAILABLE))?;

    info!("Forwarding (responses) to upstream: {upstream_url}");

    let fwd = forward_to_upstream(
        client,
        &upstream_url,
        upstream_headers,
        body_str,
        &protocol,
        state.max_response_body_bytes,
    )
    .await?;

    let status_str = if fwd.status >= 200 && fwd.status < 300 {
        "success"
    } else {
        "error"
    };
    let latency_ms = timer.elapsed_ms();

    let body_bytes = fwd.body_bytes;
    let stream = futures::stream::once({
        let data = String::from_utf8_lossy(&body_bytes).to_string();
        async move { Ok(Event::default().data(data)) }
    });

    continuity::observer::observe_response(&body_bytes);

    let model_clone = model;
    let integration_name_clone = integration_name;
    let status_clone = status_str.to_string();
    let project_path = current_project_path();
    let id_ctx_for_ledger = id_ctx.clone();
    let db_path = state.storage_ledger_db.clone();
    tokio::spawn(async move {
        let db = match LedgerDb::open(&db_path) {
            Ok(db) => db,
            Err(e) => {
                error!("Failed to open ledger DB: {e}");
                return;
            }
        };
        let pricing = PricingMap::load_embedded();
        let output_tokens = estimate_tokens(&String::from_utf8_lossy(&body_bytes));

        let record = NewLedgerRecord {
            timestamp: chrono::Utc::now(),
            model: model_clone,
            profile_name: integration_name_clone,
            input_tokens,
            output_tokens,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            coalesced_count: 0,
            latency_ms,
            status: status_clone,
            cost: None,
            project_path,
            reduction_input_tokens: 0,
            reduction_output_tokens: 0,
            reduction_count: 0,
            efficiency_mode: String::new(),
            local_cache_hit: false,
            runtime_id: id_ctx_for_ledger.runtime_id.as_str().to_string(),
            request_id: id_ctx_for_ledger.request_id.as_str().to_string(),
            integration_id: id_ctx_for_ledger.integration_id,
            upstream_id: id_ctx_for_ledger.upstream_id,
            trust_domain_id: id_ctx_for_ledger.trust_domain_id.as_str().to_string(),
            config_snapshot_hash: id_ctx_for_ledger.config_snapshot_hash,
            attribution: id_ctx_for_ledger.attribution.to_string(),
            protocol: "openai-responses".to_string(),
        };
        if let Err(e) = record_request(&db, &pricing, record) {
            error!("Failed to record to ledger: {e}");
        }
    });

    Ok(Sse::new(stream))
}
