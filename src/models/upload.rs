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
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateP2PSessionRequest {
    pub files: Vec<P2PFileInfo>,
    pub turnstile_token: String,
    pub password: Option<String>,
}
