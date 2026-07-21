use axum::body::{Bytes, HttpBody};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

/// Plain-text 413 response used when the request body exceeds the limit.
fn payload_too_large() -> Response {
    (StatusCode::PAYLOAD_TOO_LARGE, "413 Payload Too Large").into_response()
}

/// Asynchronously read the full body into `Bytes`, capping at `max_bytes`.
///
/// Rejects requests whose `Content-Length` exceeds `max_bytes`, and
/// accumulates chunked bodies while checking the limit before the body
/// is passed to JSON or String parsing.
pub async fn read_body_limited(
    headers: &HeaderMap,
    body: axum::body::Body,
    max_bytes: u64,
) -> Result<Bytes, Response> {
    // Pre-check Content-Length
    if let Some(cl) = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        if cl > max_bytes {
            return Err(payload_too_large());
        }
    }

    let mut buf: Vec<u8> = Vec::new();
    let mut body = std::pin::pin!(body);

    while let Some(frame_result) = futures::future::poll_fn(|cx| body.as_mut().poll_frame(cx)).await
    {
        let frame = frame_result.map_err(|_| StatusCode::BAD_REQUEST.into_response())?;

        if let Ok(data) = frame.into_data() {
            let chunk_len = data.len() as u64;
            if buf.len() as u64 + chunk_len > max_bytes {
                return Err(payload_too_large());
            }
            buf.extend_from_slice(&data);
        }
    }

    Ok(Bytes::from(buf))
}
