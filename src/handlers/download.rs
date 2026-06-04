use axum::{
    body::Body,
    extract::{Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
    Json,
};
use futures::StreamExt as _;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{
        bad_request, unauthorized, forbidden, not_found, internal_error, AppError,
        CreateDownloadLogDto, FileListResponse, FileInfoInGroup, DownloadFilesRequest,
        DownloadQuery, VerifyPasswordRequest, DownloadUrlResponse, FileInfoResponse,
    },
    services::{NotificationService, StorageService, signaling::SignalingState, email::FileNotificationInfo},
    utils::{encode_content_disposition, parse_device_platform, verify_turnstile_token, extract_client_ip},
};
use std::io::{Write as _, Cursor};
use zip::write::{FileOptions, ZipWriter};

#[derive(Clone)]
pub struct DownloadState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
    pub signaling: SignalingState,
    pub notifications: Arc<NotificationService>,
}

#[utoipa::path(
    get,
    path = "/files/list",
    tag = "download",
    params(
        ("code" = String, Query, description = "6-digit share code")
    ),
    responses(
        (status = 200, description = "File list retrieved successfully", body = FileListResponse),
        (status = 404, description = "Files not found")
    )
)]
pub async fn get_file_list(
    State(state): State<DownloadState>,
    Query(query): Query<DownloadQuery>,
    headers: HeaderMap,
) -> Result<Json<FileListResponse>, AppError> {
    let turnstile_token = headers
        .get("X-Turnstile-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| bad_request("Security verification required"))?;

    let client_ip = extract_client_ip(&headers);

    verify_turnstile_token(&state.config.turnstile.secret_key, turnstile_token, Some(client_ip))
        .await
        .map_err(|_| forbidden("Security verification failed. Please try again."))?;

    let rows = repository::find_file_shares_by_code_with_uploader(&state.db, &query.code).await?;

    if rows.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let uploader_name = rows[0].1.clone();
    let first_file = &rows[0].0;
    let total_count = rows.len();
    let description = first_file.description.clone();
    let has_password = rows.iter().any(|(f, _)| f.password_hash.is_some());
    let is_one_time = first_file.is_one_time;
    let transfer_type = first_file.transfer_type.clone();
    let expires_at = first_file.expires_at;

    let files: Vec<FileInfoInGroup> = rows
        .iter()
        .map(|(f, _)| FileInfoInGroup {
            id: f.id.clone(),
            file_name: f.file_name.clone(),
            file_size: f.file_size,
            file_type: f.file_type.clone(),
            image_width: f.image_width,
            image_height: f.image_height,
        })
        .collect();

    let uploader_online = if transfer_type == "p2p" {
        Some(state.signaling.find_uploader(&query.code).is_some())
    } else {
        None
    };

    Ok(Json(FileListResponse {
        share_code: query.code,
        files,
        total_count,
        description,
        has_password,
        is_one_time,
        transfer_type,
        expires_at,
        uploader_name,
        uploader_online,
    }))
}

#[utoipa::path(
    get,
    path = "/file/info",
    tag = "download",
    params(
        ("code" = String, Query, description = "6-digit share code")
    ),
    responses(
        (status = 200, description = "File information retrieved successfully", body = FileInfoResponse),
        (status = 404, description = "File not found")
    )
)]
pub async fn get_file_info(
    State(state): State<DownloadState>,
    Query(query): Query<DownloadQuery>,
    headers: HeaderMap,
) -> Result<Json<FileInfoResponse>, AppError> {
    let turnstile_token = headers
        .get("X-Turnstile-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| bad_request("Security verification required"))?;

    let client_ip = extract_client_ip(&headers);

    verify_turnstile_token(&state.config.turnstile.secret_key, turnstile_token, Some(client_ip))
        .await
        .map_err(|_| forbidden("Security verification failed. Please try again."))?;

    let (file_share, uploader_name) =
        repository::find_file_share_by_code_with_uploader(&state.db, &query.code)
            .await?
            .ok_or_else(|| not_found("File not found or expired"))?;

    let uploader_online = if file_share.transfer_type == "p2p" {
        Some(state.signaling.find_uploader(&query.code).is_some())
    } else {
        None
    };

    Ok(Json(FileInfoResponse {
        file_name: file_share.file_name,
        file_size: file_share.file_size,
        file_type: file_share.file_type,
        transfer_type: file_share.transfer_type.clone(),
        description: file_share.description,
        has_password: file_share.password_hash.is_some(),
        is_one_time: file_share.is_one_time,
        expires_at: file_share.expires_at,
        uploader_online,
        uploader_name,
    }))
}

#[utoipa::path(
    get,
    path = "/download/file",
    tag = "download",
    params(
        ("code" = String, Query, description = "6-digit share code"),
        ("file_id" = String, Query, description = "File ID to download")
    ),
    responses(
        (status = 200, description = "File downloaded successfully", content_type = "application/octet-stream"),
        (status = 401, description = "Password required or invalid"),
        (status = 404, description = "File not found")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn download_single_file(
    State(state): State<DownloadState>,
    Query(params): Query<serde_json::Value>,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, AppError> {
    let code = params
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("code parameter is required"))?;

    let file_id = params
        .get("file_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("file_id parameter is required"))?;

    let file_shares = repository::find_file_shares_by_code(&state.db, code)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?;

    if file_shares.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let file_share = file_shares
        .iter()
        .find(|f| f.id == file_id)
        .ok_or_else(|| not_found("File not found"))?;

    if file_share.transfer_type == "p2p" {
        return Err(forbidden(
            "This file is configured for P2P transfer. Use WebRTC connection.",
        ));
    }

    if let Some(password_hash) = &file_share.password_hash {
        let password = headers
            .get("X-File-Password")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| unauthorized("Password required. Set the X-File-Password header."))?;

        let is_valid = bcrypt::verify(password, password_hash)
            .map_err(|_| internal_error("Password verification failed"))?;

        if !is_valid {
            return Err(unauthorized("Incorrect password"));
        }
    }

    let user_claims = request.extensions().get::<Claims>().cloned();

    let ip_address = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim())
        .or_else(|| {
            headers
                .get("X-Real-IP")
                .and_then(|v| v.to_str().ok())
        })
        .unwrap_or("unknown")
        .to_string();

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let device_platform = user_agent.as_ref().map(|ua| parse_device_platform(ua));

    let ip_address_for_alert = ip_address.clone();

    let _ = repository::create_download_log(
        &state.db,
        CreateDownloadLogDto {
            file_share_id: file_share.id.clone(),
            downloader_user_id: user_claims.as_ref().map(|c| c.sub.clone()),
            ip_address,
            user_agent,
        },
        device_platform,
    )
    .await;

    state
        .notifications
        .notify_download(
            code,
            vec![FileNotificationInfo {
                file_name: file_share.file_name.clone(),
                file_size: file_share.file_size,
                file_type: file_share.file_type.clone(),
            }],
            user_claims.as_ref().map(|c| c.sub.as_str()),
            file_share.user_id.as_deref(),
            &ip_address_for_alert,
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
    let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(async_read, 256 * 1024));

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

    builder.body(body).map_err(|e| internal_error(format!("Failed to build response: {}", e)))
}

#[utoipa::path(
    get,
    path = "/preview/file",
    tag = "download",
    params(
        ("code" = String, Query, description = "6-digit share code"),
        ("file_id" = String, Query, description = "File ID to preview")
    ),
    responses(
        (status = 200, description = "File preview loaded successfully", content_type = "application/octet-stream"),
        (status = 401, description = "Password required or invalid"),
        (status = 404, description = "File not found")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn preview_file(
    State(state): State<DownloadState>,
    Query(params): Query<serde_json::Value>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let code = params
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("code parameter is required"))?;

    let file_id = params
        .get("file_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("file_id parameter is required"))?;

    let file_shares = repository::find_file_shares_by_code(&state.db, code)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?;

    if file_shares.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let file_share = file_shares
        .iter()
        .find(|f| f.id == file_id)
        .ok_or_else(|| not_found("File not found"))?;

    if let Some(password_hash) = &file_share.password_hash {
        let password = headers
            .get("X-File-Password")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| unauthorized("Password required. Set the X-File-Password header."))?;

        let is_valid = bcrypt::verify(password, password_hash)
            .map_err(|_| internal_error("Password verification failed"))?;

        if !is_valid {
            return Err(unauthorized("Incorrect password"));
        }
    }

    let s3_resp = state
        .storage
        .download_file_stream(&file_share.storage_key)
        .await?;

    let content_length = s3_resp.content_length();

    let async_read = s3_resp.body.into_async_read();
    let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(async_read, 256 * 1024));

    let content_type: HeaderValue = file_share
        .file_type
        .parse()
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));

    let content_disposition: HeaderValue =
        encode_content_disposition("inline", &file_share.file_name)
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("inline"));

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, content_disposition);

    if let Some(len) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, len.to_string());
    }

    builder.body(body).map_err(|e| internal_error(format!("Failed to build response: {}", e)))
}

