use anyhow::Context;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

use crate::config::loader::{config_dir, load_config};
use crate::identity::{self, RuntimeId};

/// Application state shared across all request handlers.
#[derive(Clone)]
pub struct AppState {
    pub runtime_id: RuntimeId,
    pub config_snapshot_hash: String,
}

pub async fn serve() -> anyhow::Result<()> {
    let dir = config_dir();
    let runtime_id = RuntimeId::load_or_create(&dir);
    info!("Toche runtime id: {}", runtime_id);

    let config = load_config().context("Failed to load configuration")?;
    let config_toml = toml::to_string_pretty(&config).unwrap_or_default();
    let config_snapshot_hash = identity::compute_config_snapshot(&config_toml);

    let state = Arc::new(AppState {
        runtime_id,
        config_snapshot_hash,
    });

    let port = config.runtime.port;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let app = Router::new()
        .route("/v1/messages", axum::routing::post(super::routes::messages))
        .route("/health", axum::routing::get(health))
        .route("/ready", axum::routing::get(ready))
        .with_state(state);

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
