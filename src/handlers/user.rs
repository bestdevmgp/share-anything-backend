use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{unauthorized, forbidden, not_found, internal_error, ErrorResponse, DownloadLogResponse, FileShareResponse, FileShareWithStats},
    services::{generate_qr_code, StorageService},
};

#[derive(Clone)]
pub struct UserState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadHistoryResponse {
    pub items: Vec<FileShareWithStats>,
    pub total: usize,
    pub limit: i64,
    pub offset: i64,
}

/// Get user's upload history (requires authentication)
#[utoipa::path(
    get,
    path = "/user/uploads",
    tag = "user",
    params(
        ("limit" = Option<i64>, Query, description = "Number of items to return (default: 20)"),
        ("offset" = Option<i64>, Query, description = "Number of items to skip (default: 0)")
    ),
    responses(
        (status = 200, description = "Upload history retrieved successfully", body = UploadHistoryResponse),
        (status = 401, description = "Unauthorized - authentication required")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_upload_history(
    State(state): State<UserState>,
    Query(pagination): Query<PaginationQuery>,
    request: Request,
) -> Result<Json<UploadHistoryResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Extract user claims (should be set by require_auth middleware)
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("인증이 필요합니다"))?;

    let file_shares = repository::find_file_shares_by_user(
        &state.db,
        &user_claims.sub,
        pagination.limit,
        pagination.offset,
    )
    .await
    .map_err(|e| internal_error(format!("업로드 이력 조회 실패: {}", e)))?;

    let mut items = Vec::new();

    for file_share in file_shares {
        let download_count = repository::count_downloads_by_file_share(&state.db, &file_share.id)
            .await
            .unwrap_or(0);

        let download_url = format!(
            "{}/download?code={}",
            state.config.server.base_url, file_share.share_code
        );

        let qr_code = generate_qr_code(&download_url).ok();

        items.push(FileShareWithStats {
            file_share: FileShareResponse {
                id: file_share.id.clone(),
                share_code: file_share.share_code.clone(),
                file_name: file_share.file_name.clone(),
                file_size: file_share.file_size,
                file_type: file_share.file_type.clone(),
                description: file_share.description.clone(),
                has_password: file_share.password_hash.is_some(),
                expires_at: file_share.expires_at,
                created_at: file_share.created_at,
                download_url,
                qr_code,
            },
            download_count,
        });
    }

    let total = items.len();

    Ok(Json(UploadHistoryResponse {
        items,
        total,
        limit: pagination.limit,
        offset: pagination.offset,
    }))
}

/// Get download logs for a specific file (requires authentication)
#[utoipa::path(
    get,
    path = "/user/uploads/{file_id}/downloads",
    tag = "user",
    params(
        ("file_id" = String, Path, description = "File share ID")
    ),
    responses(
        (status = 200, description = "Download logs retrieved successfully", body = Vec<DownloadLogResponse>),
        (status = 401, description = "Unauthorized - authentication required"),
        (status = 403, description = "Forbidden - file does not belong to user"),
        (status = 404, description = "File not found")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn get_download_logs(
    State(state): State<UserState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<Json<Vec<DownloadLogResponse>>, (StatusCode, Json<ErrorResponse>)> {
    // Extract user claims
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("인증이 필요합니다"))?;

    // Check if file belongs to user
    let file_share = repository::find_file_share_by_id(&state.db, &file_id)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?
        .ok_or_else(|| not_found("파일을 찾을 수 없습니다"))?;

    if file_share.user_id.as_ref() != Some(&user_claims.sub) {
        return Err(forbidden("다른 사용자의 파일에 접근할 수 없습니다"));
    }

    // Get download logs
    let logs = repository::find_download_logs_by_file_share(&state.db, &file_id)
        .await
        .map_err(|e| internal_error(format!("다운로드 로그 조회 실패: {}", e)))?;

    let mut response = Vec::new();

    for log in logs {
        let downloader_name = if let Some(user_id) = &log.downloader_user_id {
            repository::find_user_by_id(&state.db, user_id)
                .await
                .ok()
                .flatten()
                .map(|u| u.name)
        } else {
            None
        };

        response.push(DownloadLogResponse {
            id: log.id,
            downloader_name,
            ip_address: log.ip_address,
            device_platform: log.device_platform.unwrap_or_else(|| "Unknown".to_string()),
            downloaded_at: log.downloaded_at,
        });
    }

    Ok(Json(response))
}

/// Delete a file share (requires authentication)
#[utoipa::path(
    delete,
    path = "/user/uploads/{file_id}",
    tag = "user",
    params(
        ("file_id" = String, Path, description = "File share ID")
    ),
    responses(
        (status = 204, description = "File deleted successfully"),
        (status = 401, description = "Unauthorized - authentication required"),
        (status = 403, description = "Forbidden - file does not belong to user"),
        (status = 404, description = "File not found")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn delete_file_share(
    State(state): State<UserState>,
    Path(file_id): Path<String>,
    request: Request,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // Extract user claims
    let user_claims = request
        .extensions()
        .get::<Claims>()
        .ok_or_else(|| unauthorized("인증이 필요합니다"))?;

    // Check if file belongs to user
    let file_share = repository::find_file_share_by_id(&state.db, &file_id)
        .await
        .map_err(|_| internal_error("파일 조회 실패"))?
        .ok_or_else(|| not_found("파일을 찾을 수 없습니다"))?;

    if file_share.user_id.as_ref() != Some(&user_claims.sub) {
        return Err(forbidden("다른 사용자의 파일을 삭제할 수 없습니다"));
    }

    // Delete from storage
    state
        .storage
        .delete_file(&file_share.storage_key)
        .await
        .map_err(|e| internal_error(format!("스토리지에서 파일 삭제 실패: {}", e)))?;

    // Delete from database
    repository::delete_file_share(&state.db, &file_id)
        .await
        .map_err(|e| internal_error(format!("데이터베이스에서 파일 삭제 실패: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}