#[utoipa::path(
    get,
    path = "/download",
    tag = "download",
    params(
        ("code" = String, Query, description = "6-digit share code")
    ),
    responses(
        (status = 200, description = "File downloaded successfully", content_type = "application/octet-stream"),
        (status = 401, description = "Password required or invalid"),
        (status = 404, description = "File not found")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn download_file(
    State(state): State<DownloadState>,
    Query(query): Query<DownloadQuery>,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, AppError> {
    let file_share = repository::find_file_share_by_code(&state.db, &query.code)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?
        .ok_or_else(|| not_found("File not found or expired"))?;

    if let Some(password_hash) = &file_share.password_hash {
        let password = headers
            .get("X-File-Password")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| unauthorized("Password required. Set the X-File-Password header."))?;

        let is_valid = bcrypt::verify(password, password_hash)
            .map_err(|_| internal_error("Password verification failed"))?;

        if !is_valid {
            return Err(unauthorized("Incorrect password"));
        }
    }

    let user_claims = request.extensions().get::<Claims>().cloned();

    let ip_address = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim())
        .or_else(|| {
            headers
                .get("X-Real-IP")
                .and_then(|v| v.to_str().ok())
        })
        .unwrap_or("unknown")
        .to_string();

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let device_platform = user_agent.as_ref().map(|ua| parse_device_platform(ua));

    let ip_address_for_alert = ip_address.clone();

    let _ = repository::create_download_log(
        &state.db,
        CreateDownloadLogDto {
            file_share_id: file_share.id.clone(),
            downloader_user_id: user_claims.as_ref().map(|c| c.sub.clone()),
            ip_address,
            user_agent,
        },
        device_platform,
    )
    .await;

    state
        .notifications
        .notify_download(
            &query.code,
            vec![FileNotificationInfo {
                file_name: file_share.file_name.clone(),
                file_size: file_share.file_size,
                file_type: file_share.file_type.clone(),
            }],
            user_claims.as_ref().map(|c| c.sub.as_str()),
            file_share.user_id.as_deref(),
            &ip_address_for_alert,
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
    let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(async_read, 256 * 1024));

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

    builder.body(body).map_err(|e| internal_error(format!("Failed to build response: {}", e)))
}

