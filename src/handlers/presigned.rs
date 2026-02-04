use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
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
        bad_request, unauthorized, forbidden, internal_error, ErrorResponse,
        ExpirationPeriod, FileShareResponse, MultipleFileUploadResponse,
        PresignedUploadRequest, PresignedUploadResponse, PresignedUploadUrl,
        CompleteUploadRequest,
        InitMultipartUploadRequest, InitMultipartUploadResponse, MultipartUploadFileInit,
        GetPartUrlsRequest, GetPartUrlsResponse, PartPresignedUrl,
        CompleteMultipartUploadRequest,
    },
    services::{generate_qr_code, StorageService},
    utils::{generate_share_code, generate_storage_key, verify_turnstile_token, extract_client_ip},
};

#[derive(Clone)]
pub struct PresignedState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

const PRESIGNED_URL_EXPIRY_SECS: u64 = 3600;

pub async fn request_presigned_upload(
    State(state): State<PresignedState>,
    user_claims: Option<Extension<Claims>>,
    headers: HeaderMap,
    Json(request): Json<PresignedUploadRequest>,
) -> Result<Json<PresignedUploadResponse>, (StatusCode, Json<ErrorResponse>)> {
    let client_ip = extract_client_ip(&headers);

    let user_claims = user_claims.map(|ext| ext.0.clone());

    verify_turnstile_token(&state.config.turnstile.secret_key, &request.turnstile_token, Some(client_ip.clone()))
        .await
        .map_err(|_| forbidden("보안 확인에 실패했습니다. 다시 시도해주세요"))?;

    if request.files.is_empty() {
        return Err(bad_request("최소 1개 이상의 파일 정보가 필요합니다"));
    }

    let max_total_size: i64 = if user_claims.is_some() {
        3 * 1024 * 1024 * 1024
    } else {
        500 * 1024 * 1024
    };

    let total_size: i64 = request.files.iter().map(|f| f.file_size).sum();

    if total_size > max_total_size {
        let limit_mb = max_total_size / 1024 / 1024;
        let current_mb = total_size / 1024 / 1024;
        return Err(bad_request(format!(
            "파일 크기 제한을 초과하였습니다. (업로드: {}MB, 제한: {}MB)",
            current_mb, limit_mb
        )));
    }

    let expiration = if let Some(exp) = request.expiration {
        if user_claims.is_none() && !matches!(exp, ExpirationPeriod::FiveMinutes) {
            return Err(unauthorized("로그인하지 않은 사용자는 5분 유효기간만 사용할 수 있습니다."));
        }
        exp
    } else {
        ExpirationPeriod::FiveMinutes
    };

    let is_one_time = request.is_one_time.unwrap_or(false);

    if is_one_time && user_claims.is_none() {
        return Err(unauthorized("일회용 다운로드 설정은 로그인이 필요합니다"));
    }

    if request.password.is_some() && user_claims.is_none() {
        return Err(unauthorized("비밀번호 설정은 로그인이 필요합니다"));
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
                internal_error("Presigned URL 생성 실패")
            })?;

        urls.push(PresignedUploadUrl {
            file_name: file_info.file_name.clone(),
            storage_key,
            presigned_url,
        });
    }

    let password_hash = if let Some(password) = &request.password {
        Some(
            bcrypt::hash(password, bcrypt::DEFAULT_COST)
                .map_err(|_| internal_error("비밀번호 해싱 실패"))?,
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
        &expiration_period_str,
        session_expires_at,
    )
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create upload session");
        internal_error("업로드 세션 생성 실패")
    })?;

    Ok(Json(PresignedUploadResponse {
        upload_session_id,
        share_code,
        urls,
        expires_in_secs: PRESIGNED_URL_EXPIRY_SECS,
    }))
}

pub async fn complete_presigned_upload(
    State(state): State<PresignedState>,
    Json(request): Json<CompleteUploadRequest>,
) -> Result<Json<MultipleFileUploadResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to get upload session");
            internal_error("업로드 세션 조회 실패")
        })?
        .ok_or_else(|| bad_request("유효하지 않은 업로드 세션입니다"))?;

    if session.share_code != request.share_code {
        return Err(bad_request("공유 코드가 일치하지 않습니다"));
    }

    if session.completed {
        return Err(bad_request("이미 완료된 업로드 세션입니다"));
    }

    let expiration_period = ExpirationPeriod::from_str(&session.expiration_period)
        .unwrap_or(ExpirationPeriod::FiveMinutes);
    let expires_at = Utc::now() + expiration_period.to_duration();

    let share_group_id = Uuid::new_v4().to_string();
    let mut uploaded_files: Vec<FileShareResponse> = Vec::new();

    for file_info in &request.files {
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
            expires_at,
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create file share record");
            internal_error(format!("데이터베이스 저장 실패: {}", e))
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
            internal_error("업로드 세션 완료 처리 실패")
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

    Ok(Json(MultipleFileUploadResponse {
        share_code: session.share_code,
        total_count: uploaded_files.len(),
        files: uploaded_files,
    }))
}

