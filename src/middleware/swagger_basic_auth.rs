use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};
use base64::Engine;
use sha2::{Digest, Sha256};

/// Constant-time equality for two byte slices. Hashing both sides first means
/// the comparison runs over fixed-length digests, so neither the length nor the
/// content of the secret leaks through timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let a_hash = Sha256::digest(a);
    let b_hash = Sha256::digest(b);
    a_hash
        .iter()
        .zip(b_hash.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

pub async fn swagger_basic_auth(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if !path.starts_with("/swagger-ui") && !path.starts_with("/api-docs/") {
        return next.run(request).await;
    }

    let username = std::env::var("SWAGGER_USERNAME").unwrap_or_default();
    let password = std::env::var("SWAGGER_PASSWORD").unwrap_or_default();
    if username.is_empty() || password.is_empty() {
        return basic_auth_challenge();
    }

    let expected = format!("{}:{}", username, password);
    let expected_b64 = base64::engine::general_purpose::STANDARD.encode(&expected);

    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .unwrap_or("");

    if !constant_time_eq(provided.as_bytes(), expected_b64.as_bytes()) {
        return basic_auth_challenge();
    }

    next.run(request).await
}

fn basic_auth_challenge() -> Response {
    let mut res = Response::new(Body::from("Unauthorized"));
    *res.status_mut() = StatusCode::UNAUTHORIZED;
    res.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"Swagger\""),
    );
    res
}
