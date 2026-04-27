use axum::{
    extract::{Query, State},
    response::Redirect,
};
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    config::Config,
    db::{repository, DbPool},
};

const DEVICE_CONFIRM_TYP: &str = "device_confirm";
const DEVICE_CONFIRM_EXP_DAYS: i64 = 7;

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceConfirmClaims {
    pub sub: String,
    pub jti: String,
    pub ua_hash: String,
    pub ip: String,
    pub dev: Option<String>,
    pub typ: String,
    pub exp: usize,
    pub iat: usize,
}

pub fn issue_device_confirm_token(
    secret: &str,
    user_id: &str,
    jti: &str,
    user_agent_hash: &str,
    ip_address: &str,
    device_label: Option<&str>,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let exp = now + chrono::Duration::days(DEVICE_CONFIRM_EXP_DAYS);
    let claims = DeviceConfirmClaims {
        sub: user_id.to_string(),
        jti: jti.to_string(),
        ua_hash: user_agent_hash.to_string(),
        ip: ip_address.to_string(),
        dev: device_label.map(|s| s.to_string()),
        typ: DEVICE_CONFIRM_TYP.to_string(),
        exp: exp.timestamp() as usize,
        iat: now.timestamp() as usize,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
}

fn verify(token: &str, secret: &str) -> Option<DeviceConfirmClaims> {
    let key = DecodingKey::from_secret(secret.as_ref());
    let validation = Validation::default();
    let data = decode::<DeviceConfirmClaims>(token, &key, &validation).ok()?;
    if data.claims.typ != DEVICE_CONFIRM_TYP {
        return None;
    }
    Some(data.claims)
}

#[derive(Clone)]
pub struct DeviceConfirmState {
    pub config: Arc<Config>,
    pub db: DbPool,
}

#[derive(Debug, Deserialize)]
pub struct ConfirmQuery {
    pub token: String,
}

pub async fn trust_device(
    State(state): State<DeviceConfirmState>,
    Query(query): Query<ConfirmQuery>,
) -> Redirect {
    let frontend = &state.config.server.frontend_url;

    let claims = match verify(&query.token, &state.config.jwt.secret) {
        Some(c) => c,
        None => return Redirect::to(&format!("{}/auth/device/result?status=invalid", frontend)),
    };

    if let Err(e) = repository::add_trusted_device(
        &state.db,
        &claims.sub,
        &claims.ua_hash,
        None,
        &claims.ip,
        claims.dev.as_deref(),
    )
    .await
    {
        tracing::error!(error = %e, "Failed to add trusted device");
        return Redirect::to(&format!("{}/auth/device/result?status=error", frontend));
    }

    Redirect::to(&format!("{}/auth/device/result?status=trusted", frontend))
}

pub async fn terminate_device(
    State(state): State<DeviceConfirmState>,
    Query(query): Query<ConfirmQuery>,
) -> Redirect {
    let frontend = &state.config.server.frontend_url;

    let claims = match verify(&query.token, &state.config.jwt.secret) {
        Some(c) => c,
        None => return Redirect::to(&format!("{}/auth/device/result?status=invalid", frontend)),
    };

    if let Err(e) = repository::delete_session(&state.db, &claims.sub, &claims.jti).await {
        tracing::error!(error = %e, "Failed to delete session for device terminate");
        return Redirect::to(&format!("{}/auth/device/result?status=error", frontend));
    }

    Redirect::to(&format!("{}/auth/device/result?status=terminated", frontend))
}
