use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::personal_token_auth::PersonalTokenUser,
    models::{
        bad_request, forbidden, internal_error, not_found, unauthorized, AppError,
        ExpirationPeriod, CreateDownloadLogDto,
        CliUploadResponse, CliFileInfoResponse, CliFileDetail, CliFileListResponse,
        CliP2PCreateRequest, CliP2PCreateResponse,
        CliMultipartInitRequest, CliMultipartInitResponse, CliMultipartFileInit,
        CliPresignPartsRequest, CliPresignPartsResponse, CliPartUrl,
        CliCompleteMultipartRequest,
        CliDownloadQuery, CliDownloadUrlResponse, CliDownloadCompleteRequest,
        CliUploadHistoryQuery,
    },
    services::StorageService,
};

#[allow(unused_imports)]
use crate::models::{
    CliMeResponse,
    CliUploadHistoryItem, CliUploadHistoryResponse,
    CliDownloadHistoryItem, CliDownloadHistoryResponse,
    CliShareDownloadLog, CliShareLogsResponse,
};

use crate::{
    utils::{
        encode_content_disposition, generate_storage_key, parse_device_platform, PrettyJson,
    },
};

#[derive(Clone)]
pub struct CliState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

const PRESIGNED_URL_EXPIRY_SECS: u64 = 3600;
pub const API_KEY_ACTIVE_STORAGE_LIMIT: i64 = 500 * 1024 * 1024 * 1024;
pub const STORAGE_QUOTA_MESSAGE: &str = "API key storage quota exceeded.";

