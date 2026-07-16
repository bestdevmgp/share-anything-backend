use serde::Deserialize;
use utoipa::ToSchema;

use crate::models::{ExpirationPeriod, TransferType};

#[derive(Debug, Deserialize)]
pub struct UploadMetadata {
    pub description: Option<String>,
    pub password: Option<String>,
    pub expiration: Option<ExpirationPeriod>,
    pub is_one_time: Option<bool>,
    pub transfer_type: Option<TransferType>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct P2PFileInfo {
    pub name: String,
    pub size: i64,
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub relative_path: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateP2PSessionRequest {
    pub files: Vec<P2PFileInfo>,
    pub password: Option<String>,
    #[serde(default)]
    pub empty_folders: Vec<String>,
    /// Uploader's UI language, used to localize the Open Graph link preview.
    #[serde(default)]
    pub locale: Option<String>,
}
