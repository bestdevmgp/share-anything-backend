use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::api_key_auth::ApiKeyUser,
    models::{
        bad_request, internal_error, not_found, unauthorized, ErrorResponse,
        ExpirationPeriod, CreateDownloadLogDto,
    },
    services::{StorageService, email::EmailService},
    utils::{encode_content_disposition, generate_share_code, generate_storage_key, parse_device_platform},
};

#[derive(Clone)]
pub struct CliState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
    pub email: Arc<EmailService>,
}

// --- Response types ---

#[derive(Debug, Serialize)]
pub struct CliUploadResponse {
    pub share_code: String,
    pub files: Vec<String>,
    pub curl_command: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct CliFileInfoResponse {
    pub share_code: String,
    pub files: Vec<CliFileDetail>,
    pub has_password: bool,
    pub is_one_time: bool,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct CliFileDetail {
    pub file_name: String,
    pub file_size: i64,
}

#[derive(Debug, Serialize)]
pub struct CliFileListResponse {
    pub share_code: String,
    pub files: Vec<CliFileDetail>,
    pub total_count: usize,
    pub has_password: bool,
    pub is_one_time: bool,
    pub expires_at: String,
}

// --- Multipart init types ---

#[derive(Debug, Deserialize)]
pub struct CliMultipartInitRequest {
    pub files: Vec<CliMultipartFileInfo>,
    pub description: Option<String>,
    pub password: Option<String>,
    pub expiration: Option<String>,
    pub is_one_time: Option<bool>,
    pub chunk_size: i64,
}

#[derive(Debug, Deserialize)]
pub struct CliMultipartFileInfo {
    pub file_name: String,
    pub file_size: i64,
    pub content_type: String,
}

#[derive(Debug, Serialize)]
pub struct CliMultipartInitResponse {
    pub upload_session_id: String,
    pub share_code: String,
    pub files: Vec<CliMultipartFileInit>,
    pub chunk_size: i64,
}

#[derive(Debug, Serialize)]
pub struct CliMultipartFileInit {
    pub file_name: String,
    pub storage_key: String,
    pub upload_id: String,
    pub total_parts: i32,
}

// --- Presign parts types ---

#[derive(Debug, Deserialize)]
pub struct CliPresignPartsRequest {
    pub upload_session_id: String,
    pub storage_key: String,
    pub upload_id: String,
    pub part_numbers: Vec<i32>,
}

#[derive(Debug, Serialize)]
pub struct CliPresignPartsResponse {
    pub storage_key: String,
    pub urls: Vec<CliPartUrl>,
    pub expires_in_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct CliPartUrl {
    pub part_number: i32,
    pub presigned_url: String,
}

// --- Complete multipart types ---

#[derive(Debug, Deserialize)]
pub struct CliCompleteMultipartRequest {
    pub upload_session_id: String,
    pub share_code: String,
    pub files: Vec<CliCompleteFileInfo>,
}

#[derive(Debug, Deserialize)]
pub struct CliCompleteFileInfo {
    pub file_name: String,
    pub storage_key: String,
    pub upload_id: String,
    pub file_size: i64,
    pub content_type: String,
    pub parts: Vec<CliCompletedPart>,
}

#[derive(Debug, Deserialize)]
pub struct CliCompletedPart {
    pub part_number: i32,
    pub etag: String,
}

// --- Query types ---

#[derive(Debug, Deserialize)]
pub struct CliDownloadQuery {
    pub password: Option<String>,
    pub file_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CliUploadHistoryQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// --- Constants ---

const CLI_GUEST_MAX_FILE_SIZE: i64 = 100 * 1024 * 1024; // 100MB
const CLI_AUTH_MAX_FILE_SIZE: i64 = 3 * 1024 * 1024 * 1024; // 3GB
const PRESIGNED_URL_EXPIRY_SECS: u64 = 3600;

// --- Pretty JSON response ---

pub struct PrettyJson<T>(T);

impl<T: Serialize> IntoResponse for PrettyJson<T> {
    fn into_response(self) -> Response {
        match serde_json::to_string_pretty(&self.0) {
            Ok(body) => Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}

// --- Helper ---

fn format_expires_at(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn parse_cli_expiration(s: &str) -> Option<ExpirationPeriod> {
    match s {
        "30m" | "thirty_minutes" => Some(ExpirationPeriod::ThirtyMinutes),
        "1h" | "one_hour" => Some(ExpirationPeriod::OneHour),
        "3h" | "three_hours" => Some(ExpirationPeriod::ThreeHours),
        "6h" | "six_hours" => Some(ExpirationPeriod::SixHours),
        "12h" | "twelve_hours" => Some(ExpirationPeriod::TwelveHours),
        "24h" | "twenty_four_hours" => Some(ExpirationPeriod::TwentyFourHours),
        _ => None,
    }
}

// --- Handlers ---

pub async fn cli_upload(
    State(state): State<CliState>,
    api_key_user: Option<axum::extract::Extension<ApiKeyUser>>,
    mut multipart: Multipart,
) -> Result<PrettyJson<CliUploadResponse>, (StatusCode, Json<ErrorResponse>)> {
    let api_key_user = api_key_user.map(|ext| ext.0.clone());

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
        .map_err(|_| bad_request("멀티파트 데이터 파싱 실패"))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                let file_name = field
                    .file_name()
                    .ok_or_else(|| bad_request("파일 이름이 없습니다"))?
                    .to_string();
                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|_| bad_request("파일 데이터를 읽을 수 없습니다"))?;

                files.push(FileData {
                    name: file_name,
                    data: data.to_vec(),
                    content_type,
                });
            }
            "description" => {
                let text = field.text().await.map_err(|_| bad_request("description 필드를 읽을 수 없습니다"))?;
                if !text.is_empty() {
                    description = Some(text);
                }
            }
            "password" => {
                let text = field.text().await.map_err(|_| bad_request("password 필드를 읽을 수 없습니다"))?;
                if !text.is_empty() {
                    password = Some(text);
                }
            }
            "expiration" => {
                let text = field.text().await.map_err(|_| bad_request("expiration 필드를 읽을 수 없습니다"))?;
                if !text.is_empty() {
                    expiration_str = Some(text);
                }
            }
            "is_one_time" => {
                let text = field.text().await.map_err(|_| bad_request("is_one_time 필드를 읽을 수 없습니다"))?;
                if !text.is_empty() {
                    is_one_time = text.parse::<bool>().ok();
                }
            }
            _ => {}
        }
    }

    if files.is_empty() {
        return Err(bad_request("파일이 업로드되지 않았습니다"));
    }

    let max_size = if api_key_user.is_some() {
        CLI_AUTH_MAX_FILE_SIZE
    } else {
        CLI_GUEST_MAX_FILE_SIZE
    };

    let total_size: i64 = files.iter().map(|f| f.data.len() as i64).sum();
    if total_size > max_size {
        return Err(bad_request(format!(
            "파일 크기 제한 초과 ({}MB / {}MB)",
            total_size / 1024 / 1024,
            max_size / 1024 / 1024
        )));
    }

    // Expiration
    let expiration = if let Some(exp_str) = expiration_str {
        if api_key_user.is_none() {
            return Err(unauthorized("만료 시간 설정은 API 키가 필요합니다"));
        }
        parse_cli_expiration(&exp_str)
            .ok_or_else(|| bad_request("유효하지 않은 만료 시간입니다. 사용 가능: 30m, 1h, 3h, 6h, 12h, 24h"))?
    } else {
        ExpirationPeriod::ThirtyMinutes
    };

    // Password
    let password_hash = if let Some(pw) = password {
        if api_key_user.is_none() {
            return Err(unauthorized("비밀번호 설정은 API 키가 필요합니다"));
        }
        Some(bcrypt::hash(pw, bcrypt::DEFAULT_COST).map_err(|_| internal_error("비밀번호 해싱 실패"))?)
    } else {
        None
    };

    // One-time
    let is_one_time = is_one_time.unwrap_or(false);
    if is_one_time && api_key_user.is_none() {
        return Err(unauthorized("1회 다운로드 설정은 API 키가 필요합니다"));
    }

    let expires_at = Utc::now() + expiration.to_duration();

    let share_code = loop {
        let code = generate_share_code();
        if !repository::check_code_exists(&state.db, &code)
            .await
            .map_err(|_| internal_error("공유 코드 중복 확인 실패"))?
        {
            break code;
        }
    };

    let share_group_id = Uuid::new_v4().to_string();
    let user_id = api_key_user.as_ref().map(|u| u.user_id.clone());
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
            .map_err(|e| internal_error(format!("스토리지 업로드 실패: {}", e)))?;

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
        )
        .await
        .map_err(|e| internal_error(format!("데이터베이스 저장 실패: {}", e)))?;

        uploaded_files.push(file_share.file_name);
    }

    let download_url = format!("{}/cli/download/{}", state.config.server.base_url, share_code);
    let curl_command = format!("curl -OJ {}", download_url);

    Ok(PrettyJson(CliUploadResponse {
        share_code,
        files: uploaded_files,
        curl_command,
        expires_at: format_expires_at(expires_at),
    }))
}

pub async fn cli_multipart_init(
    State(state): State<CliState>,
    api_key_user: Option<axum::extract::Extension<ApiKeyUser>>,
    Json(request): Json<CliMultipartInitRequest>,
) -> Result<Json<CliMultipartInitResponse>, (StatusCode, Json<ErrorResponse>)> {
    let api_key_user = api_key_user.map(|ext| ext.0.clone());

    if request.files.is_empty() {
        return Err(bad_request("최소 1개 이상의 파일 정보가 필요합니다"));
    }

    let max_size = if api_key_user.is_some() {
        CLI_AUTH_MAX_FILE_SIZE
    } else {
        CLI_GUEST_MAX_FILE_SIZE
    };

    let total_size: i64 = request.files.iter().map(|f| f.file_size).sum();
    if total_size > max_size {
        return Err(bad_request(format!(
            "파일 크기 제한 초과 ({}MB / {}MB)",
            total_size / 1024 / 1024,
            max_size / 1024 / 1024
        )));
    }

    let expiration = if let Some(exp_str) = &request.expiration {
        if api_key_user.is_none() {
            return Err(unauthorized("만료 시간 설정은 API 키가 필요합니다"));
        }
        parse_cli_expiration(exp_str)
            .ok_or_else(|| bad_request("유효하지 않은 만료 시간입니다"))?
    } else {
        ExpirationPeriod::ThirtyMinutes
    };

    if request.password.is_some() && api_key_user.is_none() {
        return Err(unauthorized("비밀번호 설정은 API 키가 필요합니다"));
    }

    let is_one_time = request.is_one_time.unwrap_or(false);
    if is_one_time && api_key_user.is_none() {
        return Err(unauthorized("1회 다운로드 설정은 API 키가 필요합니다"));
    }

    let share_code = loop {
        let code = generate_share_code();
        if !repository::check_code_exists(&state.db, &code)
            .await
            .map_err(|_| internal_error("공유 코드 중복 확인 실패"))?
        {
            break code;
        }
    };

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
                .map_err(|e| internal_error(format!("멀티파트 업로드 생성 실패: {}", e)))?
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
        Some(bcrypt::hash(pw, bcrypt::DEFAULT_COST).map_err(|_| internal_error("비밀번호 해싱 실패"))?)
    } else {
        None
    };

