use axum::{extract::State, Json};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::{
    config::Config,
    models::{bad_request, forbidden, internal_error, AppError},
    utils::{extract_client_ip, verify_turnstile_token},
};

#[derive(Clone)]
pub struct SessionTokenState {
    pub config: Arc<Config>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ExchangeRequest {
    pub turnstile_token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExchangeResponse {
    pub session_token: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionTokenClaims {
    pub kind: String,
    pub iat: i64,
    pub exp: i64,
}

#[utoipa::path(
    post,
    path = "/auth/session-token",
    tag = "auth",
    request_body = ExchangeRequest,
    responses(
        (status = 200, description = "Session token issued", body = ExchangeResponse),
        (status = 400, description = "Missing turnstile token"),
        (status = 403, description = "Turnstile verification failed"),
    )
)]
pub async fn exchange_session_token(
    State(state): State<SessionTokenState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ExchangeRequest>,
) -> Result<Json<ExchangeResponse>, AppError> {
    if req.turnstile_token.is_empty() {
        return Err(bad_request("turnstile_token is required"));
    }
    let client_ip = extract_client_ip(&headers);
    verify_turnstile_token(
        &state.config.turnstile.secret_key,
        &req.turnstile_token,
        Some(client_ip),
    )
    .await
    .map_err(|e| forbidden(format!("Turnstile verification failed: {}", e)))?;

    let now = Utc::now();
    let exp = now + Duration::seconds(state.config.session_token.ttl_seconds);
    let claims = SessionTokenClaims {
        kind: "session".to_string(),
        iat: now.timestamp(),
        exp: exp.timestamp(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.session_token.jwt_secret.as_bytes()),
    )
    .map_err(|e| internal_error(format!("JWT encode failed: {}", e)))?;

    Ok(Json(ExchangeResponse {
        session_token: token,
        expires_at: exp.to_rfc3339(),
    }))
}
