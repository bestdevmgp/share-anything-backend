use axum::{
    extract::{Extension, State},
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
        bad_request, unauthorized, internal_error, AppError,
        ExpirationPeriod, FileShareResponse, MultipleFileUploadResponse, TransferType,
        PresignedUploadRequest, PresignedUploadResponse, PresignedUploadUrl,
        CompleteUploadRequest,
        InitMultipartUploadRequest, InitMultipartUploadResponse, MultipartUploadFileInit,
        GetPartUrlsRequest, GetPartUrlsResponse, PartPresignedUrl,
        CompleteMultipartUploadRequest,
    },
    services::{generate_qr_code, NotificationService, StorageService},
    utils::generate_storage_key,
};

#[derive(Clone)]
pub struct PresignedState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
    pub notifications: Arc<NotificationService>,
}

const PRESIGNED_URL_EXPIRY_SECS: u64 = 3600;

/// Request presigned PUT URLs for single-part uploads directly to S3.
#[utoipa::path(
    post,
    path = "/file/presign",
    tag = "presigned",
    request_body = PresignedUploadRequest,
    responses(
        (status = 200, description = "Presigned URLs generated", body = PresignedUploadResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Authentication required for some options"),
        (status = 413, description = "`file_too_large` — per-file size limit exceeded. See https://share.mingyu.dev/api-terms-of-use for current limits.")
    ),
    security(("bearer_auth" = []))
)]
pub async fn request_presigned_upload(
    State(state): State<PresignedState>,
    user_claims: Option<Extension<Claims>>,
    Json(request): Json<PresignedUploadRequest>,
) -> Result<Json<PresignedUploadResponse>, AppError> {
    let user_claims = user_claims.map(|ext| ext.0.clone());

    if request.files.is_empty() {
        return Err(bad_request("At least one file is required"));
    }

    let max_total_size: i64 = if user_claims.is_some() {
        3 * 1024 * 1024 * 1024
    } else {
        500 * 1024 * 1024
    };

    if request.files.iter().any(|f| f.file_size > crate::handlers::cli::STANDARD_PER_FILE_LIMIT) {
        return Err(crate::models::payload_too_large(crate::handlers::cli::FILE_TOO_LARGE_MESSAGE));
    }
    let total_size: i64 = request.files.iter().map(|f| f.file_size).sum();

    if total_size > max_total_size {
        return Err(crate::models::payload_too_large(crate::handlers::cli::FILE_TOO_LARGE_MESSAGE));
    }

    let expiration = if let Some(exp) = request.expiration {
        if user_claims.is_none() && !matches!(exp, ExpirationPeriod::FiveMinutes) {
            return Err(unauthorized("Guest users can only use the 5-minute expiration"));
        }
        exp
    } else {
        if let Some(claims) = user_claims.as_ref() {
            let user = repository::find_user_by_id(&state.db, &claims.sub)
                .await?
                .ok_or_else(|| internal_error("Authenticated user not found"))?;
            ExpirationPeriod::from_str(&user.default_expiration)
                .unwrap_or(ExpirationPeriod::ThirtyMinutes)
        } else {
            ExpirationPeriod::ThirtyMinutes
        }
    };

    let is_one_time = request.is_one_time.unwrap_or(false);

    if is_one_time && user_claims.is_none() {
        return Err(unauthorized("Sign in required for one-time download"));
    }

    if request.password.is_some() && user_claims.is_none() {
        return Err(unauthorized("Sign in required for password protection"));
    }

    let share_code = repository::reserve_share_code(&state.db).await?;

    let upload_session_id = Uuid::new_v4().to_string();
    let mut urls: Vec<PresignedUploadUrl> = Vec::new();

    for file_info in &request.files {
        let storage_key = generate_storage_key(
            &state.config.s3.prefix,
            &Uuid::new_v4().to_string(),
            &file_info.file_name,
        );

        let presigned_url = state
            .storage
            .generate_presigned_put_url(&storage_key, &file_info.content_type, PRESIGNED_URL_EXPIRY_SECS)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to generate presigned URL");
                internal_error("Failed to create presigned URL")
            })?;

        urls.push(PresignedUploadUrl {
            file_name: file_info.file_name.clone(),
            storage_key,
            presigned_url,
        });
    }

    let password_hash = if let Some(password) = &request.password {
        let password = password.clone();
        Some(
            tokio::task::spawn_blocking(move || bcrypt::hash(password, bcrypt::DEFAULT_COST))
                .await
                .map_err(|_| internal_error("Failed to hash password"))?
                .map_err(|_| internal_error("Failed to hash password"))?,
        )
    } else {
        None
    };

    let expiration_period_str = expiration.to_string();
    let session_expires_at = Utc::now() + chrono::Duration::hours(1);

    repository::create_upload_session(
        &state.db,
        &upload_session_id,
        &share_code,
        user_claims.as_ref().map(|c| c.sub.as_str()),
        request.description.as_deref(),
        password_hash.as_deref(),
        is_one_time,
        false,
        &expiration_period_str,
        session_expires_at,
    )
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create upload session");
        internal_error("Failed to create upload session")
    })?;

    Ok(Json(PresignedUploadResponse {
        upload_session_id,
        share_code,
        urls,
        expires_in_secs: PRESIGNED_URL_EXPIRY_SECS,
    }))
}

