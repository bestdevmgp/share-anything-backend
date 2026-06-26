use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use crate::{
    config::Config,
    db::{repository, DbPool},
    models::AppError,
};

pub fn extract_jwt(headers: &HeaderMap) -> Option<String> {
    if let Some(auth_header) = headers.get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    crate::utils::auth_cookie::read_auth_cookie(headers)
}

fn auth_401(code: &str, message: &str, clear_cookie: bool) -> Response {
    let body = Json(json!({ "code": code, "message": message }));
    if clear_cookie {
        (
            StatusCode::UNAUTHORIZED,
            [(header::SET_COOKIE, crate::utils::auth_cookie::clear_auth_cookie())],
            body,
        )
            .into_response()
    } else {
        (StatusCode::UNAUTHORIZED, body).into_response()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub name: String,
    pub jti: String,
    pub exp: usize,
    pub iat: usize,
}

#[derive(Clone)]
pub struct AuthState {
    pub config: Arc<Config>,
    pub db: DbPool,
}

pub async fn optional_auth(
    State(state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if let Some(token) = extract_jwt(request.headers()) {
        if let Ok(claims) = verify_jwt(&token, &state.config.jwt.secret) {
            if session_active(&state.db, &claims.jti).await {
                spawn_touch_session(&state.db, &claims.jti);
                request.extensions_mut().insert(claims);
            }
        }
    }

    Ok(next.run(request).await)
}

pub async fn require_auth(
    State(state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = match extract_jwt(request.headers()) {
        Some(t) => t,
        None => return Ok(auth_401("AUTH_REQUIRED", "Authentication required", false)),
    };

    let claims = match verify_jwt(&token, &state.config.jwt.secret) {
        Ok(c) => c,
        Err(_) => {
            return Ok(auth_401(
                "AUTH_TOKEN_EXPIRED",
                "Invalid or expired authentication token",
                true,
            ))
        }
    };

    if !session_active(&state.db, &claims.jti).await {
        return Ok(auth_401(
            "AUTH_SESSION_REVOKED",
            "Session ended. Please sign in again.",
            true,
        ));
    }

    spawn_touch_session(&state.db, &claims.jti);
    request.extensions_mut().insert(claims);

    Ok(next.run(request).await)
}

async fn session_active(db: &DbPool, jti: &str) -> bool {
    matches!(repository::find_session(db, jti).await, Ok(Some(_)))
}

fn spawn_touch_session(db: &DbPool, jti: &str) {
    let db = db.clone();
    let jti = jti.to_string();
    tokio::spawn(async move {
        if let Err(e) = repository::touch_session_last_seen(&db, &jti).await {
            tracing::warn!(error = %e, "Failed to update session last_seen_at");
        }
    });
}

pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let decoding_key = DecodingKey::from_secret(secret.as_ref());
    let validation = Validation::default();

    let token_data = decode::<Claims>(token, &decoding_key, &validation)?;

    Ok(token_data.claims)
}

pub fn create_jwt(
    user_id: &str,
    email: &str,
    name: &str,
    jti: &str,
    secret: &str,
    expiration_hours: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let exp = now + chrono::Duration::hours(expiration_hours);

    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        name: name.to_string(),
        jti: jti.to_string(),
        exp: exp.timestamp() as usize,
        iat: now.timestamp() as usize,
    };

    let encoding_key = jsonwebtoken::EncodingKey::from_secret(secret.as_ref());
    let header = jsonwebtoken::Header::default();

    jsonwebtoken::encode(&header, &claims, &encoding_key)
}