fn format_expires_at(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn parse_cli_expiration(s: &str) -> Option<ExpirationPeriod> {
    match s {
        "5m" | "five_minutes" => Some(ExpirationPeriod::FiveMinutes),
        "30m" | "thirty_minutes" => Some(ExpirationPeriod::ThirtyMinutes),
        "1h" | "one_hour" => Some(ExpirationPeriod::OneHour),
        "3h" | "three_hours" => Some(ExpirationPeriod::ThreeHours),
        "6h" | "six_hours" => Some(ExpirationPeriod::SixHours),
        "12h" | "twelve_hours" => Some(ExpirationPeriod::TwelveHours),
        "24h" | "twenty_four_hours" => Some(ExpirationPeriod::TwentyFourHours),
        _ => None,
    }
}

/// Upload one or more files via a multipart form.
///
/// Prefer the presigned flow (`POST /cli/uploads/multipart`) for large files — this endpoint
/// relays bytes through the server and can fail when the multipart part lacks a reliable
/// Content-Length.
///
/// Multipart fields:
/// - `file` (one or more) — the file content. Repeat for multiple files.
/// - `description` (optional, text)
/// - `password` (optional, text; requires `X-Personal-Token`)
/// - `expiration` (optional, text: 5m, 30m, 1h, 3h, 6h, 12h, 24h; requires token)
/// - `is_one_time` (optional, text: "true"/"false"; requires token)
#[utoipa::path(
    post,
    path = "/cli/uploads",
    tag = "cli",
    responses(
        (status = 200, description = "Files uploaded", body = CliUploadResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Personal token required for password / expiration / one-time"),
        (status = 413, description = "`file_too_large` — upload size limit exceeded. See https://shareany.app/api-terms-of-use for current limits."),
        (status = 429, description = "`storage_quota_exceeded` — API key storage quota exceeded (only on requests authenticated by an API key, `sak_`). See https://shareany.app/api-terms-of-use for current limits."),
        (status = 500, description = "Storage upload failed")
    )
)]
pub async fn cli_upload(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    headers: axum::http::HeaderMap,
    mut multipart: Multipart,
) -> Result<PrettyJson<CliUploadResponse>, AppError> {
    let token_user = token_user.map(|ext| ext.0.clone());

    let mut description: Option<String> = None;
    let mut password: Option<String> = None;
    let mut expiration_str: Option<String> = None;
    let mut is_one_time: Option<bool> = None;

    struct StagedFile {
        name: String,
        storage_key: String,
        content_type: String,
        size: i64,
    }
    let mut staged_files: Vec<StagedFile> = Vec::new();
    let mut total_size: i64 = 0;

    let api_key_id_for_quota = token_user.as_ref().and_then(|u| u.api_key_id.clone());
    let mut existing_active_storage: i64 = 0;
    if let Some(ref kid) = api_key_id_for_quota {
        existing_active_storage = repository::sum_active_storage_for_api_key(&state.db, kid)
            .await
            .map_err(|_| internal_error("Failed to compute API key storage usage"))?;
        if existing_active_storage >= API_KEY_ACTIVE_STORAGE_LIMIT {
            return Err(crate::models::storage_quota_exceeded(STORAGE_QUOTA_MESSAGE));
        }
    }

    let daily_identity = crate::utils::quota_identity(
        token_user.as_ref().map(|u| u.user_id.as_str()),
        &headers,
    );
    let daily_used: i64 = if api_key_id_for_quota.is_none() {
        repository::get_daily_upload_usage(&state.db, &daily_identity, crate::utils::kst_today())
            .await
            .unwrap_or(0)
    } else {
        0
    };

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| bad_request("Failed to parse multipart data"))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "description" => {
                let text = field.text().await.map_err(|_| bad_request("Failed to read description field"))?;
                if !text.is_empty() {
                    description = Some(text);
                }
            }
            "password" => {
                let text = field.text().await.map_err(|_| bad_request("Failed to read password field"))?;
                if !text.is_empty() {
                    password = Some(text);
                }
            }
            "expiration" => {
                let text = field.text().await.map_err(|_| bad_request("Failed to read expiration field"))?;
                if !text.is_empty() {
                    expiration_str = Some(text);
                }
            }
            "is_one_time" => {
                let text = field.text().await.map_err(|_| bad_request("Failed to read is_one_time field"))?;
                if !text.is_empty() {
                    is_one_time = text.parse::<bool>().ok();
                }
            }
            "file" => {
                let file_name = field
                    .file_name()
                    .ok_or_else(|| bad_request("File name is missing"))?
                    .to_string();
                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();

                let storage_key = generate_storage_key(
                    &state.config.s3.prefix,
                    &Uuid::new_v4().to_string(),
                    &file_name,
                );

                let data = field
                    .bytes()
                    .await
                    .map_err(|_| bad_request("Failed to read file data"))?;
                let file_size = data.len() as i64;

                state
                    .storage
                    .upload_file(&storage_key, data.to_vec(), &content_type)
                    .await
                    .map_err(|e| internal_error(format!("Storage upload failed: {}", e)))?;
                total_size += file_size;
                if api_key_id_for_quota.is_some()
                    && existing_active_storage + total_size > API_KEY_ACTIVE_STORAGE_LIMIT
                {
                    return Err(crate::models::storage_quota_exceeded(STORAGE_QUOTA_MESSAGE));
                }
                if api_key_id_for_quota.is_none()
                    && daily_used + total_size
                        > crate::utils::daily_limit_for(token_user.as_ref().map(|u| u.user_id.as_str()))
                {
                    return Err(crate::models::daily_quota_exceeded(crate::utils::DAILY_QUOTA_MESSAGE));
                }

                staged_files.push(StagedFile {
                    name: file_name,
                    storage_key,
                    content_type,
                    size: file_size,
                });
            }
            _ => {}
        }
    }

    if staged_files.is_empty() {
        return Err(bad_request("No files were uploaded"));
    }

    let expiration = if let Some(exp_str) = expiration_str {
        if token_user.is_none() {
            return Err(unauthorized("Personal token required to set expiration"));
        }
        parse_cli_expiration(&exp_str)
            .ok_or_else(|| bad_request("Invalid expiration. Available: 5m, 30m, 1h, 3h, 6h, 12h, 24h"))?
    } else {
        ExpirationPeriod::ThirtyMinutes
    };

    let password_hash = if let Some(pw) = password {
        if token_user.is_none() {
            return Err(unauthorized("Personal token required to set password"));
        }
        Some(
            tokio::task::spawn_blocking(move || bcrypt::hash(pw, bcrypt::DEFAULT_COST))
                .await
                .map_err(|_| internal_error("Failed to hash password"))?
                .map_err(|_| internal_error("Failed to hash password"))?,
        )
    } else {
        None
    };

    let is_one_time = is_one_time.unwrap_or(false);
    if is_one_time && token_user.is_none() {
        return Err(unauthorized("Personal token required to set one-time download"));
    }

    let expires_at = Utc::now() + expiration.to_duration();
    let share_code = repository::reserve_share_code(&state.db).await?;
    let share_group_id = Uuid::new_v4().to_string();
    let user_id = token_user.as_ref().map(|u| u.user_id.clone());
    let api_key_id = token_user.as_ref().and_then(|u| u.api_key_id.clone());
    let mut uploaded_files: Vec<String> = Vec::new();

    for (idx, file) in staged_files.into_iter().enumerate() {
        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            user_id.clone(),
            api_key_id.clone(),
            share_code.clone(),
            file.name.clone(),
            file.size,
            file.content_type.clone(),
            "server".to_string(),
            file.storage_key,
            description.clone(),
            password_hash.clone(),
            is_one_time,
            false,
            expires_at,
            None,
            None,
            idx as i32,
            None,
            None,
            None,
        )
        .await
        .map_err(|e| internal_error(format!("Database save failed: {}", e)))?;

        uploaded_files.push(file_share.file_name);
    }

    if api_key_id_for_quota.is_none() {
        crate::utils::record_daily_usage(
            &state.db,
            token_user.as_ref().map(|u| u.user_id.as_str()),
            &headers,
            total_size,
        )
        .await;
    }

    let download_url = format!("{}/cli/shares/{}/download", state.config.server.base_url, share_code);
    let curl_command = format!("curl -OJ -H \"X-Personal-Token: $TOKEN\" {}", download_url);

    Ok(PrettyJson(CliUploadResponse {
        share_code,
        files: uploaded_files,
        curl_command,
        expires_at: format_expires_at(expires_at),
    }))
}

