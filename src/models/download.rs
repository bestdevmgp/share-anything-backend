use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    pub code: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct VerifyPasswordRequest {
    pub code: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DownloadUrlResponse {
    pub download_url: String,
    pub expires_in_secs: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FileInfoResponse {
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub transfer_type: String,
    pub description: Option<String>,
    pub has_password: bool,
    pub is_one_time: bool,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub expires_at: DateTime<Utc>,
    pub uploader_online: Option<bool>,
    pub uploader_name: Option<String>,
}
