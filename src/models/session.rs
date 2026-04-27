use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use utoipa::ToSchema;

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct Session {
    pub jti: String,
    pub user_id: String,
    pub device_label: Option<String>,
    pub user_agent: Option<String>,
    pub user_agent_hash: String,
    pub ip_address: String,
    pub location: Option<String>,
    pub last_seen_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct BlockedDevice {
    pub id: String,
    pub user_id: String,
    pub user_agent_hash: String,
    pub user_agent: Option<String>,
    pub ip_address: String,
    pub device_label: Option<String>,
    pub blocked_at: DateTime<Utc>,
}

pub struct CreateSessionDto {
    pub jti: String,
    pub user_id: String,
    pub device_label: Option<String>,
    pub user_agent: String,
    pub user_agent_hash: String,
    pub ip_address: String,
    pub location: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionResponse {
    pub jti: String,
    pub device_label: Option<String>,
    pub ip_address: String,
    pub location: Option<String>,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub last_seen_at: DateTime<Utc>,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub created_at: DateTime<Utc>,
    pub is_current: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BlockedDeviceResponse {
    pub id: String,
    pub device_label: Option<String>,
    pub ip_address: String,
    #[serde(serialize_with = "crate::utils::serialize_as_kst")]
    pub blocked_at: DateTime<Utc>,
}