/// Create a P2P file share session via the CLI tool.
///
/// Registers file metadata and returns a share code for WebRTC-based peer-to-peer transfer.
/// Requires `X-Personal-Token` header for authentication.
#[utoipa::path(
    post,
    path = "/cli/p2p/create",
    tag = "cli",
    request_body = CliP2PCreateRequest,
    responses(
        (status = 200, description = "P2P session created", body = CliP2PCreateResponse),
        (status = 400, description = "Invalid request")
    )
)]
pub async fn cli_p2p_create(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Json(request): Json<CliP2PCreateRequest>,
) -> Result<PrettyJson<CliP2PCreateResponse>, AppError> {
    let token_user = token_user.map(|ext| ext.0.clone());

    if request.files.is_empty() {
        return Err(bad_request("At least one file is required"));
    }

    let password_hash = if let Some(pw) = &request.password {
        let pw = pw.clone();
        Some(
            tokio::task::spawn_blocking(move || bcrypt::hash(pw, bcrypt::DEFAULT_COST))
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
    let user_id = token_user.as_ref().map(|u| u.user_id.clone());
    let mut file_names: Vec<String> = Vec::new();

    for (idx, file_info) in request.files.iter().enumerate() {
        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            user_id.clone(),
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
            crate::utils::normalize_relative_path(file_info.relative_path.as_deref()),
            None,
        )
        .await
        .map_err(|e| internal_error(format!("Database save failed: {}", e)))?;

        file_names.push(file_share.file_name);
    }

    let empty_folders = crate::utils::normalize_empty_folders(&request.empty_folders);
    if !empty_folders.is_empty() {
        repository::create_empty_folders(&state.db, &share_code, &empty_folders)
            .await
            .map_err(|e| internal_error(format!("Failed to save empty folders: {}", e)))?;
    }

    Ok(PrettyJson(CliP2PCreateResponse {
        share_code,
        files: file_names,
        expires_at: format_expires_at(expires_at),
    }))
}

/// Initialize a multipart upload session.
///
/// Returns a `storage_key`, `upload_id`, and `total_parts` per file. The CLI then calls
/// `/cli/uploads/multipart/{id}/parts` for presigned URLs and PUTs bytes directly to R2.
#[utoipa::path(
    post,
    path = "/cli/uploads/multipart",
    tag = "cli",
    request_body = CliMultipartInitRequest,
    responses(
        (status = 200, description = "Upload session created", body = CliMultipartInitResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Personal token required for password / expiration / one-time"),
        (status = 413, description = "`file_too_large` — upload size limit exceeded. See https://shareany.app/api-terms-of-use for current limits."),
        (status = 429, description = "`storage_quota_exceeded` — API key storage quota exceeded (only on requests authenticated by an API key, `sak_`). See https://shareany.app/api-terms-of-use for current limits."),
        (status = 500, description = "Failed to create multipart upload on storage")
    )
)]
pub async fn cli_multipart_init(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CliMultipartInitRequest>,
) -> Result<Json<CliMultipartInitResponse>, AppError> {
    let token_user = token_user.map(|ext| ext.0.clone());

    if request.files.is_empty() {
        return Err(bad_request("At least one file is required"));
    }

    let total_size: i64 = request.files.iter().map(|f| f.file_size).sum();

    let api_key_id = token_user.as_ref().and_then(|u| u.api_key_id.clone());
    if let Some(ref kid) = api_key_id {
        let existing = repository::sum_active_storage_for_api_key(&state.db, kid)
            .await
            .map_err(|_| internal_error("Failed to compute API key storage usage"))?;
        if existing + total_size > API_KEY_ACTIVE_STORAGE_LIMIT {
            return Err(crate::models::storage_quota_exceeded(STORAGE_QUOTA_MESSAGE));
        }
    }

    if api_key_id.is_none() {
        crate::utils::enforce_daily_quota(
            &state.db,
            token_user.as_ref().map(|u| u.user_id.as_str()),
            &headers,
            total_size,
        )
        .await?;
    }

    let expiration = if let Some(exp_str) = &request.expiration {
        if token_user.is_none() {
            return Err(unauthorized("Personal token required to set expiration"));
        }
        parse_cli_expiration(exp_str)
            .ok_or_else(|| bad_request("Invalid expiration"))?
    } else {
        ExpirationPeriod::ThirtyMinutes
    };

    if request.password.is_some() && token_user.is_none() {
        return Err(unauthorized("Personal token required to set password"));
    }

    let is_one_time = request.is_one_time.unwrap_or(false);
    if is_one_time && token_user.is_none() {
        return Err(unauthorized("Personal token required to set one-time download"));
    }

    let share_code = repository::reserve_share_code(&state.db).await?;

    let upload_session_id = Uuid::new_v4().to_string();
    let chunk_size = request.chunk_size;
    let mut files: Vec<CliMultipartFileInit> = Vec::new();

    for file_info in &request.files {
        let storage_key = generate_storage_key(
            &state.config.s3.prefix,
            &Uuid::new_v4().to_string(),
            &file_info.file_name,
        );

        let total_parts = ((file_info.file_size as f64) / (chunk_size as f64)).ceil() as i32;

        let upload_id = state
            .storage
            .create_multipart_upload(&storage_key, &file_info.content_type)
            .await
            .map_err(|e| internal_error(format!("Failed to create multipart upload: {}", e)))?;

        files.push(CliMultipartFileInit {
            file_name: file_info.file_name.clone(),
            storage_key,
            upload_id,
            total_parts,
        });
    }

    let password_hash = if let Some(pw) = &request.password {
        let pw = pw.clone();
        Some(
            tokio::task::spawn_blocking(move || bcrypt::hash(pw, bcrypt::DEFAULT_COST))
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
        token_user.as_ref().map(|u| u.user_id.as_str()),
        request.description.as_deref(),
        password_hash.as_deref(),
        is_one_time,
        false,
        &expiration_period_str,
        session_expires_at,
        None,
    )
    .await
    .map_err(|e| internal_error(format!("Failed to create upload session: {}", e)))?;

    Ok(Json(CliMultipartInitResponse {
        upload_session_id,
        share_code,
        files,
        chunk_size,
    }))
}

/// Issue presigned PUT URLs for the requested part numbers of a multipart upload.
///
/// The CLI uses these URLs to PUT bytes directly to R2 without relaying through the backend.
#[utoipa::path(
    post,
    path = "/cli/uploads/multipart/{id}/parts",
    tag = "cli",
    params(
        ("id" = String, Path, description = "Upload session id returned by /cli/uploads/multipart")
    ),
    request_body = CliPresignPartsRequest,
    responses(
        (status = 200, description = "Presigned URLs issued", body = CliPresignPartsResponse),
        (status = 400, description = "Upload session already completed"),
        (status = 403, description = "Upload session belongs to another user"),
        (status = 404, description = "Upload session not found"),
        (status = 500, description = "Failed to generate presigned URL")
    )
)]
pub async fn cli_presign_parts(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Json(request): Json<CliPresignPartsRequest>,
) -> Result<Json<CliPresignPartsResponse>, AppError> {
    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|_| internal_error("Failed to get upload session"))?
        .ok_or_else(|| not_found("Upload session not found"))?;

    let token_user = token_user.map(|ext| ext.0);
    let token_user_id = token_user.as_ref().map(|u| u.user_id.as_str());

    if let Some(uid) = token_user_id {
        if session.user_id.as_deref() != Some(uid) {
            return Err(forbidden("Upload session does not belong to the authenticated user"));
        }
    }

    if session.completed {
        return Err(bad_request("Upload session already completed"));
    }

    let mut urls: Vec<CliPartUrl> = Vec::new();

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
            .map_err(|_| internal_error("Failed to generate presigned URL"))?;

        urls.push(CliPartUrl {
            part_number: *part_number,
            presigned_url,
        });
    }

    Ok(Json(CliPresignPartsResponse {
        storage_key: request.storage_key,
        urls,
        expires_in_secs: PRESIGNED_URL_EXPIRY_SECS,
    }))
}