/// Complete a presigned single-part upload and finalize the file share record.
#[utoipa::path(
    post,
    path = "/file/complete",
    tag = "presigned",
    request_body = CompleteUploadRequest,
    responses(
        (status = 200, description = "Upload completed", body = MultipleFileUploadResponse),
        (status = 400, description = "Invalid session or already completed")
    )
)]
pub async fn complete_presigned_upload(
    State(state): State<PresignedState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CompleteUploadRequest>,
) -> Result<Json<MultipleFileUploadResponse>, AppError> {
    let device_id = crate::utils::extract_device_id(&headers);

    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to get upload session");
            internal_error("Failed to fetch upload session")
        })?
        .ok_or_else(|| bad_request("Invalid upload session"))?;

    if session.share_code != request.share_code {
        return Err(bad_request("Share code does not match"));
    }

    if session.completed {
        return Err(bad_request("Upload session already completed"));
    }

    let expiration_period = ExpirationPeriod::from_str(&session.expiration_period)
        .unwrap_or(ExpirationPeriod::FiveMinutes);
    let expires_at = Utc::now() + expiration_period.to_duration();

    let share_group_id = Uuid::new_v4().to_string();
    let mut uploaded_files: Vec<FileShareResponse> = Vec::new();

    for (idx, file_info) in request.files.iter().enumerate() {
        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            session.user_id.clone(),
            None,
            session.share_code.clone(),
            file_info.file_name.clone(),
            file_info.file_size,
            file_info.content_type.clone(),
            "server".to_string(),
            file_info.storage_key.clone(),
            session.description.clone(),
            session.password_hash.clone(),
            session.is_one_time,
            session.is_quick_access,
            expires_at,
            file_info.image_width,
            file_info.image_height,
            idx as i32,
            device_id.clone(),
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create file share record");
            internal_error(format!("Failed to save to database: {}", e))
        })?;

        uploaded_files.push(FileShareResponse {
            id: file_share.id,
            share_code: file_share.share_code.clone(),
            file_name: file_share.file_name,
            file_size: file_share.file_size,
            file_type: file_share.file_type,
            transfer_type: file_share.transfer_type,
            description: file_share.description,
            has_password: file_share.password_hash.is_some(),
            is_one_time: file_share.is_one_time,
            expires_at: file_share.expires_at,
            created_at: file_share.created_at,
            download_url: String::new(),
            qr_code: None,
            uploader_online: None,
        });
    }

    repository::complete_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to complete upload session");
            internal_error("Failed to complete upload session")
        })?;

    let download_url = format!(
        "{}/download?code={}",
        state.config.server.base_url, session.share_code
    );

    let qr_code = generate_qr_code(&download_url).ok();

    for file in &mut uploaded_files {
        file.download_url = download_url.clone();
        file.qr_code = qr_code.clone();
    }

    if !session.is_quick_access {
        if let Some(ref user_id) = session.user_id {
            state
                .notifications
                .notify_upload(
                    user_id,
                    &session.share_code,
                    &uploaded_files,
                    expires_at,
                    None,
                    session.description.clone(),
                    TransferType::Server,
                )
                .await;
        }
    }

    Ok(Json(MultipleFileUploadResponse {
        share_code: session.share_code,
        total_count: uploaded_files.len(),
        files: uploaded_files,
    }))
}

