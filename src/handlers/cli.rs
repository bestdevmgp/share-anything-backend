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
        CliDownloadQuery, CliUploadHistoryQuery,
    },
    services::StorageService,
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

const CLI_GUEST_MAX_FILE_SIZE: i64 = 100 * 1024 * 1024;
const CLI_AUTH_MAX_FILE_SIZE: i64 = 3 * 1024 * 1024 * 1024;
const PRESIGNED_URL_EXPIRY_SECS: u64 = 3600;

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

pub async fn cli_upload(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    mut multipart: Multipart,
) -> Result<PrettyJson<CliUploadResponse>, AppError> {
    let token_user = token_user.map(|ext| ext.0.clone());

    struct FileData {
        name: String,
        data: Vec<u8>,
        content_type: String,
    }

    let mut files: Vec<FileData> = Vec::new();
    let mut description: Option<String> = None;
    let mut password: Option<String> = None;
    let mut expiration_str: Option<String> = None;
    let mut is_one_time: Option<bool> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| bad_request("Failed to parse multipart data"))?
    {
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
                let data = field
                    .bytes()
                    .await
                    .map_err(|_| bad_request("Failed to read file data"))?;

                files.push(FileData {
                    name: file_name,
                    data: data.to_vec(),
                    content_type,
                });
            }
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
            _ => {}
        }
    }

    if files.is_empty() {
        return Err(bad_request("No files were uploaded"));
    }

    let max_size = if token_user.is_some() {
        CLI_AUTH_MAX_FILE_SIZE
    } else {
        CLI_GUEST_MAX_FILE_SIZE
    };

    let total_size: i64 = files.iter().map(|f| f.data.len() as i64).sum();
    if total_size > max_size {
        return Err(bad_request(format!(
            "File size limit exceeded ({}MB / {}MB)",
            total_size / 1024 / 1024,
            max_size / 1024 / 1024
        )));
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
        Some(bcrypt::hash(pw, bcrypt::DEFAULT_COST).map_err(|_| internal_error("Failed to hash password"))?)
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
    let mut uploaded_files: Vec<String> = Vec::new();

    for file_data in files {
        let file_size = file_data.data.len() as i64;
        let storage_key = generate_storage_key(
            &state.config.s3.prefix,
            &Uuid::new_v4().to_string(),
            &file_data.name,
        );

        state
            .storage
            .upload_file(&storage_key, file_data.data, &file_data.content_type)
            .await
            .map_err(|e| internal_error(format!("Storage upload failed: {}", e)))?;

        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            user_id.clone(),
            share_code.clone(),
            file_data.name.clone(),
            file_size,
            file_data.content_type.clone(),
            "server".to_string(),
            storage_key,
            description.clone(),
            password_hash.clone(),
            is_one_time,
            false,
            expires_at,
            None,
            None,
        )
        .await
        .map_err(|e| internal_error(format!("Database save failed: {}", e)))?;

        uploaded_files.push(file_share.file_name);
    }

    let download_url = format!("{}/v1/shares/{}/download", state.config.server.base_url, share_code);
    let curl_command = format!("curl -OJ -H \"X-Personal-Token: $TOKEN\" {}", download_url);

    Ok(PrettyJson(CliUploadResponse {
        share_code,
        files: uploaded_files,
        curl_command,
        expires_at: format_expires_at(expires_at),
    }))
}

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
        Some(bcrypt::hash(pw, bcrypt::DEFAULT_COST).map_err(|_| internal_error("Failed to hash password"))?)
    } else {
        None
    };

    let share_code = repository::reserve_share_code(&state.db).await?;

    let expires_at = Utc::now() + chrono::Duration::hours(24);
    let share_group_id = Uuid::new_v4().to_string();
    let user_id = token_user.as_ref().map(|u| u.user_id.clone());
    let mut file_names: Vec<String> = Vec::new();

    for file_info in &request.files {
        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            user_id.clone(),
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
        )
        .await
        .map_err(|e| internal_error(format!("Database save failed: {}", e)))?;

        file_names.push(file_share.file_name);
    }

    Ok(PrettyJson(CliP2PCreateResponse {
        share_code,
        files: file_names,
        expires_at: format_expires_at(expires_at),
    }))
}