/// Finalize a multipart upload session.
///
/// Send each file's part ETags (or `upload_id = "direct"` for single-part files). The server
/// calls S3 CompleteMultipartUpload where needed and creates the `file_share` rows.
#[utoipa::path(
    post,
    path = "/cli/uploads/multipart/{id}/complete",
    tag = "cli",
    params(
        ("id" = String, Path, description = "Upload session id returned by /cli/uploads/multipart")
    ),
    request_body = CliCompleteMultipartRequest,
    responses(
        (status = 200, description = "Upload finalized", body = CliUploadResponse),
        (status = 400, description = "Share code mismatch or session already completed"),
        (status = 403, description = "Upload session belongs to another user"),
        (status = 404, description = "Upload session not found"),
        (status = 500, description = "Failed to complete multipart upload on storage")
    )
)]
pub async fn cli_complete_multipart(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CliCompleteMultipartRequest>,
) -> Result<PrettyJson<CliUploadResponse>, AppError> {
    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|_| internal_error("Failed to get upload session"))?
        .ok_or_else(|| not_found("Upload session not found"))?;

    let token_user = token_user.map(|ext| ext.0);
    let token_user_id = token_user.as_ref().map(|u| u.user_id.as_str());

    if let Some(uid) = token_user_id {
        if session.user_id.as_deref() != Some(uid) {
            return Err(forbidden("Upload session does not belong to the authenticated user"));
        }
    }

    if session.share_code != request.share_code {
        return Err(bad_request("Share code mismatch"));
    }

    if session.completed {
        return Err(bad_request("Upload session already completed"));
    }

    let expiration_period = ExpirationPeriod::from_str(&session.expiration_period)
        .unwrap_or(ExpirationPeriod::ThirtyMinutes);
    let expires_at = Utc::now() + expiration_period.to_duration();

    let share_group_id = Uuid::new_v4().to_string();
    let api_key_id = token_user.as_ref().and_then(|u| u.api_key_id.clone());
    let mut uploaded_files: Vec<String> = Vec::new();

    for (idx, file_info) in request.files.iter().enumerate() {
        if file_info.upload_id != "direct" && !file_info.parts.is_empty() {
            let parts: Vec<(i32, String)> = file_info
                .parts
                .iter()
                .map(|p| (p.part_number, p.etag.clone()))
                .collect();

            state
                .storage
                .complete_multipart_upload(&file_info.storage_key, &file_info.upload_id, parts)
                .await
                .map_err(|e| internal_error(format!("Failed to complete multipart upload: {}", e)))?;
        }

        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            session.user_id.clone(),
            api_key_id.clone(),
            session.share_code.clone(),
            file_info.file_name.clone(),
            file_info.file_size,
            file_info.content_type.clone(),
            "server".to_string(),
            file_info.storage_key.clone(),
            session.description.clone(),
            session.password_hash.clone(),
            session.is_one_time,
            false,
            expires_at,
            None,
            None,
            idx as i32,
            None,
            None,
            None,
        )
        .await
        .map_err(|e| internal_error(format!("Database save failed: {}", e)))?;

        uploaded_files.push(file_share.file_name);
    }

    repository::complete_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|_| internal_error("Failed to complete upload session"))?;

    if api_key_id.is_none() {
        crate::utils::record_daily_usage(
            &state.db,
            session.user_id.as_deref(),
            &headers,
            request.files.iter().map(|f| f.file_size).sum(),
        )
        .await;
    }

    let download_url = format!(
        "{}/cli/shares/{}/download",
        state.config.server.base_url, session.share_code
    );
    let curl_command = format!("curl -OJ -H \"X-Personal-Token: $TOKEN\" {}", download_url);

    Ok(PrettyJson(CliUploadResponse {
        share_code: session.share_code,
        files: uploaded_files,
        curl_command,
        expires_at: format_expires_at(expires_at),
    }))
}

