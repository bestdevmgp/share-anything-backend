use axum::{
    body::Body,
    extract::{Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{bad_request, unauthorized, forbidden, not_found, internal_error, ErrorResponse, CreateDownloadLogDto, FileListResponse, FileInfoInGroup, DownloadFilesRequest},
    services::StorageService,
    utils::{parse_device_platform, verify_turnstile_token, extract_client_ip},
};
use std::io::{Write as _, Cursor};
use zip::write::{FileOptions, ZipWriter};

#[derive(Clone)]
pub struct DownloadState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    code: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct VerifyPasswordRequest {
    code: String,
    password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FileInfoResponse {
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub description: Option<String>,
    pub has_password: bool,
    pub is_one_time: bool,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub uploader_name: Option<String>,
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
) -> Result<Json<FileListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &query.code)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?;

    if file_shares.is_empty() {
        return Err(not_found("찾을 수 없거나 만료된 파일입니다."));
    }

    let first_file = &file_shares[0];

    let uploader_name = if let Some(user_id) = &first_file.user_id {
        repository::find_user_by_id(&state.db, user_id)
            .await
            .ok()
            .flatten()
            .map(|u| u.name)
    } else {
        None
    };

    let files: Vec<FileInfoInGroup> = file_shares
        .iter()
        .map(|f| FileInfoInGroup {
            id: f.id.clone(),
            file_name: f.file_name.clone(),
            file_size: f.file_size,
            file_type: f.file_type.clone(),
        })
        .collect();

    Ok(Json(FileListResponse {
        share_code: query.code,
        files,
        total_count: file_shares.len(),
        description: first_file.description.clone(),
        has_password: first_file.password_hash.is_some(),
        is_one_time: first_file.is_one_time,
        expires_at: first_file.expires_at,
        uploader_name,
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
) -> Result<Json<FileInfoResponse>, (StatusCode, Json<ErrorResponse>)> {
    let file_share = repository::find_file_share_by_code(&state.db, &query.code)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?
        .ok_or_else(|| not_found("찾을 수 없거나 만료된 파일입니다."))?;

    let uploader_name = if let Some(user_id) = &file_share.user_id {
        repository::find_user_by_id(&state.db, user_id)
            .await
            .ok()
            .flatten()
            .map(|u| u.name)
    } else {
        None
    };

    Ok(Json(FileInfoResponse {
        file_name: file_share.file_name,
        file_size: file_share.file_size,
        file_type: file_share.file_type,
        description: file_share.description,
        has_password: file_share.password_hash.is_some(),
        is_one_time: file_share.is_one_time,
        expires_at: file_share.expires_at,
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
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
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
        .map_err(|_| internal_error("파일 조회 실패"))?;

    if file_shares.is_empty() {
        return Err(not_found("찾을 수 없거나 만료된 파일입니다."));
    }

    let file_share = file_shares
        .iter()
        .find(|f| f.id == file_id)
        .ok_or_else(|| not_found("해당 파일을 찾을 수 없습니다"))?;

    let turnstile_token = headers
        .get("X-Turnstile-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| bad_request("보안 확인이 필요합니다"))?;

    let client_ip = extract_client_ip(&headers);

    verify_turnstile_token(&state.config.turnstile.secret_key, turnstile_token, Some(client_ip))
        .await
        .map_err(|_| forbidden("보안 확인에 실패했습니다. 다시 시도해주세요"))?;

    if let Some(password_hash) = &file_share.password_hash {
        let password = headers
            .get("X-File-Password")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| unauthorized("비밀번호가 필요합니다. X-File-Password 헤더를 설정하세요"))?;

        let is_valid = bcrypt::verify(password, password_hash)
            .map_err(|_| internal_error("비밀번호 검증 실패"))?;

        if !is_valid {
            return Err(unauthorized("비밀번호가 일치하지 않습니다"));
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

    let _ = repository::create_download_log(
        &state.db,
        CreateDownloadLogDto {
            file_share_id: file_share.id.clone(),
            downloader_user_id: user_claims.map(|c| c.sub),
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
        .map_err(|e| internal_error(format!("스토리지에서 파일 다운로드 실패: {}", e)))?;

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
            .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
    );

    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", file_share.file_name)
            .parse()
            .unwrap(),
    );

    Ok(response)
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
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
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
        .map_err(|_| internal_error("파일 조회 실패"))?;

    if file_shares.is_empty() {
        return Err(not_found("찾을 수 없거나 만료된 파일입니다."));
    }

    let file_share = file_shares
        .iter()
        .find(|f| f.id == file_id)
        .ok_or_else(|| not_found("해당 파일을 찾을 수 없습니다"))?;

    if let Some(password_hash) = &file_share.password_hash {
        let password = headers
            .get("X-File-Password")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| unauthorized("비밀번호가 필요합니다. X-File-Password 헤더를 설정하세요"))?;

        let is_valid = bcrypt::verify(password, password_hash)
            .map_err(|_| internal_error("비밀번호 검증 실패"))?;

        if !is_valid {
            return Err(unauthorized("비밀번호가 일치하지 않습니다"));
        }
    }

    let file_data = state
        .storage
        .download_file(&file_share.storage_key)
        .await
        .map_err(|e| internal_error(format!("스토리지에서 파일 다운로드 실패: {}", e)))?;

    let mut response = Response::new(Body::from(file_data));

    response.headers_mut().insert(
        header::CONTENT_TYPE,
        file_share
            .file_type
            .parse()
            .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
    );

    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("inline; filename=\"{}\"", file_share.file_name)
            .parse()
            .unwrap(),
    );

    Ok(response)
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
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let file_share = repository::find_file_share_by_code(&state.db, &query.code)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?
        .ok_or_else(|| not_found("찾을 수 없거나 만료된 파일입니다."))?;

    // Verify Turnstile token
    let turnstile_token = headers
        .get("X-Turnstile-Token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| bad_request("보안 확인이 필요합니다"))?;

    let client_ip = extract_client_ip(&headers);

    verify_turnstile_token(&state.config.turnstile.secret_key, turnstile_token, Some(client_ip))
        .await
        .map_err(|_| forbidden("보안 확인에 실패했습니다. 다시 시도해주세요"))?;

    if let Some(password_hash) = &file_share.password_hash {
        let password = headers
            .get("X-File-Password")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| unauthorized("비밀번호가 필요합니다. X-File-Password 헤더를 설정하세요"))?;

        let is_valid = bcrypt::verify(password, password_hash)
            .map_err(|_| internal_error("비밀번호 검증 실패"))?;

        if !is_valid {
            return Err(unauthorized("비밀번호가 일치하지 않습니다"));
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

    let _ = repository::create_download_log(
        &state.db,
        CreateDownloadLogDto {
            file_share_id: file_share.id.clone(),
            downloader_user_id: user_claims.map(|c| c.sub),
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
        .map_err(|e| internal_error(format!("스토리지에서 파일 다운로드 실패: {}", e)))?;

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
            .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
    );

    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", file_share.file_name)
            .parse()
            .unwrap(),
    );

    Ok(response)
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
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let user_claims = request.extensions().get::<Claims>().cloned();

    let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| bad_request("요청 본문을 읽을 수 없습니다"))?;

    let req: DownloadFilesRequest = serde_json::from_slice(&body_bytes)
        .map_err(|_| bad_request("잘못된 요청 형식입니다"))?;

    if req.file_ids.is_empty() {
        return Err(bad_request("최소 1개 이상의 파일 ID가 필요합니다"));
    }

    let turnstile_token = req
        .turnstile_token
        .as_ref()
        .ok_or_else(|| bad_request("보안 확인이 필요합니다"))?;

    let client_ip = extract_client_ip(&headers);

    verify_turnstile_token(&state.config.turnstile.secret_key, turnstile_token, Some(client_ip))
        .await
        .map_err(|_| forbidden("보안 확인에 실패했습니다. 다시 시도해주세요"))?;

    let all_files = repository::find_file_shares_by_code(&state.db, &req.code)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?;

    if all_files.is_empty() {
        return Err(not_found("찾을 수 없거나 만료된 파일입니다."));
    }

    let files_to_download: Vec<_> = all_files
        .iter()
        .filter(|f| req.file_ids.contains(&f.id))
        .collect();

    if files_to_download.is_empty() {
        return Err(not_found("요청한 파일을 찾을 수 없습니다"));
    }

    if let Some(password_hash) = &files_to_download[0].password_hash {
        if let Some(password) = &req.password {
            let is_valid = bcrypt::verify(password, password_hash)
                .map_err(|_| internal_error("비밀번호 검증 실패"))?;

            if !is_valid {
                return Err(unauthorized("비밀번호가 일치하지 않습니다"));
            }
        } else {
            return Err(unauthorized("비밀번호가 필요합니다"));
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

    let buffer = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(buffer);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    for file_share in files_to_download.iter() {
        let file_data = state
            .storage
            .download_file(&file_share.storage_key)
            .await
            .map_err(|e| internal_error(format!("스토리지에서 파일 다운로드 실패: {}", e)))?;

        zip.start_file(&file_share.file_name, options)
            .map_err(|_| internal_error("ZIP 파일 생성 실패"))?;

        zip.write_all(&file_data)
            .map_err(|_| internal_error("ZIP 파일 쓰기 실패"))?;

        let _ = repository::create_download_log(
            &state.db,
            CreateDownloadLogDto {
                file_share_id: file_share.id.clone(),
                downloader_user_id: user_claims.as_ref().map(|c| c.sub.clone()),
                ip_address: ip_address.clone(),
                user_agent: user_agent.clone(),
            },
            device_platform.clone(),
        )
        .await;
    }

    let buffer = zip
        .finish()
        .map_err(|_| internal_error("ZIP 파일 완성 실패"))?;

    let zip_data = buffer.into_inner();

    let is_one_time = files_to_download[0].is_one_time;
    if is_one_time {
        for file_share in files_to_download.iter() {
            let _ = state.storage.delete_file(&file_share.storage_key).await;

            let _ = repository::delete_file_share(&state.db, &file_share.id).await;
        }
    }

    let mut response = Response::new(Body::from(zip_data));

    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/zip".parse().unwrap(),
    );

    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"files_{}.zip\"", req.code)
            .parse()
            .unwrap(),
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
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let file_share = repository::find_file_share_by_code(&state.db, &req.code)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?
        .ok_or_else(|| not_found("찾을 수 없거나 만료된 파일입니다."))?;

    if let Some(password_hash) = &file_share.password_hash {
        let is_valid = bcrypt::verify(&req.password, password_hash)
            .map_err(|_| internal_error("비밀번호 검증 실패"))?;

        if is_valid {
            Ok(StatusCode::OK)
        } else {
            Err(unauthorized("비밀번호가 일치하지 않습니다"))
        }
    } else {
        Ok(StatusCode::OK)
    }
}
