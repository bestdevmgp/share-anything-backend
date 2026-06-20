use axum::{
    extract::{Extension, Multipart, Request, State},
    Json,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{
        bad_request, unauthorized, internal_error, AppError,
        ExpirationPeriod, FileShareResponse, MultipleFileUploadResponse, TransferType,
        UploadMetadata, CreateP2PSessionRequest,
    },
    services::{generate_qr_code, NotificationService, StorageService},
    utils::generate_storage_key,
};
use chrono::Utc;

#[derive(Clone)]
pub struct UploadState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
    pub notifications: Arc<NotificationService>,
}

#[utoipa::path(
    post,
    path = "/file/upload",
    tag = "upload",
    responses(
        (status = 200, description = "Files uploaded successfully", body = MultipleFileUploadResponse),
        (status = 400, description = "Bad request - missing or invalid file"),
        (status = 401, description = "Unauthorized - custom expiration/password requires authentication"),
        (status = 413, description = "`file_too_large` — upload size limit exceeded. See https://share.mingyu.dev/api-terms-of-use for current limits.")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn upload_file(
    State(state): State<UploadState>,
    user_claims: Option<Extension<Claims>>,
    headers: axum::http::HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<MultipleFileUploadResponse>, AppError> {
    let user_claims = user_claims.map(|ext| ext.0.clone());

    let device_id = crate::utils::extract_device_id(&headers);

    struct FileData {
        name: String,
        data: Vec<u8>,
        content_type: String,
    }

    let mut files: Vec<FileData> = Vec::new();
    let mut description: Option<String> = None;
    let mut password: Option<String> = None;
    let mut expiration: Option<ExpirationPeriod> = None;
    let mut is_one_time: Option<bool> = None;
    let mut transfer_type: Option<TransferType> = None;

    while let Some(field) = multipart.next_field().await.map_err(|_| bad_request("Failed to parse multipart data"))? {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                let file_name = field
                    .file_name()
                    .ok_or_else(|| bad_request("File name is missing"))?
                    .to_string();
                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let data = field.bytes().await.map_err(|_| bad_request("Cannot read file data"))?;

                files.push(FileData {
                    name: file_name,
                    data: data.to_vec(),
                    content_type,
                });
            }
            "description" => {
                let text = field.text().await.map_err(|_| bad_request("Cannot read description field"))?;
                if !text.is_empty() {
                    description = Some(text);
                }
            }
            "password" => {
                let text = field.text().await.map_err(|_| bad_request("Cannot read password field"))?;
                if !text.is_empty() {
                    password = Some(text);
                }
            }
            "expiration" => {
                let text = field.text().await.map_err(|_| bad_request("Cannot read expiration field"))?;
                if !text.is_empty() {
                    expiration = serde_json::from_value(serde_json::json!(text)).ok();
                }
            }
            "is_one_time" => {
                let text = field.text().await.map_err(|_| bad_request("Cannot read is_one_time field"))?;
                if !text.is_empty() {
                    is_one_time = text.parse::<bool>().ok();
                }
            }
            "transfer_type" => {
                let text = field.text().await.map_err(|_| bad_request("Cannot read transfer_type field"))?;
                if !text.is_empty() {
                    transfer_type = serde_json::from_str(&format!("\"{}\"", text))
                        .map_err(|_| bad_request("Invalid transfer_type value"))?;
                }
            }
            _ => {}
        }
    }

    if files.is_empty() {
        return Err(bad_request("No files uploaded. At least one file is required."));
    }

    let total_size: i64 = files.iter().map(|f| f.data.len() as i64).sum();

    let metadata = UploadMetadata {
        description,
        password,
        expiration,
        is_one_time,
        transfer_type,
    };

    let expiration = if let Some(exp) = metadata.expiration {
        if user_claims.is_none() && !matches!(exp, ExpirationPeriod::FiveMinutes) {
            return Err(unauthorized("Guest users can only use the 5-minute expiration"));
        }
        exp
    } else {
        ExpirationPeriod::FiveMinutes
    };

    let transfer_type = metadata.transfer_type.unwrap_or(TransferType::Server);

    // Standard (server) uploads count against the daily quota; P2P is exempt.
    if matches!(transfer_type, TransferType::Server) {
        crate::utils::enforce_daily_quota(
            &state.db,
            user_claims.as_ref().map(|c| c.sub.as_str()),
            &headers,
            total_size,
        )
        .await?;
    }

    let is_one_time = if matches!(transfer_type, TransferType::P2p) {
        true
    } else {
        metadata.is_one_time.unwrap_or(false)
    };

    if is_one_time && user_claims.is_none() && !matches!(transfer_type, TransferType::P2p) {
        return Err(unauthorized("Sign in required for one-time download"));
    }

    let expires_at = Utc::now() + expiration.to_duration();

    let original_password = metadata.password.clone();

    let password_hash = if let Some(password) = metadata.password {
        if user_claims.is_none() {
            return Err(unauthorized("Sign in required for password protection"));
        }
        Some(
            tokio::task::spawn_blocking(move || bcrypt::hash(password, bcrypt::DEFAULT_COST))
                .await
                .map_err(|_| internal_error("Failed to hash password"))?
                .map_err(|_| internal_error("Failed to hash password"))?,
        )
    } else {
        None
    };

    let share_code = repository::reserve_share_code(&state.db).await?;

    let share_group_id = Uuid::new_v4().to_string();
    let mut uploaded_files: Vec<FileShareResponse> = Vec::new();

    for (idx, file_data) in files.into_iter().enumerate() {
        let file_size = file_data.data.len() as i64;

        let storage_key = if matches!(transfer_type, TransferType::P2p) {
            String::new()
        } else {
            let key = generate_storage_key(
                &state.config.s3.prefix,
                &Uuid::new_v4().to_string(),
                &file_data.name,
            );

            state
                .storage
                .upload_file(&key, file_data.data, &file_data.content_type)
                .await
                .map_err(|e| internal_error(format!("Storage upload failed: {}", e)))?;

            key
        };

        let transfer_type_str = match transfer_type {
            TransferType::Server => "server",
            TransferType::P2p => "p2p",
        };

        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            user_claims.as_ref().map(|c| c.sub.clone()),
            None,
            share_code.clone(),
            file_data.name.clone(),
            file_size,
            file_data.content_type.clone(),
            transfer_type_str.to_string(),
            storage_key,
            metadata.description.clone(),
            password_hash.clone(),
            is_one_time,
            false,
            expires_at,
            None,
            None,
            idx as i32,
            device_id.clone(),
            None,
        )
        .await
        .map_err(|e| internal_error(format!("Failed to save to database: {}", e)))?;

        uploaded_files.push(FileShareResponse {
            id: file_share.id,
            share_code: file_share.share_code.clone(),
            file_name: file_share.file_name,
            file_size: file_share.file_size,
            file_type: file_share.file_type,
            transfer_type: file_share.transfer_type.clone(),
            description: file_share.description,
            relative_path: file_share.relative_path,
            has_password: file_share.password_hash.is_some(),
            is_one_time: file_share.is_one_time,
            expires_at: file_share.expires_at,
            created_at: file_share.created_at,
            download_url: String::new(),
            qr_code: None,
            uploader_online: None,
        });
    }

    let download_url = format!(
        "{}/download?code={}",
        state.config.server.base_url, share_code
    );

    let qr_code = generate_qr_code(&download_url).ok();

    for file in &mut uploaded_files {
        file.download_url = download_url.clone();
        file.qr_code = qr_code.clone();
    }

    if matches!(transfer_type, TransferType::Server) {
        crate::utils::record_daily_usage(
            &state.db,
            user_claims.as_ref().map(|c| c.sub.as_str()),
            &headers,
            total_size,
        )
        .await;
    }

    if let Some(ref claims) = user_claims {
        state
            .notifications
            .notify_upload(
                &claims.sub,
                &share_code,
                &uploaded_files,
                expires_at,
                original_password.clone(),
                metadata.description.clone(),
                transfer_type.clone(),
            )
            .await;
    }

    Ok(Json(MultipleFileUploadResponse {
        share_code: share_code.clone(),
        total_count: uploaded_files.len(),
        files: uploaded_files,
    }))
}