    let expiration_period_str = expiration.to_string();
    let session_expires_at = Utc::now() + chrono::Duration::hours(1);

    repository::create_upload_session(
        &state.db,
        &upload_session_id,
        &share_code,
        api_key_user.as_ref().map(|u| u.user_id.as_str()),
        request.description.as_deref(),
        password_hash.as_deref(),
        is_one_time,
        false,
        &expiration_period_str,
        session_expires_at,
    )
    .await
    .map_err(|e| internal_error(format!("업로드 세션 생성 실패: {}", e)))?;

    Ok(Json(CliMultipartInitResponse {
        upload_session_id,
        share_code,
        files,
        chunk_size,
    }))
}

pub async fn cli_presign_parts(
    State(state): State<CliState>,
    Json(request): Json<CliPresignPartsRequest>,
) -> Result<Json<CliPresignPartsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|_| internal_error("업로드 세션 조회 실패"))?
        .ok_or_else(|| bad_request("유효하지 않은 업로드 세션입니다"))?;

    if session.completed {
        return Err(bad_request("이미 완료된 업로드 세션입니다"));
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
            .map_err(|_| internal_error("파트 Presigned URL 생성 실패"))?;

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
    Json(request): Json<CliCompleteMultipartRequest>,
) -> Result<PrettyJson<CliUploadResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|_| internal_error("업로드 세션 조회 실패"))?
        .ok_or_else(|| bad_request("유효하지 않은 업로드 세션입니다"))?;

    if session.share_code != request.share_code {
        return Err(bad_request("공유 코드가 일치하지 않습니다"));
    }

    if session.completed {
        return Err(bad_request("이미 완료된 업로드 세션입니다"));
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
                .map_err(|e| internal_error(format!("멀티파트 업로드 완료 실패: {}", e)))?;
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
        )
        .await
        .map_err(|e| internal_error(format!("데이터베이스 저장 실패: {}", e)))?;

        uploaded_files.push(file_share.file_name);
    }

    repository::complete_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|_| internal_error("업로드 세션 완료 처리 실패"))?;

    let download_url = format!(
        "{}/cli/download/{}",
        state.config.server.base_url, session.share_code
    );
    let curl_command = format!("curl -OJ {}", download_url);

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
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?;

    if file_shares.is_empty() {
        return Err(not_found("찾을 수 없거나 만료된 파일입니다"));
    }

    let file_share = if let Some(file_id) = &query.file_id {
        file_shares
            .iter()
            .find(|f| f.id == *file_id)
            .ok_or_else(|| not_found("해당 파일을 찾을 수 없습니다"))?
    } else {
        &file_shares[0]
    };

    if file_share.transfer_type == "p2p" {
        return Err(bad_request("P2P 파일은 CLI 다운로드를 지원하지 않습니다"));
    }

    // Password check
    if let Some(password_hash) = &file_share.password_hash {
        let password = query
            .password
            .as_deref()
            .or_else(|| {
                headers
                    .get("X-File-Password")
                    .and_then(|v| v.to_str().ok())
            })
            .ok_or_else(|| unauthorized("비밀번호가 필요합니다. ?password=<pw> 또는 X-File-Password 헤더를 사용하세요"))?;

        let is_valid = bcrypt::verify(password, password_hash)
            .map_err(|_| internal_error("비밀번호 검증 실패"))?;

        if !is_valid {
            return Err(unauthorized("비밀번호가 일치하지 않습니다"));
        }
    }

    // Download log
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
        .map_err(|e| internal_error(format!("파일 다운로드 실패: {}", e)))?;

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
        encode_content_disposition("attachment", &file_share.file_name)
            .parse()
            .unwrap_or_else(|_| "attachment".parse().unwrap()),
    );

    Ok(response)
}

pub async fn cli_download_info(
    State(state): State<CliState>,
    Path(code): Path<String>,
) -> Result<PrettyJson<CliFileInfoResponse>, (StatusCode, Json<ErrorResponse>)> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?;

    if file_shares.is_empty() {
        return Err(not_found("찾을 수 없거나 만료된 파일입니다"));
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
    }))
}

pub async fn cli_file_list(
    State(state): State<CliState>,
    Path(code): Path<String>,
) -> Result<PrettyJson<CliFileListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?;

    if file_shares.is_empty() {
        return Err(not_found("찾을 수 없거나 만료된 파일입니다"));
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
    api_key_user: axum::extract::Extension<ApiKeyUser>,
    Query(query): Query<CliUploadHistoryQuery>,
) -> Result<PrettyJson<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0);

    let file_shares = repository::find_file_shares_by_user(&state.db, &api_key_user.user_id, limit, offset)
        .await
        .map_err(|e| internal_error(format!("업로드 이력 조회 실패: {}", e)))?;

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
