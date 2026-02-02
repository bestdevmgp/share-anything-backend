use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct FileShare {
    pub id: String,
    pub share_group_id: Option<String>,
    pub user_id: Option<String>,
    pub share_code: String,
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub transfer_type: String,
    pub p2p_status: Option<String>,
    pub uploader_peer_id: Option<String>,
    pub storage_key: String,
    pub description: Option<String>,
    pub password_hash: Option<String>,
    pub is_one_time: bool,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFileShareDto {
    pub user_id: Option<String>,
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub description: Option<String>,
    pub password: Option<String>,
    pub expiration: ExpirationPeriod,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpirationPeriod {
    FiveMinutes,      // 5분
    ThirtyMinutes,    // 30분
    OneHour,          // 1시간
    ThreeHours,       // 3시간
    SixHours,         // 6시간
    TwelveHours,      // 12시간
    TwentyFourHours,  // 24시간
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TransferType {
    Server,
    P2p,
}

impl ExpirationPeriod {
    pub fn to_duration(&self) -> chrono::Duration {
        match self {
            ExpirationPeriod::FiveMinutes => chrono::Duration::minutes(5),
            ExpirationPeriod::ThirtyMinutes => chrono::Duration::minutes(30),
            ExpirationPeriod::OneHour => chrono::Duration::hours(1),
            ExpirationPeriod::ThreeHours => chrono::Duration::hours(3),
            ExpirationPeriod::SixHours => chrono::Duration::hours(6),
            ExpirationPeriod::TwelveHours => chrono::Duration::hours(12),
            ExpirationPeriod::TwentyFourHours => chrono::Duration::hours(24),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FileShareResponse {
    pub id: String,
    pub share_code: String,
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub transfer_type: String,
    pub description: Option<String>,
    pub has_password: bool,
    pub is_one_time: bool,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub expires_at: DateTime<Utc>,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub created_at: DateTime<Utc>,
    pub download_url: String,
    pub qr_code: Option<String>, // Base64 encoded QR code image
    pub uploader_online: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FileShareWithStats {
    #[serde(flatten)]
    pub file_share: FileShareResponse,
    pub download_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MultipleFileUploadResponse {
    pub share_code: String,
    pub files: Vec<FileShareResponse>,
    pub total_count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FileListResponse {
    pub share_code: String,
    pub files: Vec<FileInfoInGroup>,
    pub total_count: usize,
    pub description: Option<String>,
    pub has_password: bool,
    pub is_one_time: bool,
    pub transfer_type: String,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub expires_at: DateTime<Utc>,
    pub uploader_name: Option<String>,
    pub uploader_online: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FileInfoInGroup {
    pub id: String,
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DownloadFilesRequest {
    pub code: String,
    pub file_ids: Vec<String>,
    pub password: Option<String>,
}

// Presigned Upload Types
#[derive(Debug, Deserialize, ToSchema)]
pub struct PresignedUploadFileInfo {
    pub file_name: String,
    pub file_size: i64,
    pub content_type: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PresignedUploadRequest {
    pub files: Vec<PresignedUploadFileInfo>,
    pub description: Option<String>,
    pub password: Option<String>,
    pub expiration: Option<ExpirationPeriod>,
    pub is_one_time: Option<bool>,
    pub turnstile_token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PresignedUploadUrl {
    pub file_name: String,
    pub storage_key: String,
    pub presigned_url: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PresignedUploadResponse {
    pub upload_session_id: String,
    pub share_code: String,
    pub urls: Vec<PresignedUploadUrl>,
    pub expires_in_secs: u64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteUploadFile {
    pub file_name: String,
    pub storage_key: String,
    pub file_size: i64,
    pub content_type: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteUploadRequest {
    pub upload_session_id: String,
    pub share_code: String,
    pub files: Vec<CompleteUploadFile>,
}

// Multipart Upload Types
#[derive(Debug, Deserialize, ToSchema)]
pub struct MultipartUploadFileInfo {
    pub file_name: String,
    pub file_size: i64,
    pub content_type: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct InitMultipartUploadRequest {
    pub files: Vec<MultipartUploadFileInfo>,
    pub description: Option<String>,
    pub password: Option<String>,
    pub expiration: Option<ExpirationPeriod>,
    pub is_one_time: Option<bool>,
    pub turnstile_token: String,
    pub chunk_size: i64, // Size of each chunk in bytes
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MultipartUploadFileInit {
    pub file_name: String,
    pub storage_key: String,
    pub upload_id: String,
    pub total_parts: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct InitMultipartUploadResponse {
    pub upload_session_id: String,
    pub share_code: String,
    pub files: Vec<MultipartUploadFileInit>,
    pub chunk_size: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GetPartUrlsRequest {
    pub upload_session_id: String,
    pub storage_key: String,
    pub upload_id: String,
    pub part_numbers: Vec<i32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PartPresignedUrl {
    pub part_number: i32,
    pub presigned_url: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GetPartUrlsResponse {
    pub storage_key: String,
    pub urls: Vec<PartPresignedUrl>,
    pub expires_in_secs: u64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompletedPart {
    pub part_number: i32,
    pub etag: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteMultipartFileInfo {
    pub file_name: String,
    pub storage_key: String,
    pub upload_id: String,
    pub file_size: i64,
    pub content_type: String,
    pub parts: Vec<CompletedPart>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteMultipartUploadRequest {
    pub upload_session_id: String,
    pub share_code: String,
    pub files: Vec<CompleteMultipartFileInfo>,
}