#[utoipa::path(
    post,
    path = "/file/p2p/create",
    tag = "upload",
    request_body = CreateP2PSessionRequest,
    responses(
        (status = 200, description = "P2P session created successfully", body = MultipleFileUploadResponse),
        (status = 400, description = "Bad request"),
        (status = 429, description = "Rate limited")
    )
)]
pub async fn create_p2p_session(
    State(state): State<UploadState>,
    request_parts: Request,
) -> Result<Json<MultipleFileUploadResponse>, AppError> {
    let user_claims = request_parts.extensions().get::<Claims>().cloned();

    let body_bytes = axum::body::to_bytes(request_parts.into_body(), usize::MAX)
        .await
        .map_err(|_| bad_request("Cannot read request body"))?;

    let request: CreateP2PSessionRequest = serde_json::from_slice(&body_bytes)
        .map_err(|_| bad_request("Invalid request format"))?;

    if request.files.is_empty() {
        return Err(bad_request("File information is required"));
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

    let share_code = repository::reserve_share_code(&state.db).await?;

    let expires_at = Utc::now() + chrono::Duration::hours(24);
    let share_group_id = Uuid::new_v4().to_string();
    let mut uploaded_files: Vec<FileShareResponse> = Vec::new();

    for (idx, file_info) in request.files.into_iter().enumerate() {
        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            user_claims.as_ref().map(|c| c.sub.clone()),
            None,
            share_code.clone(),
            file_info.name.clone(),
            file_info.size,
            file_info.content_type.clone(),
            "p2p".to_string(),
            String::new(),
            None,
            password_hash.clone(),
            true,
            false,
            expires_at,
            None,
            None,
            idx as i32,
            None,
            None,
        )
        .await
        .map_err(|e| internal_error(format!("Failed to save to database: {}", e)))?;

        uploaded_files.push(FileShareResponse {
            id: file_share.id,
            share_code: file_share.share_code.clone(),
            file_name: file_share.file_name,
            file_size: file_share.file_size,
            file_type: file_share.file_type,
            transfer_type: file_share.transfer_type.clone(),
            description: file_share.description,
            relative_path: file_share.relative_path,
            has_password: file_share.password_hash.is_some(),
            is_one_time: file_share.is_one_time,
            expires_at: file_share.expires_at,
            created_at: file_share.created_at,
            download_url: String::new(),
            qr_code: None,
            uploader_online: None,
        });
    }

    let download_url = format!(
        "{}/download?code={}",
        state.config.server.base_url, share_code
    );

    let qr_code = generate_qr_code(&download_url).ok();

    for file in &mut uploaded_files {
        file.download_url = download_url.clone();
        file.qr_code = qr_code.clone();
    }

    Ok(Json(MultipleFileUploadResponse {
        share_code: share_code.clone(),
        total_count: uploaded_files.len(),
        files: uploaded_files,
    }))
}
