use anyhow::Context;
use axum::Router;
use std::net::SocketAddr;
use tracing::info;

pub async fn serve() -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 8743));
    let app = Router::new()
        .route("/v1/messages", axum::routing::post(super::routes::messages));

    info!("Toche gateway listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind gateway address")?;

    axum::serve(listener, app)
        .await
        .context("Gateway server error")
}
