use axum::{
    extract::{Extension, Multipart, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{bad_request, unauthorized, internal_error, ErrorResponse, ExpirationPeriod, FileShareResponse, MultipleFileUploadResponse},
    services::{generate_qr_code, StorageService},
    utils::generate_share_code,
};

#[derive(Clone)]
pub struct UploadState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

#[derive(Debug, Deserialize)]
pub struct UploadMetadata {
    pub description: Option<String>,
    pub password: Option<String>,
    pub expiration: Option<ExpirationPeriod>,
}

#[utoipa::path(
    post,
    path = "/file/upload",
    tag = "upload",
    responses(
        (status = 200, description = "Files uploaded successfully", body = MultipleFileUploadResponse),
        (status = 400, description = "Bad request - missing or invalid file"),
        (status = 401, description = "Unauthorized - custom expiration/password requires authentication")
    ),
    security(
        ("bearer_auth" = [])
    )
)]
pub async fn upload_file(
    State(state): State<UploadState>,
    user_claims: Option<Extension<Claims>>,
    mut multipart: Multipart,
) -> Result<Json<MultipleFileUploadResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Extract optional user from extensions (set by optional_auth middleware)
    let user_claims = user_claims.map(|ext| ext.0.clone());

    struct FileData {
        name: String,
        data: Vec<u8>,
        content_type: String,
    }

    let mut files: Vec<FileData> = Vec::new();
    let mut description: Option<String> = None;
    let mut password: Option<String> = None;
    let mut expiration: Option<ExpirationPeriod> = None;

    // Parse multipart form data
    while let Some(field) = multipart.next_field().await.map_err(|_| bad_request("멀티파트 데이터 파싱 실패"))? {
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
                let data = field.bytes().await.map_err(|_| bad_request("파일 데이터를 읽을 수 없습니다"))?;

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
                    expiration = serde_json::from_value(serde_json::json!(text)).ok();
                }
            }
            _ => {}
        }
    }

    // Check if at least one file was uploaded
    if files.is_empty() {
        return Err(bad_request("파일이 업로드되지 않았습니다. 최소 1개 이상의 파일이 필요합니다"));
    }

    // Check file size limits based on user authentication
    let max_total_size: i64 = if user_claims.is_some() {
        3 * 1024 * 1024 * 1024  // 3GB for logged-in users
    } else {
        500 * 1024 * 1024  // 500MB for anonymous users
    };

    let total_size: i64 = files.iter().map(|f| f.data.len() as i64).sum();

    if total_size > max_total_size {
        let limit_mb = max_total_size / 1024 / 1024;
        let current_mb = total_size / 1024 / 1024;
        return Err(bad_request(format!(
            "파일 크기 제한을 초과하였습니다. (업로드: {}MB, 제한: {}MB). {}",
            current_mb,
            limit_mb,
            if user_claims.is_none() {
                "파일 크기 제한을 초과하였습니다. 로그인하여 더 큰 파일을 업로드할 수 있습니다."
            } else {
                ""
            }
        )));
    }

    let metadata = UploadMetadata {
        description,
        password,
        expiration,
    };

    // Determine expiration
    let expiration = if let Some(exp) = metadata.expiration {
        // User can only set custom expiration if logged in
        if user_claims.is_none() {
            return Err(unauthorized("만료 기간 설정은 로그인이 필요합니다"));
        }
        exp
    } else {
        ExpirationPeriod::OneDay // Default
    };

    let expires_at = Utc::now() + expiration.to_duration();

    // Hash password if provided (only allowed for logged-in users)
    let password_hash = if let Some(password) = metadata.password {
        if user_claims.is_none() {
            return Err(unauthorized("비밀번호 설정은 로그인이 필요합니다"));
        }
        Some(
            bcrypt::hash(password, bcrypt::DEFAULT_COST)
                .map_err(|_| internal_error("비밀번호 해싱 실패"))?,
        )
    } else {
        None
    };

    // Generate one share_code and share_group_id for all files
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
    let mut uploaded_files: Vec<FileShareResponse> = Vec::new();

    // Process each file
    for file_data in files {
        let file_size = file_data.data.len() as i64;

        // Generate storage key (prefix from config)
        let storage_key = if state.config.s3.prefix.is_empty() {
            format!("{}/{}", Uuid::new_v4(), file_data.name)
        } else {
            format!(
                "{}{}/{}",
                state.config.s3.prefix,
                Uuid::new_v4(),
                file_data.name
            )
        };

        // Upload to storage
        state
            .storage
            .upload_file(&storage_key, file_data.data, &file_data.content_type)
            .await
            .map_err(|e| internal_error(format!("스토리지 업로드 실패: {}", e)))?;

        // Save to database with shared share_code and share_group_id
        let file_share = repository::create_file_share(
            &state.db,
            Some(share_group_id.clone()),
            user_claims.as_ref().map(|c| c.sub.clone()),
            share_code.clone(),
            file_data.name.clone(),
            file_size,
            file_data.content_type.clone(),
            storage_key,
            metadata.description.clone(),
            password_hash.clone(),
            expires_at,
        )
        .await
        .map_err(|e| internal_error(format!("데이터베이스 저장 실패: {}", e)))?;

        uploaded_files.push(FileShareResponse {
            id: file_share.id,
            share_code: file_share.share_code.clone(),
            file_name: file_share.file_name,
            file_size: file_share.file_size,
            file_type: file_share.file_type,
            description: file_share.description,
            has_password: file_share.password_hash.is_some(),
            expires_at: file_share.expires_at,
            created_at: file_share.created_at,
            download_url: String::new(), // Will be set below
            qr_code: None, // Will be set below
        });
    }

    // Generate download URL and QR code once for the entire group
    let download_url = format!(
        "{}/download?code={}",
        state.config.server.base_url, share_code
    );

    let qr_code = generate_qr_code(&download_url).ok();

    // Update all files with the same download_url and qr_code
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