/// Download a shared file via the CLI tool.
///
/// Returns the raw file bytes with a `Content-Disposition` header carrying the original
/// filename. P2P shares are not downloadable through this endpoint — use the WebRTC flow.
#[utoipa::path(
    get,
    path = "/cli/shares/{code}/download",
    tag = "cli",
    params(
        ("code" = String, Path, description = "Share code"),
        CliDownloadQuery,
    ),
    responses(
        (status = 200, description = "File stream", content_type = "application/octet-stream"),
        (status = 400, description = "P2P share cannot be downloaded via this endpoint"),
        (status = 401, description = "Password required or incorrect"),
        (status = 404, description = "Share not found or expired")
    )
)]
pub async fn cli_download(
    State(state): State<CliState>,
    Path(code): Path<String>,
    Query(query): Query<CliDownloadQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .map_err(|_| internal_error("Failed to query files"))?;

    if file_shares.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let file_share = if let Some(file_id) = &query.file_id {
        file_shares
            .iter()
            .find(|f| f.id == *file_id)
            .ok_or_else(|| not_found("File not found"))?
    } else {
        &file_shares[0]
    };

    if file_share.transfer_type == "p2p" {
        return Err(bad_request("P2P files cannot be downloaded via CLI"));
    }

    if let Some(password_hash) = &file_share.password_hash {
        let password = query
            .password
            .as_deref()
            .or_else(|| {
                headers
                    .get("X-File-Password")
                    .and_then(|v| v.to_str().ok())
            })
            .ok_or_else(|| unauthorized("Password required. Use ?password=&lt;pw&gt; or X-File-Password header"))?;

        let password = password.to_string();
        let stored = password_hash.to_string();
        let is_valid = tokio::task::spawn_blocking(move || bcrypt::verify(&password, &stored))
            .await
            .map_err(|_| internal_error("Failed to verify password"))?
            .map_err(|_| internal_error("Failed to verify password"))?;

        if !is_valid {
            return Err(unauthorized("Incorrect password"));
        }
    }

    let ip_address = crate::utils::client_ip(&headers);

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let device_platform = user_agent.as_ref().map(|ua| parse_device_platform(ua));

    let _ = repository::create_download_log(
        &state.db,
        CreateDownloadLogDto {
            file_share_id: file_share.id.clone(),
            downloader_user_id: None,
            ip_address,
            user_agent,
        },
        device_platform,
    )
    .await;

    let s3_resp = state
        .storage
        .download_file_stream(&file_share.storage_key)
        .await?;
    let content_length = s3_resp.content_length();

    if file_share.is_one_time {
        let _ = state.storage.delete_file(&file_share.storage_key).await;
        let _ = repository::delete_file_share(&state.db, &file_share.id).await;
    }

    let async_read = s3_resp.body.into_async_read();
    let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(
        async_read,
        256 * 1024,
    ));

    let content_type: HeaderValue = file_share
        .file_type
        .parse()
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let content_disposition: HeaderValue =
        encode_content_disposition("attachment", &file_share.file_name)
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("attachment"));

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, content_disposition);
    if let Some(len) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, len.to_string());
    }

    builder
        .body(body)
        .map_err(|e| internal_error(format!("Failed to build response: {}", e)))
}

const CLI_DOWNLOAD_URL_EXPIRY_SECS: u64 = 3600;

