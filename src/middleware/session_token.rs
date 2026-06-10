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

use crate::{config::Config, handlers::session_token::SessionTokenClaims};

const HEADER: &str = "X-Session-Token";

pub async fn require_session_token(
    State(config): State<Arc<Config>>,
    request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    // If the request already carries a verified user JWT (from the auth middleware
    // chained before this one), exempt it from session-token verification.
    if request.extensions().get::<crate::middleware::auth::Claims>().is_some() {
        return Ok(next.run(request).await);
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