pub async fn init_multipart_upload(
    State(state): State<PresignedState>,
    user_claims: Option<Extension<Claims>>,
    headers: HeaderMap,
    Json(request): Json<InitMultipartUploadRequest>,
) -> Result<Json<InitMultipartUploadResponse>, (StatusCode, Json<ErrorResponse>)> {
    let client_ip = extract_client_ip(&headers);

    let user_claims = user_claims.map(|ext| ext.0.clone());

    verify_turnstile_token(&state.config.turnstile.secret_key, &request.turnstile_token, Some(client_ip.clone()))
        .await
        .map_err(|_| forbidden("보안 확인에 실패했습니다. 다시 시도해주세요"))?;

    if request.files.is_empty() {
        return Err(bad_request("최소 1개 이상의 파일 정보가 필요합니다"));
    }

    let max_total_size: i64 = if user_claims.is_some() {
        3 * 1024 * 1024 * 1024
    } else {
        500 * 1024 * 1024
    };

    let total_size: i64 = request.files.iter().map(|f| f.file_size).sum();

    if total_size > max_total_size {
        let limit_mb = max_total_size / 1024 / 1024;
        let current_mb = total_size / 1024 / 1024;
        return Err(bad_request(format!(
            "파일 크기 제한을 초과하였습니다. (업로드: {}MB, 제한: {}MB)",
            current_mb, limit_mb
        )));
    }

    let expiration = if let Some(exp) = request.expiration {
        if user_claims.is_none() && !matches!(exp, ExpirationPeriod::FiveMinutes) {
            return Err(unauthorized("로그인하지 않은 사용자는 5분 유효기간만 사용할 수 있습니다."));
        }
        exp
    } else {
        ExpirationPeriod::FiveMinutes
    };

    let is_one_time = request.is_one_time.unwrap_or(false);

    if is_one_time && user_claims.is_none() {
        return Err(unauthorized("일회용 다운로드 설정은 로그인이 필요합니다"));
    }

    if request.password.is_some() && user_claims.is_none() {
        return Err(unauthorized("비밀번호 설정은 로그인이 필요합니다"));
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
                    internal_error("R2 멀티파트 업로드 생성 실패")
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

    let password_hash = if let Some(password) = &request.password {
        Some(
            bcrypt::hash(password, bcrypt::DEFAULT_COST)
                .map_err(|_| internal_error("비밀번호 해싱 실패"))?,
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
        &expiration_period_str,
        session_expires_at,
    )
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create upload session");
        internal_error("업로드 세션 생성 실패")
    })?;

    Ok(Json(InitMultipartUploadResponse {
        upload_session_id,
        share_code,
        files,
        chunk_size,
    }))
}

pub async fn get_part_presigned_urls(
    State(state): State<PresignedState>,
    Json(request): Json<GetPartUrlsRequest>,
) -> Result<Json<GetPartUrlsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to get upload session");
            internal_error("업로드 세션 조회 실패")
        })?
        .ok_or_else(|| bad_request("유효하지 않은 업로드 세션입니다"))?;

    if session.completed {
        return Err(bad_request("이미 완료된 업로드 세션입니다"));
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
                internal_error("파트 Presigned URL 생성 실패")
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

pub async fn complete_multipart_upload(
    State(state): State<PresignedState>,
    Json(request): Json<CompleteMultipartUploadRequest>,
) -> Result<Json<MultipleFileUploadResponse>, (StatusCode, Json<ErrorResponse>)> {
    let session = repository::get_upload_session(&state.db, &request.upload_session_id)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to get upload session");
            internal_error("업로드 세션 조회 실패")
        })?
        .ok_or_else(|| bad_request("유효하지 않은 업로드 세션입니다"))?;

    if session.share_code != request.share_code {
        return Err(bad_request("공유 코드가 일치하지 않습니다"));
    }

    if session.completed {
        return Err(bad_request("이미 완료된 업로드 세션입니다"));
    }

    let expiration_period = ExpirationPeriod::from_str(&session.expiration_period)
        .unwrap_or(ExpirationPeriod::FiveMinutes);
    let expires_at = Utc::now() + expiration_period.to_duration();

    let share_group_id = Uuid::new_v4().to_string();
    let mut uploaded_files: Vec<FileShareResponse> = Vec::new();

    for file_info in &request.files {
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
                    internal_error("R2 멀티파트 업로드 완료 실패")
                })?;
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
            expires_at,
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create file share record");
            internal_error(format!("데이터베이스 저장 실패: {}", e))
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
            internal_error("업로드 세션 완료 처리 실패")
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

    Ok(Json(MultipleFileUploadResponse {
        share_code: session.share_code,
        total_count: uploaded_files.len(),
        files: uploaded_files,
    }))
}