/// Issue a short-lived presigned URL for downloading a shared file directly from object storage.
///
/// This is the fast path — the CLI fetches the URL here, then `GET`s the file straight from R2
/// without proxying through this server. Bypassing the backend saves a round trip per byte and
/// avoids the server's egress as a bottleneck on large files.
///
/// **One-time shares are consumed when the URL is issued.** The share row is deleted before
/// the URL is returned, so a second URL request returns `404`; the underlying object is kept
/// alive for slightly longer than the URL TTL and then deleted in the background. Issuing a
/// URL therefore counts as the download for one-time shares — there is no retry window.
///
/// For non-one-time shares, **this endpoint does not increment the download count**. Call
/// `POST /cli/shares/{code}/download-complete` after the file has been fully fetched from R2
/// so that the count reflects actual completions.
#[utoipa::path(
    post,
    path = "/cli/shares/{code}/download-url",
    tag = "cli",
    params(
        ("code" = String, Path, description = "Share code"),
        CliDownloadQuery,
    ),
    responses(
        (status = 200, description = "Presigned download URL issued", body = CliDownloadUrlResponse),
        (status = 400, description = "P2P share cannot be downloaded via this endpoint"),
        (status = 401, description = "Password required or incorrect"),
        (status = 404, description = "Share not found, expired, or one-time consumed")
    )
)]
pub async fn cli_download_url(
    State(state): State<CliState>,
    Path(code): Path<String>,
    Query(query): Query<CliDownloadQuery>,
    headers: HeaderMap,
) -> Result<PrettyJson<CliDownloadUrlResponse>, AppError> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .map_err(|_| internal_error("Failed to query files"))?;

    if file_shares.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let file_share = if let Some(file_id) = &query.file_id {
        file_shares
            .iter()
            .find(|f| f.id == *file_id)
            .ok_or_else(|| not_found("File not found"))?
    } else {
        &file_shares[0]
    };

    if file_share.transfer_type == "p2p" {
        return Err(bad_request("P2P files cannot be downloaded via CLI"));
    }

    if let Some(password_hash) = &file_share.password_hash {
        let password = query
            .password
            .as_deref()
            .or_else(|| {
                headers
                    .get("X-File-Password")
                    .and_then(|v| v.to_str().ok())
            })
            .ok_or_else(|| unauthorized("Password required. Use ?password=<pw> or X-File-Password header"))?;

        let password = password.to_string();
        let stored = password_hash.to_string();
        let is_valid = tokio::task::spawn_blocking(move || bcrypt::verify(&password, &stored))
            .await
            .map_err(|_| internal_error("Failed to verify password"))?
            .map_err(|_| internal_error("Failed to verify password"))?;

        if !is_valid {
            return Err(unauthorized("Incorrect password"));
        }
    }

    let download_url = state
        .storage
        .generate_presigned_get_url(
            &file_share.storage_key,
            CLI_DOWNLOAD_URL_EXPIRY_SECS,
            Some(&file_share.file_name),
        )
        .await
        .map_err(|e| internal_error(format!("Failed to create download URL: {}", e)))?;

    if file_share.is_one_time {
        let _ = repository::delete_file_share(&state.db, &file_share.id).await;

        let storage = state.storage.clone();
        let db = state.db.clone();
        let storage_key = file_share.storage_key.clone();
        let file_id = file_share.id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(
                CLI_DOWNLOAD_URL_EXPIRY_SECS + 60,
            ))
            .await;
            let _ = storage.delete_file(&storage_key).await;
            let _ = repository::delete_file_share(&db, &file_id).await;
        });
    }

    Ok(PrettyJson(CliDownloadUrlResponse {
        download_url,
        file_id: file_share.id.clone(),
        file_name: file_share.file_name.clone(),
        file_size: file_share.file_size,
        content_type: file_share.file_type.clone(),
        expires_in_secs: CLI_DOWNLOAD_URL_EXPIRY_SECS,
    }))
}

/// Record a successful direct download triggered by `POST /cli/shares/{code}/download-url`.
///
/// Inserts a `download_logs` row so the share's download count reflects actual completions.
/// Returns `204` even when the share has already been deleted (e.g. expired between URL-issue
/// and ping, or this was a one-time share whose row was removed at URL-issue time) — the call
/// is best-effort logging and never fails the client.
#[utoipa::path(
    post,
    path = "/cli/shares/{code}/download-complete",
    tag = "cli",
    params(
        ("code" = String, Path, description = "Share code"),
    ),
    request_body = CliDownloadCompleteRequest,
    responses(
        (status = 204, description = "Completion recorded (or no-op if share is already gone)"),
    )
)]
pub async fn cli_download_complete(
    State(state): State<CliState>,
    Path(code): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CliDownloadCompleteRequest>,
) -> Result<StatusCode, AppError> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .map_err(|_| internal_error("Failed to query files"))?;

    let Some(file_share) = file_shares.iter().find(|f| f.id == body.file_id) else {
        return Ok(StatusCode::NO_CONTENT);
    };

    let ip_address = crate::utils::client_ip(&headers);

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let device_platform = user_agent.as_ref().map(|ua| parse_device_platform(ua));

    let _ = repository::create_download_log(
        &state.db,
        CreateDownloadLogDto {
            file_share_id: file_share.id.clone(),
            downloader_user_id: None,
            ip_address,
            user_agent,
        },
        device_platform,
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// Get file metadata for a share code (used by the CLI before downloading).
///
/// Requires `X-Personal-Token` header for authentication.
#[utoipa::path(
    get,
    path = "/cli/download/{code}/info",
    tag = "cli",
    params(
        ("code" = String, Path, description = "Share code")
    ),
    responses(
        (status = 200, description = "File info returned", body = CliFileInfoResponse),
        (status = 404, description = "File not found or expired")
    )
)]
pub async fn cli_download_info(
    State(state): State<CliState>,
    Path(code): Path<String>,
) -> Result<PrettyJson<CliFileInfoResponse>, AppError> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .map_err(|_| internal_error("Failed to query files"))?;

    if file_shares.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let first = &file_shares[0];

    let files: Vec<CliFileDetail> = file_shares
        .iter()
        .map(|f| CliFileDetail {
            id: f.id.clone(),
            file_name: f.file_name.clone(),
            file_size: f.file_size,
        })
        .collect();

    Ok(PrettyJson(CliFileInfoResponse {
        share_code: code,
        files,
        has_password: first.password_hash.is_some(),
        is_one_time: first.is_one_time,
        expires_at: format_expires_at(first.expires_at),
        transfer_type: first.transfer_type.clone(),
    }))
}