/// Initialize a multipart upload session and create S3 multipart upload IDs.
#[utoipa::path(
    post,
    path = "/file/multipart/init",
    tag = "presigned",
    request_body = InitMultipartUploadRequest,
    responses(
        (status = 200, description = "Multipart upload initialized", body = InitMultipartUploadResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Authentication required for some options"),
        (status = 413, description = "`file_too_large` — per-file size limit exceeded. See https://share.mingyu.dev/api-terms-of-use for current limits.")
    ),
    security(("bearer_auth" = []))
)]
pub async fn init_multipart_upload(
    State(state): State<PresignedState>,
    user_claims: Option<Extension<Claims>>,
    Json(request): Json<InitMultipartUploadRequest>,
) -> Result<Json<InitMultipartUploadResponse>, AppError> {
    let user_claims = user_claims.map(|ext| ext.0.clone());

    if request.files.is_empty() {
        return Err(bad_request("At least one file is required"));
    }

    let max_total_size: i64 = if user_claims.is_some() {
        3 * 1024 * 1024 * 1024
    } else {
        500 * 1024 * 1024
    };

    if request.files.iter().any(|f| f.file_size > crate::handlers::cli::STANDARD_PER_FILE_LIMIT) {
        return Err(crate::models::payload_too_large(crate::handlers::cli::FILE_TOO_LARGE_MESSAGE));
    }
    let total_size: i64 = request.files.iter().map(|f| f.file_size).sum();

    if total_size > max_total_size {
        return Err(crate::models::payload_too_large(crate::handlers::cli::FILE_TOO_LARGE_MESSAGE));
    }

    let expiration = if let Some(exp) = request.expiration {
        if user_claims.is_none() && !matches!(exp, ExpirationPeriod::FiveMinutes) {
            return Err(unauthorized("Guest users can only use the 5-minute expiration"));
        }
        exp
    } else {
        if let Some(claims) = user_claims.as_ref() {
            let user = repository::find_user_by_id(&state.db, &claims.sub)
                .await?
                .ok_or_else(|| internal_error("Authenticated user not found"))?;
            ExpirationPeriod::from_str(&user.default_expiration)
                .unwrap_or(ExpirationPeriod::ThirtyMinutes)
        } else {
            ExpirationPeriod::ThirtyMinutes
        }
    };

    let is_one_time = request.is_one_time.unwrap_or(false);

    if is_one_time && user_claims.is_none() {
        return Err(unauthorized("Sign in required for one-time download"));
    }

    if request.password.is_some() && user_claims.is_none() {
        return Err(unauthorized("Sign in required for password protection"));
    }

    let share_code = repository::reserve_share_code(&state.db).await?;

    let upload_session_id = Uuid::new_v4().to_string();
    let chunk_size = request.chunk_size;

    let mut files: Vec<MultipartUploadFileInit> = Vec::new();

    for file_info in &request.files {
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

        let upload_signature =
            crate::utils::sign_storage_key(&state.config.upload_signing.secret, &storage_key);
        files.push(MultipartUploadFileInit {
            file_name: file_info.file_name.clone(),
            storage_key,
            upload_id,
            total_parts,
            upload_signature,
        });
    }

    let password_hash = if let Some(password) = &request.password {
        let password = password.clone();
        Some(
            tokio::task::spawn_blocking(move || bcrypt::hash(password, bcrypt::DEFAULT_COST))
                .await
                .map_err(|_| internal_error("Failed to hash password"))?
                .map_err(|_| internal_error("Failed to hash password"))?,
        )
    } else {
        None
    };

    let expiration_period_str = expiration.to_string();
    let session_expires_at = Utc::now() + chrono::Duration::hours(1);

    repository::create_upload_session(
        &state.db,
        &upload_session_id,
        &share_code,
        user_claims.as_ref().map(|c| c.sub.as_str()),
        request.description.as_deref(),
        password_hash.as_deref(),
        is_one_time,
        false,
        &expiration_period_str,
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

/// Get presigned URLs for individual multipart upload parts.
#[utoipa::path(
    post,
    path = "/file/multipart/presign-parts",
    tag = "presigned",
    request_body = GetPartUrlsRequest,
    responses(
        (status = 200, description = "Part presigned URLs generated", body = GetPartUrlsResponse),
        (status = 400, description = "Invalid or completed session")
    )
)]
pub async fn get_part_presigned_urls(
    State(state): State<PresignedState>,
    Json(request): Json<GetPartUrlsRequest>,
) -> Result<Json<GetPartUrlsResponse>, AppError> {
    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to get upload session");
            internal_error("Failed to fetch upload session")
        })?
        .ok_or_else(|| bad_request("Invalid upload session"))?;

    if session.completed {
        return Err(bad_request("Upload session already completed"));
    }

    let mut urls: Vec<PartPresignedUrl> = Vec::new();

    for part_number in &request.part_numbers {
        let presigned_url = state
            .storage
            .generate_presigned_upload_part_url(
                &request.storage_key,
                &request.upload_id,
                *part_number,
                PRESIGNED_URL_EXPIRY_SECS,
            )
            .await
            .map_err(|e| {
                error!(error = %e, part_number = part_number, "Failed to generate presigned URL for part");
                internal_error("Failed to create part presigned URL")
            })?;

        urls.push(PartPresignedUrl {
            part_number: *part_number,
            presigned_url,
        });
    }

    Ok(Json(GetPartUrlsResponse {
        storage_key: request.storage_key,
        urls,
        expires_in_secs: PRESIGNED_URL_EXPIRY_SECS,
    }))
}

