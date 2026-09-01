use axum::{
    extract::Request,
    http::{header, HeaderValue},
    middleware::Next,
    response::Response,
};

/// Cache policy for the two bundled single-page apps.
///
/// The entry point has to be revalidated on every load. A browser that keeps
/// serving a cached `index.html` after an upgrade goes on asking for the
/// previous release's asset names, so a new version can sit installed on the
/// server and never appear — which is also how a stale service worker keeps a
/// user on the old UI indefinitely.
///
/// Vite hashes every filename it writes into `assets/`, so those can be kept
/// for a year. Everything else in the dist root — `sw.js`, the manifest, the
/// icons — keeps a stable name across releases and must revalidate too;
/// long-caching `sw.js` in particular would defeat the update path entirely.
pub async fn static_cache_headers(request: Request, next: Next) -> Response {
    let hashed_asset = request.uri().path().starts_with("/assets/");
    let mut response = next.run(request).await;
    let policy = if hashed_asset {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(policy));
    response
}
