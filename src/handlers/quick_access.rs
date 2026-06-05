use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;
use chrono::Utc;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{
        unauthorized, forbidden, not_found, internal_error, bad_request, AppError,
        QuickAccessUploadRequest, QuickAccessFileResponse, QuickAccessListResponse,
        InitMultipartUploadResponse, MultipartUploadFileInit,
    },
    services::StorageService,
    utils::generate_storage_key,
};

#[derive(Clone)]
pub struct QuickAccessState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

/// Initialize a Quick Access multipart upload session.
#[utoipa::path(
    post,
    path = "/user/quick-access/init",
    tag = "quick-access",
    request_body = QuickAccessUploadRequest,
    responses(
        (status = 200, description = "Upload session initialized", body = InitMultipartUploadResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 413, description = "`file_too_large` — per-file size limit exceeded. See https://share.mingyu.dev/api-terms-of-use for current limits.")
    ),
    security(("bearer_auth" = []))
)]
pub async fn init_quick_access_upload(
    State(state): State<QuickAccessState>,
    request: Request,
) -> Result<Json<InitMultipartUploadResponse>, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?
        .clone();

    let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| internal_error("Cannot read request body"))?;

    let req: QuickAccessUploadRequest = serde_json::from_slice(&body_bytes)
        .map_err(|_| internal_error("Invalid request format"))?;

    if req.files.is_empty() {
        return Err(bad_request("At least one file is required"));
    }

    let max_total_size: i64 = 3 * 1024 * 1024 * 1024;
    if req.files.iter().any(|f| f.file_size > crate::handlers::cli::STANDARD_PER_FILE_LIMIT) {
        return Err(crate::models::payload_too_large(crate::handlers::cli::FILE_TOO_LARGE_MESSAGE));
    }
    let total_size: i64 = req.files.iter().map(|f| f.file_size).sum();

    if total_size > max_total_size {
        return Err(crate::models::payload_too_large(crate::handlers::cli::FILE_TOO_LARGE_MESSAGE));
    }

    let share_code = repository::reserve_share_code(&state.db).await?;

    let upload_session_id = Uuid::new_v4().to_string();
    let chunk_size = req.chunk_size;

    let mut files: Vec<MultipartUploadFileInit> = Vec::new();

    for file_info in &req.files {
        let storage_key = generate_storage_key(
            &state.config.s3.prefix,
            &Uuid::new_v4().to_string(),
            &file_info.file_name,
        );

        let total_parts = ((file_info.file_size as f64) / (chunk_size as f64)).ceil() as i32;

        let upload_id = if total_parts > 1 {
            state.storage
                .create_multipart_upload(&storage_key, &file_info.content_type)
                .await
                .map_err(|e| {
                    error!(error = %e, "Failed to create multipart upload on R2");
                    internal_error("Failed to create R2 multipart upload")
                })?
        } else {
            String::new()
        };

        files.push(MultipartUploadFileInit {
            file_name: file_info.file_name.clone(),
            storage_key,
            upload_id,
            total_parts,
        });
    }

    let session_expires_at = Utc::now() + chrono::Duration::hours(1);

    repository::create_upload_session(
        &state.db,
        &upload_session_id,
        &share_code,
        Some(&user_claims.sub),
        req.device_info.as_deref(),
        None,
        true,
        true,
        "twenty_four_hours",
        session_expires_at,
    )
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create upload session");
        internal_error("Failed to create upload session")
    })?;

    Ok(Json(InitMultipartUploadResponse {
        upload_session_id,
        share_code,
        files,
        chunk_size,
    }))
}

/// List Quick Access files owned by the authenticated user.
#[utoipa::path(
    get,
    path = "/user/quick-access",
    tag = "quick-access",
    responses(
        (status = 200, description = "Quick Access files listed", body = QuickAccessListResponse),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_quick_access_files(
    State(state): State<QuickAccessState>,
    request: Request,
) -> Result<Json<QuickAccessListResponse>, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?;

    let file_shares = repository::find_quick_access_files_by_user(&state.db, &user_claims.sub)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to fetch quick access files");
            internal_error("Failed to fetch Quick Access file list")
        })?;

    let files: Vec<QuickAccessFileResponse> = file_shares
        .into_iter()
        .map(|fs| QuickAccessFileResponse {
            id: fs.id,
            file_name: fs.file_name,
            file_size: fs.file_size,
            file_type: fs.file_type,
            storage_key: fs.storage_key,
            uploaded_from: fs.description,
            expires_at: fs.expires_at,
            created_at: fs.created_at,
        })
        .collect();

    Ok(Json(QuickAccessListResponse { files }))
}