pub async fn cli_file_list(
    State(state): State<CliState>,
    Path(code): Path<String>,
) -> Result<PrettyJson<CliFileListResponse>, AppError> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .map_err(|_| internal_error("Failed to query files"))?;

    if file_shares.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let first = &file_shares[0];

    let files: Vec<CliFileDetail> = file_shares
        .iter()
        .map(|f| CliFileDetail {
            id: f.id.clone(),
            file_name: f.file_name.clone(),
            file_size: f.file_size,
        })
        .collect();

    let total_count = files.len();

    let empty_folders = repository::find_empty_folders_by_code(&state.db, &code)
        .await
        .unwrap_or_default();

    Ok(PrettyJson(CliFileListResponse {
        share_code: code,
        files,
        total_count,
        has_password: first.password_hash.is_some(),
        is_one_time: first.is_one_time,
        expires_at: format_expires_at(first.expires_at),
        transfer_type: first.transfer_type.clone(),
        empty_folders,
    }))
}

/// List the authenticated user's recent uploads.
#[utoipa::path(
    get,
    path = "/cli/me/uploads",
    tag = "cli",
    params(CliUploadHistoryQuery),
    responses(
        (status = 200, description = "Upload history", body = CliUploadHistoryResponse),
        (status = 401, description = "Missing or invalid personal token")
    )
)]
pub async fn cli_upload_history(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Query(query): Query<CliUploadHistoryQuery>,
) -> Result<PrettyJson<serde_json::Value>, AppError> {
    let token_user = token_user
        .ok_or_else(|| unauthorized("Personal token required. Set the 'X-Personal-Token' header."))?;
    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0);

    let file_shares = repository::find_file_shares_by_user(&state.db, &token_user.user_id, limit, offset)
        .await
        .map_err(|e| internal_error(format!("Failed to fetch upload history: {}", e)))?;

    let uploads: Vec<serde_json::Value> = file_shares
        .iter()
        .map(|f| {
            serde_json::json!({
                "share_code": f.share_code,
                "file_name": f.file_name,
                "file_size": f.file_size,
                "expires_at": format_expires_at(f.expires_at),
                "created_at": f.created_at.format("%Y-%m-%d %H:%M").to_string(),
            })
        })
        .collect();

    Ok(PrettyJson(serde_json::json!({
        "uploads": uploads,
        "count": uploads.len(),
    })))
}

/// List download logs for one of the authenticated user's shares.
#[utoipa::path(
    get,
    path = "/cli/me/uploads/{code}/downloads",
    tag = "cli",
    params(
        ("code" = String, Path, description = "Share code owned by the authenticated user")
    ),
    responses(
        (status = 200, description = "Per-share download logs", body = CliShareLogsResponse),
        (status = 401, description = "Missing or invalid personal token"),
        (status = 403, description = "Share belongs to another user"),
        (status = 404, description = "Share not found")
    )
)]
pub async fn cli_share_logs(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Path(share_code): Path<String>,
) -> Result<PrettyJson<serde_json::Value>, AppError> {
    let token_user = token_user
        .ok_or_else(|| unauthorized("Personal token required. Set the 'X-Personal-Token' header."))?;
    let file_shares = repository::find_file_shares_by_code(&state.db, &share_code)
        .await
        .map_err(|e| internal_error(format!("Failed to look up share: {}", e)))?;

    if file_shares.is_empty() {
        return Err(not_found("Share code not found"));
    }

    for fs in &file_shares {
        if fs.user_id.as_ref() != Some(&token_user.user_id) {
            return Err(forbidden("You do not have permission to access this share"));
        }
    }

    let mut downloads: Vec<serde_json::Value> = Vec::new();
    for fs in &file_shares {
        let rows = repository::find_download_logs_with_downloader_name_by_file_share(&state.db, &fs.id)
            .await
            .map_err(|e| internal_error(format!("Failed to fetch logs: {}", e)))?;
        for (log, downloader_name) in rows {
            downloads.push(serde_json::json!({
                "file_name": fs.file_name,
                "downloader_name": downloader_name,
                "ip_address": log.ip_address,
                "device_platform": log.device_platform.unwrap_or_else(|| "Unknown".to_string()),
                "downloaded_at": log.downloaded_at.format("%Y-%m-%d %H:%M").to_string(),
            }));
        }
    }

    downloads.sort_by(|a, b| {
        b["downloaded_at"]
            .as_str()
            .unwrap_or("")
            .cmp(a["downloaded_at"].as_str().unwrap_or(""))
    });

    Ok(PrettyJson(serde_json::json!({
        "share_code": share_code,
        "downloads": downloads,
        "count": downloads.len(),
    })))
}

