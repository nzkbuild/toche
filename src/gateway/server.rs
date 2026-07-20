use anyhow::Context;
use axum::Router;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use crate::config::loader::{config_dir, load_config, load_default_integration};
use crate::config::resolver::ResolvedIntegration;
use crate::identity::{self, RuntimeId};
use crate::shield;

/// Application state shared across all request handlers.
#[derive(Clone)]
pub struct AppState {
    pub runtime_id: RuntimeId,
    pub config_snapshot_hash: String,
    pub config_port: u16,
    pub request_timeout_ms: u64,
    pub default_integration: Option<ResolvedIntegration>,
    /// Absolute path to the Toche config directory, captured at build time
    /// so that background tasks don't read a stale TOCHE_CONFIG_DIR env var.
    pub config_dir: std::path::PathBuf,
    /// In-flight request coalescing belongs to this runtime only. Separate
    /// routers must not share active-flight state or coalesced responses.
    pub coalesce_store: Arc<shield::coalesce::CoalesceStore>,
}

/// Build the application router with full middleware stack.
///
/// When `config_dir_override` is `Some(dir)`, sets `TOCHE_CONFIG_DIR` to `dir`
/// before loading config. This allows integration tests to isolate config from
/// the real `~/.toche` directory.
pub fn build_router(config_dir_override: Option<PathBuf>) -> anyhow::Result<Router> {
    let dir = if let Some(override_dir) = config_dir_override {
        // SAFETY: set in test-only contexts with isolated temp dirs, never called concurrently
        unsafe { std::env::set_var("TOCHE_CONFIG_DIR", override_dir.as_os_str()) };
        override_dir
    } else {
        config_dir()
    };

    let runtime_id = RuntimeId::load_or_create(&dir);
    info!("Toche runtime id: {}", runtime_id);

    let config = load_config().context("Failed to load configuration")?;
    let config_toml = toml::to_string_pretty(&config).unwrap_or_default();
    let config_snapshot_hash = identity::compute_config_snapshot(&config_toml);

    let port = config.runtime.port;
    let request_timeout_ms = config.runtime.request_timeout_ms;
    let default_integration = load_default_integration().ok();
    let state = Arc::new(AppState {
        runtime_id,
        config_snapshot_hash,
        config_port: port,
        request_timeout_ms,
        default_integration,
        config_dir: dir.clone(),
        coalesce_store: Arc::new(shield::coalesce::CoalesceStore::new()),
    });

    Ok(Router::new()
        .route("/v1/messages", axum::routing::post(super::routes::messages))
        .route(
            "/v1/responses",
            axum::routing::post(super::routes::responses),
        )
        .route("/health", axum::routing::get(health))
        .route("/ready", axum::routing::get(ready))
        .route("/status", axum::routing::get(runtime_status))
        .with_state(state))
}

pub async fn serve() -> anyhow::Result<()> {
    let app = build_router(None)?;

    let config = load_config().context("Failed to load configuration")?;
    let port = config.runtime.port;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    info!("Toche gateway listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind gateway address")?;

    axum::serve(listener, app)
        .await
        .context("Gateway server error")
}

/// Simple liveness probe — returns 200 if the process is alive.
async fn health() -> &'static str {
    "ok"
}

/// Readiness probe — verifies the gateway can actually serve traffic.
/// Checks: profiles load, a default profile exists, and the upstream
/// URL is non-empty. Used by `toche connect` as a precondition gate.
async fn ready() -> axum::response::Json<serde_json::Value> {
    use crate::config::loader::load_default_integration;

    let mut checks: Vec<String> = Vec::new();

    let profiles_ok = match load_default_integration() {
        Ok(integration) => {
            if integration.upstream_url.is_empty() {
                checks.push("no default integration upstream configured".to_string());
                false
            } else {
                true
            }
        }
        Err(e) => {
            checks.push(format!("failed to load default integration: {e}"));
            false
        }
    };

    let all_ok = profiles_ok && checks.is_empty();
    let status = if all_ok { "ready" } else { "not ready" };

    axum::response::Json(serde_json::json!({
        "status": status,
        "checks": checks,
    }))
}

/// Full runtime status — live state including active flights, clients, and health.
async fn runtime_status(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::response::Json<serde_json::Value> {
    let flights = state.coalesce_store.active_flights();

    let mut flight_entries: Vec<serde_json::Value> = Vec::new();
    let mut protocol_counts: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    let mut integration_counts: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();

    for (key, waiter_count) in &flights {
        // Parse flight key: {url}|{fingerprint}|{trust_domain}|{policy_hash}
        let parts: Vec<&str> = key.split('|').collect();
        let url = parts.first().unwrap_or(&"unknown");
        let domain = parts.get(2).unwrap_or(&"unknown");

        flight_entries.push(serde_json::json!({
            "upstream_url": url,
            "trust_domain_hash": domain,
            "waiter_count": waiter_count.saturating_sub(1), // exclude leader
        }));
    }

    // Count by protocol from ledger
    let db_path = state.config_dir.join("ledger.db");
    if let Ok(db) = crate::meter::db::LedgerDb::open(&db_path) {
        if let Ok(entries) = db.get_entries(1000, None) {
            for e in &entries {
                *protocol_counts.entry(e.protocol.clone()).or_insert(0) += 1;
                *integration_counts
                    .entry(e.profile_name.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    let degraded: Vec<String> = Vec::new(); // future: check optional subsystems

    axum::response::Json(serde_json::json!({
        "runtime_id": state.runtime_id.as_str(),
        "config_snapshot_hash": state.config_snapshot_hash,
        "port": state.config_port,
        "active_flights": flights.len(),
        "flight_details": flight_entries,
        "protocol_counts": protocol_counts,
        "integration_counts": integration_counts,
        "degraded_systems": degraded,
        "schema_version": 11,
    }))
}
