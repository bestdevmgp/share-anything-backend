//! Session token middleware.
//!
//! Verifies a short-lived JWT session token presented via the `X-Session-Token`
//! header. The token is minted at site entry by the `/auth/session-token`
//! endpoint after an invisible Cloudflare Turnstile check.
//!
//! # Middleware ordering
//!
//! This middleware MUST be applied AFTER the user-JWT auth middleware
//! (`super::auth::auth_middleware` or equivalent) on any route that supports
//! both authenticated and anonymous access. The auth middleware inserts
//! `auth::Claims` into the request extensions; this middleware then exempts
//! authenticated requests from session-token verification.
//!
//! On purely anonymous routes (no auth middleware in the chain) this
//! middleware behaves as the sole gate — `X-Session-Token` is mandatory.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    Json,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde_json::json;

use crate::{
    config::Config,
    db::DbPool,
    handlers::session_token::SessionTokenClaims,
    middleware::personal_token_auth::is_valid_personal_token,
};

const HEADER: &str = "X-Session-Token";

/// Validates a raw session-token JWT (signature, expiry, kind). Used by the
/// WebSocket upgrade, which carries the token as a query parameter.
pub fn validate_session_token(token: &str, secret: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let key = DecodingKey::from_secret(secret.as_bytes());
    match decode::<SessionTokenClaims>(token, &key, &Validation::default()) {
        Ok(decoded) => decoded.claims.kind == "session",
        Err(_) => false,
    }
}

pub async fn require_session_token(
    State((config, db)): State<(Arc<Config>, DbPool)>,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    if request.extensions().get::<crate::middleware::auth::Claims>().is_some() {
        return Ok(next.run(request).await);
    }

    if let Some(personal_token) = request
        .headers()
        .get("X-Personal-Token")
        .and_then(|v| v.to_str().ok())
    {
        if is_valid_personal_token(&db, personal_token).await {
            return Ok(next.run(request).await);
        }
    }

    let token = match request.headers().get(HEADER).and_then(|v| v.to_str().ok()) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "code": "SESSION_TOKEN_REQUIRED",
                    "message": "Session token required",
                })),
            ));
        }
    };

    let key = DecodingKey::from_secret(config.session_token.jwt_secret.as_bytes());
    let decoded = decode::<SessionTokenClaims>(&token, &key, &Validation::default())
        .map_err(|_| (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "code": "SESSION_TOKEN_EXPIRED",
                "message": "Session token invalid or expired",
            })),
        ))?;

    if decoded.claims.kind != "session" {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "code": "SESSION_TOKEN_INVALID",
                "message": "Wrong token kind",
            })),
        ));
    }

    Ok(next.run(request).await)
}
