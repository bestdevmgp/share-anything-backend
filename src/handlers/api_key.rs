use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rand::Rng;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{
        api_key::{ApiKeyResponse, CreateApiKeyRequest, CreateApiKeyResponse},
        bad_request, internal_error, not_found, ErrorResponse,
    },
};

#[derive(Clone)]
pub struct ApiKeyState {
    pub config: Arc<Config>,
    pub db: DbPool,
}

fn generate_api_key() -> String {
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        .chars()
        .collect();
    let random_part: String = (0..40).map(|_| chars[rng.gen_range(0..chars.len())]).collect();
    format!("sa_{}", random_part)
}

pub async fn create_api_key(
    State(state): State<ApiKeyState>,
    claims: axum::extract::Extension<Claims>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<Json<CreateApiKeyResponse>, (StatusCode, Json<ErrorResponse>)> {
    let user_id = &claims.sub;
    let name = request.name.unwrap_or_else(|| "CLI Key".to_string());

    if name.len() > 255 {
        return Err(bad_request("키 이름은 255자 이하여야 합니다"));
    }

    let raw_key = generate_api_key();
    let key_prefix = &raw_key[..8];

    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    let expires_at = request.expires_in_days.map(|days| {
        chrono::Utc::now() + chrono::Duration::days(days)
    });

    let id = Uuid::new_v4().to_string();

    let api_key = repository::create_api_key(
        &state.db,
        &id,
        user_id,
        &key_hash,
        key_prefix,
        &name,
        expires_at,
    )
    .await
    .map_err(|e| internal_error(format!("API 키 생성 실패: {}", e)))?;

    Ok(Json(CreateApiKeyResponse {
        id: api_key.id,
        api_key: raw_key,
        key_prefix: api_key.key_prefix,
        name: api_key.name,
        expires_at: api_key.expires_at,
        created_at: api_key.created_at,
    }))
}

pub async fn list_api_keys(
    State(state): State<ApiKeyState>,
    claims: axum::extract::Extension<Claims>,
) -> Result<Json<Vec<ApiKeyResponse>>, (StatusCode, Json<ErrorResponse>)> {
    let keys = repository::find_api_keys_by_user(&state.db, &claims.sub)
        .await
        .map_err(|e| internal_error(format!("API 키 목록 조회 실패: {}", e)))?;

    let response: Vec<ApiKeyResponse> = keys
        .into_iter()
        .map(|k| ApiKeyResponse {
            id: k.id,
            key_prefix: k.key_prefix,
            name: k.name,
            last_used_at: k.last_used_at,
            expires_at: k.expires_at,
            created_at: k.created_at,
        })
        .collect();

    Ok(Json(response))
}

pub async fn delete_api_key(
    State(state): State<ApiKeyState>,
    claims: axum::extract::Extension<Claims>,
    Path(key_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let rows = repository::revoke_api_key(&state.db, &key_id, &claims.sub)
        .await
        .map_err(|e| internal_error(format!("API 키 폐기 실패: {}", e)))?;

    if rows == 0 {
        return Err(not_found("API 키를 찾을 수 없습니다"));
    }

    Ok(StatusCode::NO_CONTENT)
}
