use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};
use tracing::Instrument;
use uuid::Uuid;

/// Generates a UUID per request, wraps the request in a tracing span containing it,
/// and echoes the ID back as `X-Request-ID` on the response so callers can correlate
/// log entries to a specific request.
pub async fn request_id_layer(req: Request, next: Next) -> Response {
    let id = Uuid::new_v4();
    let id_str = id.to_string();

    let span = tracing::info_span!("request", request_id = %id_str);

    async move {
        let mut response = next.run(req).await;
        if let Ok(val) = HeaderValue::from_str(&id_str) {
            response.headers_mut().insert("x-request-id", val);
        }
        response
    }
    .instrument(span)
    .await
}
