use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{
        unauthorized, forbidden, not_found, internal_error, bad_request, AppError,
        DownloadLogResponse, FileShareResponse, FileShareWithStats,
        PaginationQuery, UploadHistoryResponse, NotificationSettingsResponse,
        UpdateNotificationSettingsRequest, UpdateNameRequest, UpdateNameResponse,
    },
    services::{generate_qr_code, StorageService},
};

#[derive(Clone)]
pub struct UserState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

#[utoipa::path(
    get,
    path = "/user/uploads",
    tag = "user",
    params(
        ("limit" = Option<i64>, Query, description = "Number of items to return (default: 20)"),
        ("offset" = Option<i64>, Query, description = "Number of items to skip (default: 0)")
    ),
    responses(
        (status = 200, description = "Upload history retrieved successfully", body = UploadHistoryResponse),
        (status = 401, description = "Unauthorized - authentication required")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_upload_history(
    State(state): State<UserState>,
    Query(pagination): Query<PaginationQuery>,
    request: Request,
) -> Result<Json<UploadHistoryResponse>, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?;

    let mut rows = repository::find_file_shares_with_download_count_by_user(
        &state.db,
        &user_claims.sub,
        pagination.limit,
        pagination.offset,
    )
    .await?;

    let qa_rows = repository::find_active_qa_grant_shares_with_download_count_by_user(
        &state.db,
        &user_claims.sub,
    )
    .await?;
    rows.extend(qa_rows);

    let mut items: Vec<FileShareWithStats> = rows
        .into_iter()
        .map(|(file_share, download_count)| {
            let download_url = format!(
                "{}/download?code={}",
                state.config.server.base_url, file_share.share_code
            );
            let qr_code = generate_qr_code(&download_url).ok();

            FileShareWithStats {
                file_share: FileShareResponse {
                    id: file_share.id,
                    share_code: file_share.share_code,
                    file_name: file_share.file_name,
                    file_size: file_share.file_size,
                    file_type: file_share.file_type,
                    transfer_type: file_share.transfer_type,
                    description: file_share.description,
                    relative_path: file_share.relative_path,
                    has_password: file_share.password_hash.is_some(),
                    is_one_time: file_share.is_one_time,
                    expires_at: file_share.expires_at,
                    created_at: file_share.created_at,
                    download_url,
                    qr_code,
                    uploader_online: None,
                },
                download_count,
            }
        })
        .collect();

    items.sort_by(|a, b| b.file_share.created_at.cmp(&a.file_share.created_at));

    let total = items.len();

    Ok(Json(UploadHistoryResponse {
        items,
        total,
        limit: pagination.limit,
        offset: pagination.offset,
    }))
}

