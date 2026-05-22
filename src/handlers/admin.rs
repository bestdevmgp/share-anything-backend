use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use rand::Rng;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    db::{repository, DbPool},
    models::{
        api_key_application::{
            ApiKeyApplication, ApiKeyResponse, RejectRequest,
        },
        internal_error, not_found, unauthorized, AppError,
        personal_token::Scope,
    },
    services::{discord::DiscordNotifier, email::EmailService},
};

#[derive(Clone)]
pub struct AdminState {
    pub db: DbPool,
    pub email: Arc<EmailService>,
    #[allow(dead_code)]
    pub discord: Arc<DiscordNotifier>,
}

#[derive(Debug, serde::Deserialize)]
pub struct AdminListQuery {
    pub status: Option<String>,
}

fn verify_admin_password(headers: &HeaderMap) -> Result<(), AppError> {
    let expected = std::env::var("ADMIN_PASSWORD").unwrap_or_default();
    if expected.is_empty() {
        return Err(unauthorized("Admin password not configured"));
    }

    let provided = headers
        .get("X-Admin-Password")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Constant-time comparison using a simple XOR approach to avoid short-circuit
    if provided.len() != expected.len()
        || provided
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            != 0
    {
        return Err(unauthorized("Invalid admin password"));
    }

    Ok(())
}

fn generate_api_key() -> String {
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        .chars()
        .collect();
    let random_part: String = (0..40).map(|_| chars[rng.gen_range(0..chars.len())]).collect();
    format!("sk_{}", random_part)
}

/// List all API key applications
///
/// Returns all applications, optionally filtered by status (`pending`, `approved`, `rejected`).
/// Requires the `X-Admin-Password` header set to the value of the `ADMIN_PASSWORD` environment variable.
#[utoipa::path(
    get,
    path = "/admin/api-keys/applications",
    tag = "admin",
    params(
        ("status" = Option<String>, Query, description = "Filter by status: pending, approved, or rejected")
    ),
    responses(
        (status = 200, description = "List of all applications", body = Vec<ApiKeyApplication>),
        (status = 401, description = "Missing or wrong X-Admin-Password header"),
    ),
)]
pub async fn admin_list_applications(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(filter): Query<AdminListQuery>,
) -> Result<Json<Vec<ApiKeyApplication>>, AppError> {
    verify_admin_password(&headers)?;

    let apps = repository::list_applications_by_status(&state.db, filter.status.as_deref())
        .await
        .map_err(|e| internal_error(format!("Failed to list applications: {}", e)))?;

    Ok(Json(apps))
}

/// Approve an API key application
///
/// Issues a new API key (prefix `sk_`) tied to the application, sets the application status to `approved`,
/// and emails the applicant.
/// Requires the `X-Admin-Password` header set to the value of the `ADMIN_PASSWORD` environment variable.
#[utoipa::path(
    post,
    path = "/admin/api-keys/applications/{id}/approve",
    tag = "admin",
    params(("id" = i64, Path, description = "Application ID")),
    responses(
        (status = 200, description = "Approved; raw API key returned (shown once)", body = ApiKeyResponse),
        (status = 401, description = "Missing or wrong X-Admin-Password header"),
        (status = 404, description = "Application not found"),
        (status = 409, description = "Application already processed"),
    ),
)]
pub async fn admin_approve(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<ApiKeyResponse>, AppError> {
    verify_admin_password(&headers)?;

    let app = repository::find_application_by_id(&state.db, id)
        .await
        .map_err(|e| internal_error(format!("DB error: {}", e)))?
        .ok_or_else(|| not_found("신청을 찾을 수 없습니다"))?;

    if app.status != "pending" {
        return Err(AppError::Conflict("이미 처리된 신청입니다".to_string()));
    }

    let raw_key = generate_api_key();
    let key_prefix = raw_key[..8].to_string();

    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    let key_id = Uuid::new_v4().to_string();
    let key_name = format!("API Key for {}", app.service_name);

    repository::create_api_key(
        &state.db,
        &key_id,
        &app.user_id,
        id,
        &key_hash,
        &key_prefix,
        &key_name,
        None,
    )
    .await
    .map_err(|e| internal_error(format!("Failed to create API key: {}", e)))?;

    let scopes: Vec<Scope> = Scope::parse_list(&app.scopes);
    repository::insert_key_scopes(&state.db, &key_id, &scopes)
        .await
        .map_err(|e| internal_error(format!("Failed to insert key scopes: {}", e)))?;

    repository::approve_application(&state.db, id, &key_id)
        .await
        .map_err(|e| internal_error(format!("Failed to approve application: {}", e)))?;

    if let Ok(Some(user)) = repository::find_user_by_id(&state.db, &app.user_id).await {
        state
            .email
            .send_application_approved(&user.email, &user.name, &app.service_name);
    }

    let created_at = repository::find_application_by_id(&state.db, id)
        .await
        .ok()
        .flatten()
        .map(|a| a.created_at)
        .unwrap_or_else(chrono::Utc::now);

    Ok(Json(ApiKeyResponse {
        api_key: raw_key,
        key_prefix,
        name: key_name,
        created_at,
    }))
}

/// Reject an API key application
///
/// Sets the application status to `rejected` with a reason, and emails the applicant.
/// Requires the `X-Admin-Password` header set to the value of the `ADMIN_PASSWORD` environment variable.
#[utoipa::path(
    post,
    path = "/admin/api-keys/applications/{id}/reject",
    tag = "admin",
    params(("id" = i64, Path, description = "Application ID")),
    request_body = RejectRequest,
    responses(
        (status = 204, description = "Application rejected and applicant notified"),
        (status = 401, description = "Missing or wrong X-Admin-Password header"),
        (status = 404, description = "Application not found"),
        (status = 409, description = "Application already processed"),
    ),
)]
pub async fn admin_reject(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<RejectRequest>,
) -> Result<StatusCode, AppError> {
    verify_admin_password(&headers)?;

    let app = repository::find_application_by_id(&state.db, id)
        .await
        .map_err(|e| internal_error(format!("DB error: {}", e)))?
        .ok_or_else(|| not_found("신청을 찾을 수 없습니다"))?;

    if app.status != "pending" {
        return Err(AppError::Conflict("이미 처리된 신청입니다".to_string()));
    }

    repository::reject_application(&state.db, id, &body.reject_reason)
        .await
        .map_err(|e| internal_error(format!("Failed to reject application: {}", e)))?;

    if let Ok(Some(user)) = repository::find_user_by_id(&state.db, &app.user_id).await {
        state.email.send_application_rejected(
            &user.email,
            &user.name,
            &app.service_name,
            &body.reject_reason,
        );
    }

    Ok(StatusCode::NO_CONTENT)
}