/// Complete a multipart upload and finalize all file share records.
#[utoipa::path(
    post,
    path = "/file/multipart/complete",
    tag = "presigned",
    request_body = CompleteMultipartUploadRequest,
    responses(
        (status = 200, description = "Multipart upload completed", body = MultipleFileUploadResponse),
        (status = 400, description = "Invalid session or already completed")
    )
)]
pub async fn complete_multipart_upload(
    State(state): State<PresignedState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CompleteMultipartUploadRequest>,
) -> Result<Json<MultipleFileUploadResponse>, AppError> {
    let device_id = crate::utils::extract_device_id(&headers);

    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to get upload session");
            internal_error("Failed to fetch upload session")
        })?
        .ok_or_else(|| bad_request("Invalid upload session"))?;

    if session.share_code != request.share_code {
        return Err(bad_request("Share code does not match"));
    }

    if session.completed {
        return Err(bad_request("Upload session already completed"));
    }

    let expiration_period = ExpirationPeriod::from_str(&session.expiration_period)
        .unwrap_or(ExpirationPeriod::FiveMinutes);
    let expires_at = Utc::now() + expiration_period.to_duration();

    let share_group_id = Uuid::new_v4().to_string();
    let mut uploaded_files: Vec<FileShareResponse> = Vec::new();

    for (idx, file_info) in request.files.iter().enumerate() {
        if file_info.upload_id != "direct" && !file_info.parts.is_empty() {
            let parts: Vec<(i32, String)> = file_info.parts
                .iter()
                .map(|p| (p.part_number, p.etag.clone()))
                .collect();

            state.storage
                .complete_multipart_upload(&file_info.storage_key, &file_info.upload_id, parts)
                .await
                .map_err(|e| {
                    error!(error = %e, "Failed to complete multipart upload on R2");
                    internal_error("Failed to complete R2 multipart upload")
                })?;
        }

        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            session.user_id.clone(),
            None,
            session.share_code.clone(),
            file_info.file_name.clone(),
            file_info.file_size,
            file_info.content_type.clone(),
            "server".to_string(),
            file_info.storage_key.clone(),
            session.description.clone(),
            session.password_hash.clone(),
            session.is_one_time,
            session.is_quick_access,
            expires_at,
            file_info.image_width,
            file_info.image_height,
            idx as i32,
            device_id.clone(),
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create file share record");
            internal_error(format!("Failed to save to database: {}", e))
        })?;

        uploaded_files.push(FileShareResponse {
            id: file_share.id,
            share_code: file_share.share_code.clone(),
            file_name: file_share.file_name,
            file_size: file_share.file_size,
            file_type: file_share.file_type,
            transfer_type: file_share.transfer_type,
            description: file_share.description,
            has_password: file_share.password_hash.is_some(),
            is_one_time: file_share.is_one_time,
            expires_at: file_share.expires_at,
            created_at: file_share.created_at,
            download_url: String::new(),
            qr_code: None,
            uploader_online: None,
        });
    }

    repository::complete_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to complete upload session");
            internal_error("Failed to complete upload session")
        })?;

    let download_url = format!(
        "{}/download?code={}",
        state.config.server.base_url, session.share_code
    );

    let qr_code = generate_qr_code(&download_url).ok();

    for file in &mut uploaded_files {
        file.download_url = download_url.clone();
        file.qr_code = qr_code.clone();
    }

    if !session.is_quick_access {
        if let Some(ref user_id) = session.user_id {
            state
                .notifications
                .notify_upload(
                    user_id,
                    &session.share_code,
                    &uploaded_files,
                    expires_at,
                    None,
                    session.description.clone(),
                    TransferType::Server,
                )
                .await;
        }
    }

    Ok(Json(MultipleFileUploadResponse {
        share_code: session.share_code,
        total_count: uploaded_files.len(),
        files: uploaded_files,
    }))
}