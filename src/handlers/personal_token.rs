use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rand::Rng;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{
        personal_token::{CreatePersonalTokenRequest, CreatePersonalTokenResponse, PersonalTokenResponse},
        bad_request, internal_error, not_found, AppError,
    },
};

#[derive(Clone)]
pub struct PersonalTokenState {
    pub db: DbPool,
}

fn generate_personal_token() -> String {
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        .chars()
        .collect();
    let random_part: String = (0..40).map(|_| chars[rng.gen_range(0..chars.len())]).collect();
    format!("sat_{}", random_part)
}

/// Create a new personal access token for the authenticated user.
#[utoipa::path(
    post,
    path = "/user/personal-tokens",
    tag = "personal-tokens",
    request_body = CreatePersonalTokenRequest,
    responses(
        (status = 200, description = "Personal token created", body = CreatePersonalTokenResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_personal_token(
    State(state): State<PersonalTokenState>,
    claims: axum::extract::Extension<Claims>,
    Json(request): Json<CreatePersonalTokenRequest>,
) -> Result<Json<CreatePersonalTokenResponse>, AppError> {
    let user_id = &claims.sub;
    let name = request.name.unwrap_or_else(|| "API Token".to_string());

    if name.len() > 255 {
        return Err(bad_request("토큰 이름은 255자 이하여야 합니다"));
    }

    let raw_token = generate_personal_token();
    let token_prefix = &raw_token[..8];

    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    let token_hash = hex::encode(hasher.finalize());

    let expires_at = request
        .expires_in_days
        .map(|days| chrono::Utc::now() + chrono::Duration::days(days));

    let id = Uuid::new_v4().to_string();

    let personal_token = repository::create_personal_token(
        &state.db,
        &id,
        user_id,
        &token_hash,
        token_prefix,
        &name,
        expires_at,
    )
    .await
    .map_err(|e| internal_error(format!("Personal Token 생성 실패: {}", e)))?;

    Ok(Json(CreatePersonalTokenResponse {
        id: personal_token.id,
        personal_token: raw_token,
        token_prefix: personal_token.token_prefix,
        name: personal_token.name,
        expires_at: personal_token.expires_at,
        created_at: personal_token.created_at,
    }))
}

/// List all personal access tokens for the authenticated user.
#[utoipa::path(
    get,
    path = "/user/personal-tokens",
    tag = "personal-tokens",
    responses(
        (status = 200, description = "Personal tokens listed", body = Vec<PersonalTokenResponse>),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_personal_tokens(
    State(state): State<PersonalTokenState>,
    claims: axum::extract::Extension<Claims>,
) -> Result<Json<Vec<PersonalTokenResponse>>, AppError> {
    let tokens = repository::find_personal_tokens_by_user(&state.db, &claims.sub)
        .await
        .map_err(|e| internal_error(format!("Personal Token 목록 조회 실패: {}", e)))?;

    let response: Vec<PersonalTokenResponse> = tokens
        .into_iter()
        .map(|t| PersonalTokenResponse {
            id: t.id,
            token_prefix: t.token_prefix,
            name: t.name,
            last_used_at: t.last_used_at,
            expires_at: t.expires_at,
            created_at: t.created_at,
        })
        .collect();

    Ok(Json(response))
}

/// Revoke a personal access token by ID.
#[utoipa::path(
    delete,
    path = "/user/personal-tokens/{token_id}",
    tag = "personal-tokens",
    params(
        ("token_id" = String, Path, description = "Personal token ID")
    ),
    responses(
        (status = 204, description = "Token revoked"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Token not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_personal_token(
    State(state): State<PersonalTokenState>,
    claims: axum::extract::Extension<Claims>,
    Path(token_id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rows = repository::revoke_personal_token(&state.db, &token_id, &claims.sub)
        .await
        .map_err(|e| internal_error(format!("Personal Token 폐기 실패: {}", e)))?;

    if rows == 0 {
        return Err(not_found("Personal Token을 찾을 수 없습니다"));
    }

    Ok(StatusCode::NO_CONTENT)
}
