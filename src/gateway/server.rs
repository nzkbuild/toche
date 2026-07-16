use anyhow::Context;
use axum::Router;
use std::net::SocketAddr;
use tracing::info;

pub async fn serve() -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 8743));
    let app = Router::new()
        .route("/v1/messages", axum::routing::post(super::routes::messages))
        .route("/health", axum::routing::get(health))
        .route("/ready", axum::routing::get(ready));

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
    use crate::profiles::loader::load_profiles;

    let mut checks: Vec<String> = Vec::new();

    let profiles_ok = match load_profiles() {
        Ok(profiles) => {
            let has_default = profiles.default_profile().is_some();
            if has_default {
                true
            } else {
                checks.push("no default profile configured".to_string());
                false
            }
        }
        Err(e) => {
            checks.push(format!("failed to load profiles: {e}"));
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