#[utoipa::path(
    get,
    path = "/user/uploads/{file_id}/downloads",
    tag = "user",
    params(
        ("file_id" = String, Path, description = "File share ID")
    ),
    responses(
        (status = 200, description = "Download logs retrieved successfully", body = Vec<DownloadLogResponse>),
        (status = 401, description = "Unauthorized - authentication required"),
        (status = 403, description = "Forbidden - file does not belong to user"),
        (status = 404, description = "File not found")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_download_logs(
    State(state): State<UserState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<Json<Vec<DownloadLogResponse>>, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?;

    let file_share = repository::find_file_share_by_id(&state.db, &file_id)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?
        .ok_or_else(|| not_found("File not found"))?;

    if file_share.user_id.as_ref() != Some(&user_claims.sub) {
        return Err(forbidden("Access to another user's file is forbidden"));
    }

    let rows = repository::find_download_logs_with_downloader_name_by_file_share(
        &state.db,
        &file_id,
    )
    .await?;

    let response: Vec<DownloadLogResponse> = rows
        .into_iter()
        .map(|(log, downloader_name)| DownloadLogResponse {
            id: log.id,
            downloader_name,
            ip_address: log.ip_address,
            device_platform: log.device_platform.unwrap_or_else(|| "Unknown".to_string()),
            downloaded_at: log.downloaded_at,
        })
        .collect();

    Ok(Json(response))
}

#[utoipa::path(
    delete,
    path = "/user/uploads/{file_id}",
    tag = "user",
    params(
        ("file_id" = String, Path, description = "File share ID")
    ),
    responses(
        (status = 204, description = "File deleted successfully"),
        (status = 401, description = "Unauthorized - authentication required"),
        (status = 403, description = "Forbidden - file does not belong to user"),
        (status = 404, description = "File not found")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_file_share(
    State(state): State<UserState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<StatusCode, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?;

    let file_share = repository::find_file_share_by_id(&state.db, &file_id)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?
        .ok_or_else(|| not_found("File not found"))?;

    if file_share.user_id.as_ref() != Some(&user_claims.sub) {
        return Err(forbidden("Access to another user's file is forbidden"));
    }

    if !file_share.storage_key.is_empty() {
        state
            .storage
            .delete_file(&file_share.storage_key)
            .await
            .map_err(|e| internal_error(format!("Failed to delete file from storage: {}", e)))?;
    }

    repository::delete_file_share(&state.db, &file_id)
        .await
        .map_err(|e| internal_error(format!("Failed to delete file record: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/user/uploads",
    tag = "user",
    responses(
        (status = 204, description = "All files deleted successfully"),
        (status = 401, description = "Unauthorized - authentication required")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_all_file_shares(
    State(state): State<UserState>,
    request: Request,
) -> Result<StatusCode, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?;

    let storage_keys = repository::delete_all_user_file_shares(&state.db, &user_claims.sub)
        .await
        .map_err(|e| internal_error(format!("Failed to delete all files: {}", e)))?;

    if !storage_keys.is_empty() {
        state
            .storage
            .delete_files(storage_keys)
            .await
            .map_err(|e| internal_error(format!("Failed to delete file from storage: {}", e)))?;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Update the authenticated user's display name.
#[utoipa::path(
    put,
    path = "/user/name",
    tag = "user",
    request_body = UpdateNameRequest,
    responses(
        (status = 200, description = "Name updated successfully", body = UpdateNameResponse),
        (status = 400, description = "Invalid name (empty or too long)"),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_name(
    State(state): State<UserState>,
    request: Request,
) -> Result<Json<UpdateNameResponse>, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?
        .clone();

    let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| internal_error("Cannot read request body"))?;

    let req: UpdateNameRequest = serde_json::from_slice(&body_bytes)
        .map_err(|_| internal_error("Invalid request format"))?;

    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 50 {
        return Err(bad_request("Name must be between 1 and 50 characters"));
    }

    repository::update_user_name(&state.db, &user_claims.sub, &name)
        .await
        .map_err(|e| internal_error(format!("Failed to update name: {}", e)))?;

    Ok(Json(UpdateNameResponse { name }))
}

/// Delete the authenticated user's account (soft delete).
#[utoipa::path(
    delete,
    path = "/user/account",
    tag = "user",
    responses(
        (status = 204, description = "Account deleted successfully"),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_account(
    State(state): State<UserState>,
    request: Request,
) -> Result<StatusCode, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?;

    repository::revoke_all_personal_tokens(&state.db, &user_claims.sub)
        .await
        .map_err(|e| internal_error(format!("Failed to revoke tokens: {}", e)))?;

    repository::soft_delete_user(&state.db, &user_claims.sub)
        .await
        .map_err(|e| internal_error(format!("Failed to delete account: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Get the authenticated user's notification settings.
#[utoipa::path(
    get,
    path = "/user/settings",
    tag = "user",
    responses(
        (status = 200, description = "Notification settings retrieved", body = NotificationSettingsResponse),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_notification_settings(
    State(state): State<UserState>,
    request: Request,
) -> Result<Json<NotificationSettingsResponse>, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?;

    let (notify_upload, notify_download, notify_download_alert, notify_security, notify_language, default_expiration) =
        repository::get_user_notification_settings(&state.db, &user_claims.sub)
            .await
            .map_err(|e| internal_error(format!("Failed to fetch notification settings: {}", e)))?;

    Ok(Json(NotificationSettingsResponse {
        notify_upload,
        notify_download,
        notify_download_alert,
        notify_security,
        notify_language,
        default_expiration,
    }))
}

/// Update the authenticated user's notification settings.
#[utoipa::path(
    put,
    path = "/user/settings",
    tag = "user",
    request_body = UpdateNotificationSettingsRequest,
    responses(
        (status = 200, description = "Notification settings updated", body = NotificationSettingsResponse),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_notification_settings(
    State(state): State<UserState>,
    request: Request,
) -> Result<Json<NotificationSettingsResponse>, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?
        .clone();

    let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| internal_error("Cannot read request body"))?;

    let req: UpdateNotificationSettingsRequest = serde_json::from_slice(&body_bytes)
        .map_err(|_| internal_error("Invalid request format"))?;

    const ALLOWED_EXPIRATIONS: &[&str] = &[
        "five_minutes", "thirty_minutes", "one_hour", "three_hours",
        "six_hours", "twelve_hours", "twenty_four_hours",
    ];
    if !ALLOWED_EXPIRATIONS.contains(&req.default_expiration.as_str()) {
        return Err(bad_request("Invalid default_expiration value"));
    }

    repository::update_user_notification_settings(
        &state.db,
        &user_claims.sub,
        req.notify_upload,
        req.notify_download,
        req.notify_download_alert,
        req.notify_security,
        &req.notify_language,
        &req.default_expiration,
    )
    .await
    .map_err(|e| internal_error(format!("Failed to update notification settings: {}", e)))?;

    Ok(Json(NotificationSettingsResponse {
        notify_upload: req.notify_upload,
        notify_download: req.notify_download,
        notify_download_alert: req.notify_download_alert,
        notify_security: req.notify_security,
        notify_language: req.notify_language,
        default_expiration: req.default_expiration,
    }))
}
