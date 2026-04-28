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

const DEVICE_REVOKE_TYP: &str = "device_revoke";
const DEVICE_REVOKE_EXP_DAYS: i64 = 7;

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceRevokeClaims {
    pub sub: String,
    pub jti: String,
    pub device_id: String,
    pub typ: String,
    pub exp: usize,
    pub iat: usize,
}

pub fn issue_device_revoke_token(
    secret: &str,
    user_id: &str,
    jti: &str,
    device_id: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let exp = now + chrono::Duration::days(DEVICE_REVOKE_EXP_DAYS);
    let claims = DeviceRevokeClaims {
        sub: user_id.to_string(),
        jti: jti.to_string(),
        device_id: device_id.to_string(),
        typ: DEVICE_REVOKE_TYP.to_string(),
        exp: exp.timestamp() as usize,
        iat: now.timestamp() as usize,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
}

fn verify(token: &str, secret: &str) -> Option<DeviceRevokeClaims> {
    let key = DecodingKey::from_secret(secret.as_ref());
    let validation = Validation::default();
    let data = decode::<DeviceRevokeClaims>(token, &key, &validation).ok()?;
    if data.claims.typ != DEVICE_REVOKE_TYP {
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

pub async fn revoke_device(
    State(state): State<DeviceConfirmState>,
    Query(query): Query<ConfirmQuery>,
) -> Redirect {
    let frontend = &state.config.server.frontend_url;

    let claims = match verify(&query.token, &state.config.jwt.secret) {
        Some(c) => c,
        None => return Redirect::to(&format!("{}/auth/device/result?status=invalid", frontend)),
    };

    if let Err(e) = repository::delete_session(&state.db, &claims.sub, &claims.jti).await {
        tracing::error!(error = %e, "Failed to delete session for device revoke");
        return Redirect::to(&format!("{}/auth/device/result?status=error", frontend));
    }

    if let Err(e) =
        repository::delete_trusted_device_by_device_id(&state.db, &claims.sub, &claims.device_id)
            .await
    {
        tracing::error!(error = %e, "Failed to delete trusted device for revoke");
        return Redirect::to(&format!("{}/auth/device/result?status=error", frontend));
    }

    Redirect::to(&format!("{}/auth/device/result?status=revoked", frontend))
}
