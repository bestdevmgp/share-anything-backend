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
        unauthorized, forbidden, not_found, internal_error, ErrorResponse,
        QuickAccessUploadRequest, QuickAccessFileResponse, QuickAccessListResponse,
        InitMultipartUploadResponse, MultipartUploadFileInit,
    },
    services::StorageService,
    utils::{generate_share_code, generate_storage_key},
};

#[derive(Clone)]
pub struct QuickAccessState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

pub async fn init_quick_access_upload(
    State(state): State<QuickAccessState>,
    request: Request,
) -> Result<Json<InitMultipartUploadResponse>, (StatusCode, Json<ErrorResponse>)> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("인증이 필요합니다."))?
        .clone();

    let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| internal_error("요청 본문을 읽을 수 없습니다."))?;

    let req: QuickAccessUploadRequest = serde_json::from_slice(&body_bytes)
        .map_err(|_| internal_error("잘못된 요청 형식입니다."))?;

    if req.files.is_empty() {
        return Err(internal_error("최소 1개 이상의 파일 정보가 필요합니다."));
    }

    let max_total_size: i64 = 3 * 1024 * 1024 * 1024;
    let total_size: i64 = req.files.iter().map(|f| f.file_size).sum();

    if total_size > max_total_size {
        return Err(internal_error("파일 크기 제한을 초과하였습니다."));
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
        internal_error("업로드 세션 생성 실패")
    })?;

    Ok(Json(InitMultipartUploadResponse {
        upload_session_id,
        share_code,
        files,
        chunk_size,
    }))
}

pub async fn list_quick_access_files(
    State(state): State<QuickAccessState>,
    request: Request,
) -> Result<Json<QuickAccessListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("인증이 필요합니다"))?;

    let file_shares = repository::find_quick_access_files_by_user(&state.db, &user_claims.sub)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to fetch quick access files");
            internal_error("Quick Access 파일 목록 조회 실패")
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

/// Delete a Quick Access file
pub async fn delete_quick_access_file(
    State(state): State<QuickAccessState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("인증이 필요합니다"))?;

    let file_share = repository::find_file_share_by_id(&state.db, &file_id)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?
        .ok_or_else(|| not_found("파일을 찾을 수 없습니다."))?;

    if file_share.user_id.as_ref() != Some(&user_claims.sub) {
        return Err(forbidden("다른 사용자의 파일에 접근할 수 없습니다"));
    }

    if !file_share.is_quick_access {
        return Err(forbidden("Quick Access 파일이 아닙니다."));
    }

    if !file_share.storage_key.is_empty() {
        state
            .storage
            .delete_file(&file_share.storage_key)
            .await
            .map_err(|e| internal_error(format!("스토리지에서 파일 삭제 실패: {}", e)))?;
    }

    repository::delete_file_share(&state.db, &file_id)
        .await
        .map_err(|e| internal_error(format!("데이터베이스에서 파일 삭제 실패: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn preview_quick_access_file(
    State(state): State<QuickAccessState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("인증이 필요합니다."))?;

    let file_share = repository::find_file_share_by_id(&state.db, &file_id)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?
        .ok_or_else(|| not_found("파일을 찾을 수 없습니다"))?;

    if file_share.user_id.as_ref() != Some(&user_claims.sub) {
        return Err(forbidden("다른 사용자의 파일에 접근할 수 없습니다."));
    }

    if !file_share.is_quick_access {
        return Err(forbidden("Quick Access 파일이 아닙니다."));
    }

    if file_share.expires_at < Utc::now() {
        return Err(not_found("파일이 만료되었습니다."));
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
            internal_error("미리보기 URL 생성 실패")
        })?;

    Ok(Json(serde_json::json!({
        "preview_url": preview_url,
        "file_name": file_share.file_name,
        "expires_in_secs": 3600
    })))
}

pub async fn download_quick_access_file(
    State(state): State<QuickAccessState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("인증이 필요합니다."))?
        .clone();

    let file_share = repository::find_file_share_by_id(&state.db, &file_id)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?
        .ok_or_else(|| not_found("파일을 찾을 수 없습니다."))?;

    if file_share.user_id.as_ref() != Some(&user_claims.sub) {
        return Err(forbidden("다른 사용자의 파일에 접근할 수 없습니다."));
    }

    if !file_share.is_quick_access {
        return Err(forbidden("Quick Access 파일이 아닙니다."));
    }

    if file_share.expires_at < Utc::now() {
        return Err(not_found("파일이 만료되었습니다."));
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
            internal_error("다운로드 URL 생성 실패")
        })?;

    repository::delete_file_share(&state.db, &file_id)
        .await
        .map_err(|e| internal_error(format!("파일 삭제 실패: {}", e)))?;

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
