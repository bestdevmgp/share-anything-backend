use axum::{extract::State, Json};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::{
    config::Config,
    models::{bad_request, forbidden, internal_error, AppError},
    utils::{origin_to_host, verify_turnstile_token},
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
    let client_ip = crate::utils::client_ip(&headers);
    let allowed_hostnames: Vec<String> = state
        .config
        .cors
        .allowed_origins
        .iter()
        .filter_map(|o| origin_to_host(o))
        .collect();
    let primary = verify_turnstile_token(
        &state.config.turnstile.secret_key,
        &req.turnstile_token,
        Some(client_ip.clone()),
        &allowed_hostnames,
        Some("session"),
    )
    .await;
    let verified = match primary {
        Ok(()) => Ok(()),
        Err(primary_err) => {
            let interactive_key = &state.config.turnstile.interactive_secret_key;
            if interactive_key.is_empty() {
                Err(primary_err)
            } else {
                verify_turnstile_token(
                    interactive_key,
                    &req.turnstile_token,
                    Some(client_ip),
                    &allowed_hostnames,
                    Some("session"),
                )
                .await
                .map_err(|fallback_err| format!("{} / {}", primary_err, fallback_err))
            }
        }
    };
    verified.map_err(|e| {
        tracing::warn!("Turnstile verification failed: {}", e);
        forbidden("Turnstile verification failed")
    })?;

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
