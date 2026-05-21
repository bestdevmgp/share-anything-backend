use axum::{
    extract::{Path, State},
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
        cli_auth::{CliAuthSessionResponse, CliAuthStatusResponse},
        bad_request, internal_error, not_found, AppError,
    },
};

#[derive(Clone)]
pub struct CliAuthHandlerState {
    pub db: DbPool,
    pub config: Arc<Config>,
}

fn generate_personal_token() -> String {
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        .chars()
        .collect();
    let random_part: String = (0..40).map(|_| chars[rng.gen_range(0..chars.len())]).collect();
    format!("sa_{}", random_part)
}

pub async fn create_session(
    State(state): State<CliAuthHandlerState>,
) -> Result<Json<CliAuthSessionResponse>, AppError> {
    let session_id = Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(10);

    repository::create_cli_auth_session(&state.db, &session_id, expires_at)
        .await
        .map_err(|e| internal_error(format!("CLI 인증 세션 생성 실패: {}", e)))?;

    let login_url = format!(
        "{}/cli-signin/{}",
        state.config.server.frontend_url, session_id
    );

    Ok(Json(CliAuthSessionResponse {
        session_id,
        login_url,
        expires_in_seconds: 600,
    }))
}

pub async fn check_status(
    State(state): State<CliAuthHandlerState>,
    Path(session_id): Path<String>,
) -> Result<Json<CliAuthStatusResponse>, AppError> {
    let session = repository::find_cli_auth_session(&state.db, &session_id)
        .await
        .map_err(|e| internal_error(format!("CLI 인증 세션 조회 실패: {}", e)))?
        .ok_or_else(|| not_found("CLI 인증 세션을 찾을 수 없습니다"))?;

    let (status, token_value, user_name, expires_at) = session;

    if expires_at < chrono::Utc::now() && status != "completed" {
        return Ok(Json(CliAuthStatusResponse {
            status: "expired".to_string(),
            personal_token: None,
            user_name: None,
        }));
    }

    if status == "completed" {
        let personal_token = if token_value.is_some() {
            let cleared = repository::clear_cli_auth_session_token(&state.db, &session_id)
                .await
                .map_err(|e| internal_error(format!("CLI 인증 토큰 삭제 실패: {}", e)))?;
            if cleared > 0 {
                token_value
            } else {
                None
            }
        } else {
            None
        };

        return Ok(Json(CliAuthStatusResponse {
            status: "completed".to_string(),
            personal_token,
            user_name,
        }));
    }

    Ok(Json(CliAuthStatusResponse {
        status: "pending".to_string(),
        personal_token: None,
        user_name: None,
    }))
}

pub async fn complete_session(
    State(state): State<CliAuthHandlerState>,
    claims: axum::extract::Extension<Claims>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = &claims.sub;

    let session = repository::find_cli_auth_session(&state.db, &session_id)
        .await
        .map_err(|e| internal_error(format!("CLI 인증 세션 조회 실패: {}", e)))?
        .ok_or_else(|| not_found("CLI 인증 세션을 찾을 수 없습니다"))?;

    let (status, _, _, expires_at) = session;

    if expires_at < chrono::Utc::now() {
        return Err(bad_request("CLI 인증 세션이 만료되었습니다"));
    }

    if status != "pending" {
        return Err(bad_request("CLI 인증 세션이 이미 처리되었습니다"));
    }

    let raw_token = generate_personal_token();
    let token_prefix = &raw_token[..8];

    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    let token_hash = hex::encode(hasher.finalize());

    let token_id = Uuid::new_v4().to_string();

    repository::create_personal_token(
        &state.db,
        &token_id,
        user_id,
        &token_hash,
        token_prefix,
        "CLI Token",
        "read,upload,delete",
        None,
    )
    .await
    .map_err(|e| internal_error(format!("Personal Token 생성 실패: {}", e)))?;

    let rows = repository::complete_cli_auth_session(
        &state.db,
        &session_id,
        user_id,
        &token_id,
        &raw_token,
    )
    .await
    .map_err(|e| internal_error(format!("CLI 인증 세션 완료 실패: {}", e)))?;

    if rows == 0 {
        return Err(bad_request("CLI 인증 세션을 완료할 수 없습니다"));
    }

    Ok(Json(serde_json::json!({
        "success": true
    })))
}
