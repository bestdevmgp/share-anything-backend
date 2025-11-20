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
    pub storage_key: String,
    pub description: Option<String>,
    pub password_hash: Option<String>,
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
    OneHour,
    OneDay,
    ThreeDays,
    OneWeek,
    OneMonth,
}

impl ExpirationPeriod {
    pub fn to_duration(&self) -> chrono::Duration {
        match self {
            ExpirationPeriod::OneHour => chrono::Duration::hours(1),
            ExpirationPeriod::OneDay => chrono::Duration::days(1),
            ExpirationPeriod::ThreeDays => chrono::Duration::days(3),
            ExpirationPeriod::OneWeek => chrono::Duration::weeks(1),
            ExpirationPeriod::OneMonth => chrono::Duration::days(30),
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
    pub description: Option<String>,
    pub has_password: bool,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub download_url: String,
    pub qr_code: Option<String>, // Base64 encoded QR code image
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
    pub expires_at: DateTime<Utc>,
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
