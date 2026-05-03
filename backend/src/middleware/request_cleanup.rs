use axum::{extract::Request, middleware::Next, response::Response};

pub async fn release_request_caches(request: Request, next: Next) -> Response {
    next.run(request).await
}