/// Delete one of the authenticated user's shares (and its file in storage).
#[utoipa::path(
    delete,
    path = "/cli/me/uploads/{code}",
    tag = "cli",
    params(
        ("code" = String, Path, description = "Share code owned by the authenticated user")
    ),
    responses(
        (status = 204, description = "Share deleted"),
        (status = 401, description = "Missing or invalid personal token"),
        (status = 403, description = "Share belongs to another user"),
        (status = 404, description = "Share not found"),
        (status = 500, description = "Failed to delete from storage or database")
    )
)]
pub async fn cli_delete_upload(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Path(share_code): Path<String>,
) -> Result<StatusCode, AppError> {
    let token_user = token_user
        .ok_or_else(|| unauthorized("Personal token required. Set the 'X-Personal-Token' header."))?;
    let file_shares = repository::find_file_shares_by_code(&state.db, &share_code)
        .await
        .map_err(|e| internal_error(format!("Failed to look up share: {}", e)))?;

    if file_shares.is_empty() {
        return Err(not_found("Share code not found"));
    }

    for fs in &file_shares {
        if fs.user_id.as_ref() != Some(&token_user.user_id) {
            return Err(forbidden("You do not have permission to access this share"));
        }
    }

    for fs in &file_shares {
        if !fs.storage_key.is_empty() {
            state
                .storage
                .delete_file(&fs.storage_key)
                .await
                .map_err(|e| internal_error(format!("Failed to delete from storage: {}", e)))?;
        }
        repository::delete_file_share(&state.db, &fs.id)
            .await
            .map_err(|e| internal_error(format!("Failed to delete from DB: {}", e)))?;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// List the authenticated user's recent downloads.
#[utoipa::path(
    get,
    path = "/cli/me/downloads",
    tag = "cli",
    params(CliUploadHistoryQuery),
    responses(
        (status = 200, description = "Download history", body = CliDownloadHistoryResponse),
        (status = 401, description = "Missing or invalid personal token")
    )
)]
pub async fn cli_download_history(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Query(query): Query<CliUploadHistoryQuery>,
) -> Result<PrettyJson<serde_json::Value>, AppError> {
    let token_user = token_user
        .ok_or_else(|| unauthorized("Personal token required. Set the 'X-Personal-Token' header."))?;
    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0);

    let logs = repository::find_download_logs_by_user(&state.db, &token_user.user_id, limit, offset)
        .await
        .map_err(|e| internal_error(format!("Failed to fetch download history: {}", e)))?;

    let downloads: Vec<serde_json::Value> = logs
        .iter()
        .map(|(log, share_code, file_name, file_size)| {
            serde_json::json!({
                "share_code": share_code,
                "file_name": file_name,
                "file_size": file_size,
                "ip_address": log.ip_address,
                "downloaded_at": log.downloaded_at.format("%Y-%m-%d %H:%M").to_string(),
            })
        })
        .collect();

    Ok(PrettyJson(serde_json::json!({
        "downloads": downloads,
        "count": downloads.len(),
    })))
}

/// Get the authenticated CLI user's profile and token usage info.
///
/// Requires `X-Personal-Token` header for authentication.
#[utoipa::path(
    get,
    path = "/cli/me",
    tag = "cli",
    responses(
        (status = 200, description = "User profile returned", body = CliMeResponse),
        (status = 401, description = "Unauthorized — missing or invalid personal token"),
        (status = 404, description = "User not found")
    )
)]
pub async fn cli_me(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
) -> Result<PrettyJson<serde_json::Value>, AppError> {
    let token_user = token_user
        .ok_or_else(|| unauthorized("Personal token required. Set the 'X-Personal-Token' header."))?;
    let user = repository::find_user_by_id(&state.db, &token_user.user_id)
        .await
        .map_err(|e| internal_error(format!("Failed to fetch user: {}", e)))?
        .ok_or_else(|| not_found("User not found"))?;

    let token = repository::find_personal_token_by_id(&state.db, &token_user.personal_token_id)
        .await
        .map_err(|e| internal_error(format!("Failed to fetch token: {}", e)))?;

    let last_used_at = token
        .and_then(|t| t.last_used_at)
        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string());

    Ok(PrettyJson(serde_json::json!({
        "name": user.name,
        "email": user.email,
        "last_used_at": last_used_at,
    })))
}

pub async fn cli_install_script() -> Response {
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(
            header::LOCATION,
            "https://raw.githubusercontent.com/bestdevmgp/share-anything-cli/main/install.sh",
        )
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