#[utoipa::path(
    post,
    path = "/download/bulk",
    tag = "download",
    request_body = DownloadFilesRequest,
    responses(
        (status = 200, description = "Files downloaded as ZIP successfully", content_type = "application/zip"),
        (status = 401, description = "Password required or invalid"),
        (status = 404, description = "Files not found")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn download_multiple_files(
    State(state): State<DownloadState>,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, AppError> {
    let user_claims = request.extensions().get::<Claims>().cloned();

    let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| bad_request("Cannot read request body"))?;

    let req: DownloadFilesRequest = serde_json::from_slice(&body_bytes)
        .map_err(|_| bad_request("Invalid request format"))?;

    if req.file_ids.is_empty() {
        return Err(bad_request("At least one file ID is required"));
    }

    let all_files = repository::find_file_shares_by_code(&state.db, &req.code)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?;

    if all_files.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let files_to_download: Vec<_> = all_files
        .iter()
        .filter(|f| req.file_ids.contains(&f.id))
        .collect();

    if files_to_download.is_empty() {
        return Err(not_found("Requested files not found"));
    }

    if let Some(password_hash) = &files_to_download[0].password_hash {
        if let Some(password) = &req.password {
            let is_valid = bcrypt::verify(password, password_hash)
                .map_err(|_| internal_error("Password verification failed"))?;

            if !is_valid {
                return Err(unauthorized("Incorrect password"));
            }
        } else {
            return Err(unauthorized("Password required"));
        }
    }

    let ip_address = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim())
        .or_else(|| {
            headers
                .get("X-Real-IP")
                .and_then(|v| v.to_str().ok())
        })
        .unwrap_or("unknown")
        .to_string();

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let device_platform = user_agent.as_ref().map(|ua| parse_device_platform(ua));

    // Pre-collect keys so closures don't capture a double-reference across a lifetime boundary.
    let file_meta: Vec<(usize, String, String)> = files_to_download
        .iter()
        .enumerate()
        .map(|(idx, f)| (idx, f.storage_key.clone(), f.file_name.clone()))
        .collect();

    let fetches = file_meta.into_iter().map(|(idx, key, name)| {
        let storage = state.storage.clone();
        async move {
            let bytes = storage
                .download_file(&key)
                .await
                .map_err(|e| internal_error(format!("Failed to download file from storage: {}", e)))?;
            Ok::<_, AppError>((idx, name, bytes))
        }
    });

    let mut fetch_stream = futures::stream::iter(fetches).buffer_unordered(4);
    let mut fetched: HashMap<usize, (String, Vec<u8>)> = HashMap::new();
    while let Some(result) = fetch_stream.next().await {
        let (idx, name, bytes) = result?;
        fetched.insert(idx, (name, bytes));
    }

    let buffer = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(buffer);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    let mut log_dtos: Vec<CreateDownloadLogDto> = Vec::with_capacity(files_to_download.len());

    for (idx, file_share) in files_to_download.iter().enumerate() {
        let (name, file_data) = fetched.remove(&idx).expect("all files fetched");

        zip.start_file(&name, options)
            .map_err(|_| internal_error("Failed to create ZIP file"))?;

        zip.write_all(&file_data)
            .map_err(|_| internal_error("Failed to write ZIP file"))?;

        log_dtos.push(CreateDownloadLogDto {
            file_share_id: file_share.id.clone(),
            downloader_user_id: user_claims.as_ref().map(|c| c.sub.clone()),
            ip_address: ip_address.clone(),
            user_agent: user_agent.clone(),
        });
    }

    if let Err(e) = repository::batch_create_download_logs(
        &state.db,
        log_dtos,
        device_platform.clone(),
    )
    .await
    {
        tracing::warn!(error = %e, "Failed to record bulk download logs");
    }

    let buffer = zip
        .finish()
        .map_err(|_| internal_error("Failed to finalize ZIP file"))?;

    let zip_data = buffer.into_inner();

    let notification_files: Vec<FileNotificationInfo> = files_to_download
        .iter()
        .map(|f| FileNotificationInfo {
            file_name: f.file_name.clone(),
            file_size: f.file_size,
            file_type: f.file_type.clone(),
        })
        .collect();
    state
        .notifications
        .notify_download(
            &req.code,
            notification_files,
            user_claims.as_ref().map(|c| c.sub.as_str()),
            files_to_download[0].user_id.as_deref(),
            &ip_address,
        )
        .await;

    let is_one_time = files_to_download[0].is_one_time;
    if is_one_time {
        for file_share in files_to_download.iter() {
            let _ = state.storage.delete_file(&file_share.storage_key).await;

            let _ = repository::delete_file_share(&state.db, &file_share.id).await;
        }
    }

    let mut response = Response::new(Body::from(zip_data));

    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));

    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"files_{}.zip\"", req.code)
            .parse()
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );

    Ok(response)
}

