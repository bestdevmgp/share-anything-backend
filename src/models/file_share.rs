use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct FileShare {
    pub id: String,
    pub share_group_id: Option<String>,
    pub user_id: Option<String>,
    pub created_via_api_key_id: Option<String>,
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
    pub is_quick_access: bool,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub image_width: Option<i32>,
    pub image_height: Option<i32>,
    pub display_order: i32,
    #[sqlx(default)]
    pub device_id: Option<String>,
    #[sqlx(default)]
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpirationPeriod {
    FiveMinutes,
    ThirtyMinutes,
    OneHour,
    ThreeHours,
    SixHours,
    TwelveHours,
    TwentyFourHours,
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

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "five_minutes" => Some(ExpirationPeriod::FiveMinutes),
            "thirty_minutes" => Some(ExpirationPeriod::ThirtyMinutes),
            "one_hour" => Some(ExpirationPeriod::OneHour),
            "three_hours" => Some(ExpirationPeriod::ThreeHours),
            "six_hours" => Some(ExpirationPeriod::SixHours),
            "twelve_hours" => Some(ExpirationPeriod::TwelveHours),
            "twenty_four_hours" => Some(ExpirationPeriod::TwentyFourHours),
            _ => None,
        }
    }
}

impl std::fmt::Display for ExpirationPeriod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ExpirationPeriod::FiveMinutes => "five_minutes",
            ExpirationPeriod::ThirtyMinutes => "thirty_minutes",
            ExpirationPeriod::OneHour => "one_hour",
            ExpirationPeriod::ThreeHours => "three_hours",
            ExpirationPeriod::SixHours => "six_hours",
            ExpirationPeriod::TwelveHours => "twelve_hours",
            ExpirationPeriod::TwentyFourHours => "twenty_four_hours",
        };
        write!(f, "{}", s)
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
    pub relative_path: Option<String>,
    pub has_password: bool,
    pub is_one_time: bool,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub expires_at: DateTime<Utc>,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub created_at: DateTime<Utc>,
    pub download_url: String,
    pub qr_code: Option<String>,
    pub uploader_online: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FileShareWithStats {
    #[serde(flatten)]
    pub file_share: FileShareResponse,
    pub download_count: i64,
    /// Short-lived presigned INLINE preview URL so the upload-history page can preview
    /// without a per-click /download/url round-trip. Empty (omitted) for password-protected
    /// or p2p shares — the client falls back to the /download/url path.
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub preview_url: String,
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
    pub empty_folders: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FileInfoInGroup {
    pub id: String,
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_height: Option<i32>,
    pub relative_path: String,
    /// Short-lived presigned INLINE URL so the client can preview the file instantly
    /// without a per-click /download/url round-trip. Empty (and omitted) for
    /// password-protected or p2p shares — the client then falls back to the
    /// password-validated /download/url path.
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub preview_url: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DownloadFilesRequest {
    pub code: String,
    pub file_ids: Vec<String>,
    pub password: Option<String>,
}

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
    #[serde(default)]
    pub image_width: Option<i32>,
    #[serde(default)]
    pub image_height: Option<i32>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteUploadRequest {
    pub upload_session_id: String,
    pub share_code: String,
    pub files: Vec<CompleteUploadFile>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MultipartUploadFileInfo {
    pub file_name: String,
    pub file_size: i64,
    pub content_type: String,
    #[serde(default)]
    pub relative_path: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct InitMultipartUploadRequest {
    pub files: Vec<MultipartUploadFileInfo>,
    pub description: Option<String>,
    pub password: Option<String>,
    pub expiration: Option<ExpirationPeriod>,
    pub is_one_time: Option<bool>,
    pub chunk_size: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MultipartUploadFileInit {
    pub file_name: String,
    pub storage_key: String,
    pub upload_id: String,
    pub total_parts: i32,
    /// `<exp>.<hmac>` proving to the Worker this storage key was issued by us.
    /// Empty when signing is disabled.
    pub upload_signature: String,
    pub relative_path: String,
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
    #[serde(default)]
    pub image_width: Option<i32>,
    #[serde(default)]
    pub image_height: Option<i32>,
    #[serde(default)]
    pub relative_path: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteMultipartUploadRequest {
    pub upload_session_id: String,
    pub share_code: String,
    pub files: Vec<CompleteMultipartFileInfo>,
    #[serde(default)]
    pub empty_folders: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct QuickAccessUploadRequest {
    pub files: Vec<MultipartUploadFileInfo>,
    pub chunk_size: i64,
    pub device_info: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QuickAccessFileResponse {
    pub id: String,
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub storage_key: String,
    pub uploaded_from: Option<String>,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub expires_at: DateTime<Utc>,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QuickAccessListResponse {
    pub files: Vec<QuickAccessFileResponse>,
}