pub async fn cli_multipart_init(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Json(request): Json<CliMultipartInitRequest>,
) -> Result<Json<CliMultipartInitResponse>, AppError> {
    let token_user = token_user.map(|ext| ext.0.clone());

    if request.files.is_empty() {
        return Err(bad_request("At least one file is required"));
    }

    let max_size = if token_user.is_some() {
        CLI_AUTH_MAX_FILE_SIZE
    } else {
        CLI_GUEST_MAX_FILE_SIZE
    };

    let total_size: i64 = request.files.iter().map(|f| f.file_size).sum();
    if total_size > max_size {
        return Err(bad_request(format!(
            "File size limit exceeded ({}MB / {}MB)",
            total_size / 1024 / 1024,
            max_size / 1024 / 1024
        )));
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

        let upload_id = if total_parts > 1 {
            state
                .storage
                .create_multipart_upload(&storage_key, &file_info.content_type)
                .await
                .map_err(|e| internal_error(format!("Failed to create multipart upload: {}", e)))?
        } else {
            String::new()
        };

        files.push(CliMultipartFileInit {
            file_name: file_info.file_name.clone(),
            storage_key,
            upload_id,
            total_parts,
        });
    }

    let password_hash = if let Some(pw) = &request.password {
        Some(bcrypt::hash(pw, bcrypt::DEFAULT_COST).map_err(|_| internal_error("Failed to hash password"))?)
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

pub async fn cli_complete_multipart(
    State(state): State<CliState>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
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
    let mut uploaded_files: Vec<String> = Vec::new();

    for file_info in &request.files {
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
        )
        .await
        .map_err(|e| internal_error(format!("Database save failed: {}", e)))?;

        uploaded_files.push(file_share.file_name);
    }

    repository::complete_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|_| internal_error("Failed to complete upload session"))?;

    let download_url = format!(
        "{}/v1/shares/{}/download",
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

        let is_valid = bcrypt::verify(password, password_hash)
            .map_err(|_| internal_error("Failed to verify password"))?;

        if !is_valid {
            return Err(unauthorized("Incorrect password"));
        }
    }

    let ip_address = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim())
        .or_else(|| headers.get("X-Real-IP").and_then(|v| v.to_str().ok()))
        .unwrap_or("unknown")
        .to_string();

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

    let file_data = state
        .storage
        .download_file(&file_share.storage_key)
        .await
        .map_err(|e| internal_error(format!("File download failed: {}", e)))?;

    if file_share.is_one_time {
        let _ = state.storage.delete_file(&file_share.storage_key).await;
        let _ = repository::delete_file_share(&state.db, &file_share.id).await;
    }

    let mut response = Response::new(Body::from(file_data));

    response.headers_mut().insert(
        header::CONTENT_TYPE,
        file_share
            .file_type
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );

    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        encode_content_disposition("attachment", &file_share.file_name)
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );

    Ok(response)
}

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
            file_name: f.file_name.clone(),
            file_size: f.file_size,
        })
        .collect();

    let total_count = files.len();

    Ok(PrettyJson(CliFileListResponse {
        share_code: code,
        files,
        total_count,
        has_password: first.password_hash.is_some(),
        is_one_time: first.is_one_time,
        expires_at: format_expires_at(first.expires_at),
    }))
}

pub async fn cli_upload_history(
    State(state): State<CliState>,
    token_user: axum::extract::Extension<PersonalTokenUser>,
    Query(query): Query<CliUploadHistoryQuery>,
) -> Result<PrettyJson<serde_json::Value>, AppError> {
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

pub async fn cli_share_logs(
    State(state): State<CliState>,
    token_user: axum::extract::Extension<PersonalTokenUser>,
    Path(share_code): Path<String>,
) -> Result<PrettyJson<serde_json::Value>, AppError> {
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

pub async fn cli_delete_upload(
    State(state): State<CliState>,
    token_user: axum::extract::Extension<PersonalTokenUser>,
    Path(share_code): Path<String>,
) -> Result<StatusCode, AppError> {
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

pub async fn cli_download_history(
    State(state): State<CliState>,
    token_user: axum::extract::Extension<PersonalTokenUser>,
    Query(query): Query<CliUploadHistoryQuery>,
) -> Result<PrettyJson<serde_json::Value>, AppError> {
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

pub async fn cli_me(
    State(state): State<CliState>,
    token_user: axum::extract::Extension<PersonalTokenUser>,
) -> Result<PrettyJson<serde_json::Value>, AppError> {
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