#[utoipa::path(
    post,
    path = "/file/verify-password",
    tag = "download",
    request_body = VerifyPasswordRequest,
    responses(
        (status = 200, description = "Password is valid"),
        (status = 401, description = "Password is invalid"),
        (status = 404, description = "File not found")
    )
)]
pub async fn verify_password(
    State(state): State<DownloadState>,
    Json(req): Json<VerifyPasswordRequest>,
) -> Result<StatusCode, AppError> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &req.code)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?;

    if file_shares.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let password_hash = file_shares
        .iter()
        .find_map(|f| f.password_hash.as_deref());

    let Some(hash) = password_hash else {
        return Ok(StatusCode::OK);
    };

    let is_valid = bcrypt::verify(&req.password, hash)
        .map_err(|_| internal_error("Password verification failed"))?;

    if is_valid {
        Ok(StatusCode::OK)
    } else {
        Err(unauthorized("Incorrect password"))
    }
}

const DOWNLOAD_URL_EXPIRY_SECS: u64 = 3600;

#[utoipa::path(
    get,
    path = "/download/url",
    tag = "download",
    params(
        ("code" = String, Query, description = "6-digit share code"),
        ("file_id" = String, Query, description = "File ID to download")
    ),
    responses(
        (status = 200, description = "Download URL generated", body = DownloadUrlResponse),
        (status = 401, description = "Password required or invalid"),
        (status = 404, description = "File not found")
    )
)]
pub async fn get_download_url(
    State(state): State<DownloadState>,
    Query(params): Query<serde_json::Value>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<DownloadUrlResponse>, AppError> {
    let code = params
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("code parameter is required"))?;

    let file_id = params
        .get("file_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_request("file_id parameter is required"))?;

    let file_shares = repository::find_file_shares_by_code(&state.db, code)
        .await
        .map_err(|_| internal_error("Failed to fetch file"))?;

    if file_shares.is_empty() {
        return Err(not_found("File not found or expired"));
    }

    let file_share = file_shares
        .iter()
        .find(|f| f.id == file_id)
        .ok_or_else(|| not_found("File not found"))?;

    if file_share.transfer_type == "p2p" {
        return Err(forbidden(
            "This file is configured for P2P transfer. Use WebRTC connection.",
        ));
    }

    if let Some(password_hash) = &file_share.password_hash {
        let password = headers
            .get("X-File-Password")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| unauthorized("Password required. Set the X-File-Password header."))?;

        let is_valid = bcrypt::verify(password, password_hash)
            .map_err(|_| internal_error("Password verification failed"))?;

        if !is_valid {
            return Err(unauthorized("Incorrect password"));
        }
    }

    let preview = params
        .get("preview")
        .and_then(|v| v.as_str())
        .unwrap_or("false")
        == "true";

    if !preview {
        let user_claims = request.extensions().get::<Claims>().cloned();

        let ip_address = headers
            .get("X-Forwarded-For")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim())
            .or_else(|| {
                headers
                    .get("X-Real-IP")
                    .and_then(|v| v.to_str().ok())
            })
            .unwrap_or("unknown")
            .to_string();

        let user_agent = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let device_platform = user_agent.as_ref().map(|ua| parse_device_platform(ua));

        let ip_address_for_alert = ip_address.clone();

        let _ = repository::create_download_log(
            &state.db,
            CreateDownloadLogDto {
                file_share_id: file_share.id.clone(),
                downloader_user_id: user_claims.as_ref().map(|c| c.sub.clone()),
                ip_address,
                user_agent,
            },
            device_platform,
        )
        .await;

        state
            .notifications
            .notify_download(
                code,
                vec![FileNotificationInfo {
                    file_name: file_share.file_name.clone(),
                    file_size: file_share.file_size,
                    file_type: file_share.file_type.clone(),
                }],
                user_claims.as_ref().map(|c| c.sub.as_str()),
                file_share.user_id.as_deref(),
                &ip_address_for_alert,
            )
            .await;
    }

    let inline = params
        .get("inline")
        .and_then(|v| v.as_str())
        .unwrap_or("false")
        == "true";

    let file_name_param = if inline { None } else { Some(file_share.file_name.as_str()) };

    let download_url = state
        .storage
        .generate_presigned_get_url(
            &file_share.storage_key,
            DOWNLOAD_URL_EXPIRY_SECS,
            file_name_param,
        )
        .await
        .map_err(|e| internal_error(format!("Failed to create download URL: {}", e)))?;

    if file_share.is_one_time {
        let storage_key = file_share.storage_key.clone();
        let file_id = file_share.id.clone();
        let storage = state.storage.clone();
        let db = state.db.clone();

        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            let _ = storage.delete_file(&storage_key).await;
            let _ = repository::delete_file_share(&db, &file_id).await;
        });
    }

    Ok(Json(DownloadUrlResponse {
        download_url,
        expires_in_secs: DOWNLOAD_URL_EXPIRY_SECS,
    }))
}
