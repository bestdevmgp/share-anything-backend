use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::sync::Arc;

use crate::{
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{
        api_key::ApiKeyListItem,
        api_key_application::{ApplicationResponse, CreateApplicationRequest},
        bad_request, internal_error, not_found, AppError,
        personal_token::Scope,
    },
    services::{discord::DiscordNotifier, email::EmailService},
};

#[derive(Clone)]
pub struct ApiKeyState {
    pub db: DbPool,
    pub discord: Arc<DiscordNotifier>,
    #[allow(dead_code)]
    pub email: Arc<EmailService>,
    #[allow(dead_code)]
    pub frontend_url: String,
}

fn extract_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Forwarded-For")
        .or_else(|| headers.get("X-Real-IP"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
}

/// Submit an API key application
///
/// Creates a new application for an API key tied to a third-party service.
/// Blocked if there is already a pending application, or if one was already submitted today
/// (in the user's local calendar day as determined by `tz_offset_minutes`). Requires JWT authentication.
#[utoipa::path(
    post,
    path = "/user/api-keys/applications",
    tag = "api-keys",
    request_body = CreateApplicationRequest,
    responses(
        (status = 200, description = "Application submitted successfully", body = ApplicationResponse),
        (status = 400, description = "Validation error (invalid service name, URL, or purpose too short)"),
        (status = 401, description = "Unauthorized - authentication required"),
        (status = 409, description = "Conflict - already has a pending application under review"),
        (status = 429, description = "Rate limit exceeded - one application per calendar day"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn apply(
    State(state): State<ApiKeyState>,
    claims: axum::extract::Extension<Claims>,
    headers: HeaderMap,
    Json(req): Json<CreateApplicationRequest>,
) -> Result<Json<ApplicationResponse>, AppError> {
    if req.service_name.is_empty() || req.service_name.len() > 255 {
        return Err(bad_request("서비스 이름은 1~255자 이내여야 합니다"));
    }

    if !req.service_url.starts_with("http://") && !req.service_url.starts_with("https://") {
        return Err(bad_request("서비스 URL은 http:// 또는 https://로 시작해야 합니다"));
    }
    if req.service_url.len() > 512 {
        return Err(bad_request("서비스 URL은 512자 이내여야 합니다"));
    }

    if req.purpose.len() < 30 {
        return Err(bad_request("사용 목적은 30자 이상 입력해야 합니다"));
    }

    let scopes = req
        .scopes
        .unwrap_or_else(|| vec![Scope::Read, Scope::Upload, Scope::Delete]);
    let scopes_csv = Scope::format_list(&scopes);

    let has_pending = repository::check_user_pending_application(&state.db, &claims.sub)
        .await
        .map_err(|e| internal_error(format!("Pending check failed: {}", e)))?;

    if has_pending {
        return Err(AppError::Conflict(
            "이미 검토 중인 신청이 있습니다".to_string(),
        ));
    }

    let today_count = repository::count_user_applications_today(
        &state.db,
        &claims.sub,
        req.tz_offset_minutes,
    )
    .await
    .map_err(|e| internal_error(format!("Daily limit check failed: {}", e)))?;

    if today_count > 0 {
        return Err(AppError::TooManyRequests(
            "하루에 한 번만 신청할 수 있습니다".to_string(),
        ));
    }

    let ip = extract_ip(&headers);
    let platform = headers
        .get("User-Agent")
        .and_then(|v| v.to_str().ok())
        .map(crate::utils::parse_device_platform);

    let application = repository::create_application(
        &state.db,
        &claims.sub,
        &req.service_name,
        &req.service_url,
        &req.purpose,
        &scopes_csv,
        req.requested_expires_at,
        ip.as_deref(),
        platform.as_deref(),
    )
    .await
    .map_err(|e| internal_error(format!("Application creation failed: {}", e)))?;

    if let Ok(Some(user)) = repository::find_user_by_id(&state.db, &claims.sub).await {
        state
            .discord
            .notify_api_key_application(&application, &user.name, &user.email);
    }

    Ok(Json(application.into()))
}

/// List my API key applications
///
/// Returns all API key applications submitted by the authenticated user.
#[utoipa::path(
    get,
    path = "/user/api-keys/applications",
    tag = "api-keys",
    responses(
        (status = 200, description = "List of applications", body = Vec<ApplicationResponse>),
        (status = 401, description = "Unauthorized - authentication required"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn list_my_applications(
    State(state): State<ApiKeyState>,
    claims: axum::extract::Extension<Claims>,
) -> Result<Json<Vec<ApplicationResponse>>, AppError> {
    let apps = repository::find_applications_by_user(&state.db, &claims.sub)
        .await
        .map_err(|e| internal_error(format!("Failed to list applications: {}", e)))?;

    Ok(Json(apps.into_iter().map(ApplicationResponse::from).collect()))
}

/// Get a specific API key application
///
/// Returns details for a single application belonging to the authenticated user.
#[utoipa::path(
    get,
    path = "/user/api-keys/applications/{id}",
    tag = "api-keys",
    params(("id" = i64, Path, description = "Application ID")),
    responses(
        (status = 200, description = "Application details", body = ApplicationResponse),
        (status = 401, description = "Unauthorized - authentication required"),
        (status = 404, description = "Application not found or does not belong to you"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_my_application(
    State(state): State<ApiKeyState>,
    claims: axum::extract::Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<ApplicationResponse>, AppError> {
    let app = repository::find_application_by_id(&state.db, id)
        .await
        .map_err(|e| internal_error(format!("Failed to find application: {}", e)))?
        .ok_or_else(|| not_found("신청을 찾을 수 없습니다"))?;

    if app.user_id != claims.sub {
        return Err(not_found("신청을 찾을 수 없습니다"));
    }

    Ok(Json(app.into()))
}

/// List my active API keys
///
/// Returns all non-revoked API keys issued to the authenticated user.
#[utoipa::path(
    get,
    path = "/user/api-keys",
    tag = "api-keys",
    responses(
        (status = 200, description = "List of API keys (prefixes only; full key is never returned after issuance)", body = Vec<ApiKeyListItem>),
        (status = 401, description = "Unauthorized - authentication required"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn list_my_api_keys(
    State(state): State<ApiKeyState>,
    claims: axum::extract::Extension<Claims>,
) -> Result<Json<Vec<ApiKeyListItem>>, AppError> {
    let keys = repository::find_api_keys_by_user(&state.db, &claims.sub)
        .await
        .map_err(|e| internal_error(format!("Failed to list API keys: {}", e)))?;

    let mut response = Vec::with_capacity(keys.len());
    for key in keys {
        let scopes = repository::find_scopes_by_api_key(&state.db, &key.id)
            .await
            .map_err(|e| internal_error(format!("Failed to fetch scopes: {}", e)))?;
        response.push(ApiKeyListItem {
            id: key.id,
            key_prefix: key.key_prefix,
            name: key.name,
            scopes,
            last_used_at: key.last_used_at,
            expires_at: key.expires_at,
            created_at: key.created_at,
        });
    }

    Ok(Json(response))
}

/// Revoke an API key
///
/// Permanently revokes the specified API key belonging to the authenticated user.
/// The key cannot be recovered after revocation.
#[utoipa::path(
    delete,
    path = "/user/api-keys/{id}",
    tag = "api-keys",
    params(("id" = String, Path, description = "Token ID (UUID) of the API key to revoke")),
    responses(
        (status = 204, description = "API key revoked successfully"),
        (status = 401, description = "Unauthorized - authentication required"),
        (status = 404, description = "API key not found or does not belong to you"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn revoke_api_key(
    State(state): State<ApiKeyState>,
    claims: axum::extract::Extension<Claims>,
    Path(token_id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rows = repository::revoke_api_key(&state.db, &token_id, &claims.sub)
        .await
        .map_err(|e| internal_error(format!("API Key revocation failed: {}", e)))?;

    if rows == 0 {
        return Err(not_found("API Key를 찾을 수 없습니다"));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Cancel a pending API key application
///
/// Cancels the specified pending application belonging to the authenticated user.
/// Only applications in `pending` status can be cancelled. Returns 204 on success.
#[utoipa::path(
    delete,
    path = "/user/api-keys/applications/{id}",
    tag = "api-keys",
    params(("id" = i64, Path, description = "Application ID to cancel")),
    responses(
        (status = 204, description = "Application cancelled successfully"),
        (status = 401, description = "Unauthorized - authentication required"),
        (status = 404, description = "Application not found or does not belong to you"),
        (status = 409, description = "Application is not in pending status and cannot be cancelled"),
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn cancel_application(
    State(state): State<ApiKeyState>,
    claims: axum::extract::Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    repository::cancel_application_by_user(&state.db, id, &claims.sub).await?;
    Ok(StatusCode::NO_CONTENT)
}