/// Delete a Quick Access file by ID.
#[utoipa::path(
    delete,
    path = "/user/quick-access/{file_id}",
    tag = "quick-access",
    params(
        ("file_id" = String, Path, description = "Quick Access file ID")
    ),
    responses(
        (status = 204, description = "File deleted"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "File not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_quick_access_file(
    State(state): State<QuickAccessState>,
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

    if !file_share.is_quick_access {
        return Err(forbidden("Not a Quick Access file"));
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

/// Get a short-lived presigned preview URL for a Quick Access file.
#[utoipa::path(
    get,
    path = "/user/quick-access/preview/{file_id}",
    tag = "quick-access",
    params(
        ("file_id" = String, Path, description = "Quick Access file ID")
    ),
    responses(
        (status = 200, description = "Preview URL generated"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "File not found or expired")
    ),
    security(("bearer_auth" = []))
)]
pub async fn preview_quick_access_file(
    State(state): State<QuickAccessState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<Json<serde_json::Value>, AppError> {
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

    if !file_share.is_quick_access {
        return Err(forbidden("Not a Quick Access file"));
    }

    if file_share.expires_at < Utc::now() {
        return Err(not_found("File expired"));
    }

    let preview_url = state
        .storage
        .generate_presigned_get_url(
            &file_share.storage_key,
            3600,
            None,
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to generate presigned preview URL");
            internal_error("Failed to create preview URL")
        })?;

    Ok(Json(serde_json::json!({
        "preview_url": preview_url,
        "file_name": file_share.file_name,
        "expires_in_secs": 3600
    })))
}

/// Create a temporary public share link for a Quick Access file (expires in 30 minutes).
#[utoipa::path(
    post,
    path = "/user/quick-access/share/{file_id}",
    tag = "quick-access",
    params(
        ("file_id" = String, Path, description = "Quick Access file ID")
    ),
    responses(
        (status = 200, description = "Share code created"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "File not found or expired")
    ),
    security(("bearer_auth" = []))
)]
pub async fn share_quick_access_file(
    State(state): State<QuickAccessState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?
        .clone();

    let file_share = repository::find_file_share_by_id(&state.db, &file_id)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?
        .ok_or_else(|| not_found("File not found"))?;

    if file_share.user_id.as_ref() != Some(&user_claims.sub) {
        return Err(forbidden("Access to another user's file is forbidden"));
    }

    if !file_share.is_quick_access {
        return Err(forbidden("Not a Quick Access file"));
    }

    if file_share.expires_at < Utc::now() {
        return Err(not_found("File expired"));
    }

    let share_code = repository::reserve_share_code(&state.db).await?;

    let grant_expires_at = Utc::now() + chrono::Duration::minutes(30);

    repository::create_public_share_grant(
        &state.db,
        &share_code,
        &file_share.id,
        grant_expires_at,
    )
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create public share grant");
        internal_error("Failed to create share session")
    })?;

    Ok(Json(serde_json::json!({
        "share_code": share_code
    })))
}

/// Download a Quick Access file (returns a presigned download URL and deletes the file).
#[utoipa::path(
    get,
    path = "/user/quick-access/download/{file_id}",
    tag = "quick-access",
    params(
        ("file_id" = String, Path, description = "Quick Access file ID")
    ),
    responses(
        (status = 200, description = "Download URL returned — file will be deleted after download"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "File not found or expired")
    ),
    security(("bearer_auth" = []))
)]
pub async fn download_quick_access_file(
    State(state): State<QuickAccessState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("Authentication required"))?
        .clone();

    let file_share = repository::find_file_share_by_id(&state.db, &file_id)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?
        .ok_or_else(|| not_found("File not found"))?;

    if file_share.user_id.as_ref() != Some(&user_claims.sub) {
        return Err(forbidden("Access to another user's file is forbidden"));
    }

    if !file_share.is_quick_access {
        return Err(forbidden("Not a Quick Access file"));
    }

    if file_share.expires_at < Utc::now() {
        return Err(not_found("File expired"));
    }

    let download_url = state
        .storage
        .generate_presigned_get_url(
            &file_share.storage_key,
            3600,
            Some(&file_share.file_name),
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to generate presigned download URL");
            internal_error("Failed to create download URL")
        })?;

    repository::delete_file_share(&state.db, &file_id)
        .await
        .map_err(|e| internal_error(format!("Failed to delete file: {}", e)))?;

    if !file_share.storage_key.is_empty() {
        let storage = state.storage.clone();
        let storage_key = file_share.storage_key.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            let _ = storage.delete_file(&storage_key).await;
        });
    }

    Ok(Json(serde_json::json!({
        "download_url": download_url,
        "file_name": file_share.file_name,
        "expires_in_secs": 3600
    })))
}
