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

/// Create a CLI device-pairing session and return a browser login URL.
#[utoipa::path(
    post,
    path = "/cli/auth/session",
    tag = "cli-auth",
    responses(
        (status = 200, description = "CLI auth session created", body = CliAuthSessionResponse)
    )
)]
pub async fn create_session(
    State(state): State<CliAuthHandlerState>,
) -> Result<Json<CliAuthSessionResponse>, AppError> {
    let session_id = Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(10);

    repository::create_cli_auth_session(&state.db, &session_id, expires_at)
        .await
        .map_err(|e| internal_error(format!("Failed to create CLI auth session: {}", e)))?;

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

/// Poll the status of a CLI auth session.
///
/// Returns `pending`, `completed` (with a one-time personal token), or `expired`.
#[utoipa::path(
    get,
    path = "/cli/auth/session/{session_id}/status",
    tag = "cli-auth",
    params(
        ("session_id" = String, Path, description = "CLI auth session ID")
    ),
    responses(
        (status = 200, description = "Session status", body = CliAuthStatusResponse),
        (status = 404, description = "Session not found")
    )
)]
pub async fn check_status(
    State(state): State<CliAuthHandlerState>,
    Path(session_id): Path<String>,
) -> Result<Json<CliAuthStatusResponse>, AppError> {
    let session = repository::find_cli_auth_session(&state.db, &session_id)
        .await
        .map_err(|e| internal_error(format!("Failed to look up CLI auth session: {}", e)))?
        .ok_or_else(|| not_found("CLI auth session not found"))?;

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
                .map_err(|e| internal_error(format!("Failed to clear CLI auth token: {}", e)))?;
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

/// Complete a CLI auth session from the browser (requires JWT authentication).
///
/// Issues a personal token linked to the authenticated user and marks the session complete.
#[utoipa::path(
    post,
    path = "/cli/auth/session/{session_id}/complete",
    tag = "cli-auth",
    params(
        ("session_id" = String, Path, description = "CLI auth session ID")
    ),
    responses(
        (status = 200, description = "Session completed successfully"),
        (status = 400, description = "Session expired or already completed"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Session not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn complete_session(
    State(state): State<CliAuthHandlerState>,
    claims: axum::extract::Extension<Claims>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = &claims.sub;

    let session = repository::find_cli_auth_session(&state.db, &session_id)
        .await
        .map_err(|e| internal_error(format!("Failed to look up CLI auth session: {}", e)))?
        .ok_or_else(|| not_found("CLI auth session not found"))?;

    let (status, _, _, expires_at) = session;

    if expires_at < chrono::Utc::now() {
        return Err(bad_request("CLI auth session has expired"));
    }

    if status != "pending" {
        return Err(bad_request("CLI auth session has already been processed"));
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
        None,
    )
    .await
    .map_err(|e| internal_error(format!("Failed to create personal token: {}", e)))?;

    let rows = repository::complete_cli_auth_session(
        &state.db,
        &session_id,
        user_id,
        &token_id,
        &raw_token,
    )
    .await
    .map_err(|e| internal_error(format!("Failed to complete CLI auth session: {}", e)))?;

    if rows == 0 {
        return Err(bad_request("Failed to complete CLI auth session"));
    }

    Ok(Json(serde_json::json!({
        "success": true
    })))
}
