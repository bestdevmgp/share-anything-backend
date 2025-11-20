use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::config::Config;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // User ID
    pub email: String,
    pub name: String,
    pub exp: usize, // Expiration time
    pub iat: usize, // Issued at
}

#[derive(Clone)]
pub struct AuthState {
    pub config: Arc<Config>,
}

/// Extracts user from JWT token (optional - doesn't fail if no token)
pub async fn optional_auth(
    State(state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Some(auth_header) = request.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                if let Ok(claims) = verify_jwt(token, &state.config.jwt.secret) {
                    request.extensions_mut().insert(claims);
                }
            }
        }
    }

    Ok(next.run(request).await)
}

/// Requires authentication - fails if no valid token
pub async fn require_auth(
    State(state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let auth_str = auth_header.to_str().map_err(|_| StatusCode::UNAUTHORIZED)?;

    let token = auth_str
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let claims = verify_jwt(token, &state.config.jwt.secret)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    request.extensions_mut().insert(claims);

    Ok(next.run(request).await)
}

/// Verify JWT token and extract claims
pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let decoding_key = DecodingKey::from_secret(secret.as_ref());
    let validation = Validation::default();

    let token_data = decode::<Claims>(token, &decoding_key, &validation)?;

    Ok(token_data.claims)
}

/// Create JWT token
pub fn create_jwt(
    user_id: &str,
    email: &str,
    name: &str,
    secret: &str,
    expiration_hours: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = chrono::Utc::now();
    let exp = now + chrono::Duration::hours(expiration_hours);

    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        name: name.to_string(),
        exp: exp.timestamp() as usize,
        iat: now.timestamp() as usize,
    };

    let encoding_key = jsonwebtoken::EncodingKey::from_secret(secret.as_ref());
    let header = jsonwebtoken::Header::default();

    jsonwebtoken::encode(&header, &claims, &encoding_key)
}
