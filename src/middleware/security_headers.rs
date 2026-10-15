use axum::{
    extract::Request,
    http::{header, HeaderValue},
    middleware::Next,
    response::Response,
};

/// Adds defensive response headers to every response.
///
/// - `X-Content-Type-Options: nosniff` stops browsers from MIME-sniffing
///   user-uploaded content (served via the download/preview endpoints with an
///   uploader-supplied `Content-Type`) into an executable type.
/// - `Referrer-Policy` keeps full URLs (which can carry share codes or tokens
///   in the query string) from leaking to third-party origins.
///
/// Framing headers (`X-Frame-Options` / CSP `frame-ancestors`) are deliberately
/// not set here: the frontend may embed API-served previews in an iframe, and a
/// blanket deny would break that. Headers already set by a handler are kept.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers
        .entry(header::X_CONTENT_TYPE_OPTIONS)
        .or_insert(HeaderValue::from_static("nosniff"));
    headers
        .entry(header::REFERRER_POLICY)
        .or_insert(HeaderValue::from_static("strict-origin-when-cross-origin"));
    response
}
