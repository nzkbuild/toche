use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use std::convert::Infallible;

pub async fn messages(
    _headers: axum::http::HeaderMap,
    _body: String,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let stream = futures::stream::once(async {
        Ok(Event::default().data("Toche gateway — not yet connected to upstream"))
    });
    Ok(Sse::new(stream))
}
