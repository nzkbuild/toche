use std::convert::Infallible;

use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use futures::stream::StreamExt;
use futures::stream::Stream;
use reqwest::Client;
use tracing::{error, info};

use crate::profiles::loader::load_profiles;

pub async fn messages(
    headers: HeaderMap,
    body: String,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let profiles = load_profiles().map_err(|e| {
        error!("Failed to load profiles: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let profile = profiles.default_profile().ok_or_else(|| {
        error!("No default profile configured");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

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
        .body(body)
        .send()
        .await
        .map_err(|e| {
            error!("Upstream request failed: {e}");
            StatusCode::BAD_GATEWAY
        })?;

    let status = response.status();
    if !status.is_success() {
        error!("Upstream returned {status}");
        return Err(StatusCode::BAD_GATEWAY);
    }

    let stream = response.bytes_stream().map(|result| match result {
        Ok(ref bytes) => {
            Ok(Event::default().data(String::from_utf8_lossy(bytes).to_string()))
        }
        Err(e) => {
            error!("Stream error: {e}");
            Ok(Event::default().data(format!("Stream error: {e}")))
        }
    });

    Ok(Sse::new(stream))
}
