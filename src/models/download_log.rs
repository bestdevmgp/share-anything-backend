use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct DownloadLog {
    pub id: String,
    pub file_share_id: String,
    pub downloader_user_id: Option<String>,
    pub ip_address: String,
    pub user_agent: Option<String>,
    pub device_platform: Option<String>,
    pub downloaded_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateDownloadLogDto {
    pub file_share_id: String,
    pub downloader_user_id: Option<String>,
    pub ip_address: String,
    pub user_agent: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DownloadLogResponse {
    pub id: String,
    pub downloader_name: Option<String>, // If logged in user
    pub ip_address: String,
    pub device_platform: String,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub downloaded_at: DateTime<Utc>,
}
