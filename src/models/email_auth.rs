use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct EmailAuthSession {
    pub id: String,
    pub email: String,
    pub token: String,
    pub verification_code: String,
    pub status: String,
    pub request_ip: String,
    pub device_id: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl sqlx::FromRow<'_, sqlx::mysql::MySqlRow> for EmailAuthSession {
    fn from_row(row: &sqlx::mysql::MySqlRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            id: row.try_get("id")?,
            email: row.try_get("email")?,
            token: row.try_get("token")?,
            verification_code: row.try_get("verification_code")?,
            status: row.try_get("status")?,
            request_ip: row.try_get("request_ip")?,
            device_id: row.try_get::<String, _>("device_id").unwrap_or_default(),
            expires_at: row.try_get("expires_at")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EmailSendRequest {
    pub email: String,
    pub device_id: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EmailVerifyRequest {
    pub token: String,
    #[serde(default)]
    pub device_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EmailVerifyCodeRequest {
    pub session_id: String,
    pub code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EmailSendResponse {
    pub session_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EmailStatusResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<EmailAuthData>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EmailVerifyResponse {
    pub same_device: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<EmailAuthData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_code: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EmailVerifyCodeResponse {
    pub token: String,
    pub user: EmailAuthUser,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_provider: Option<String>,
}

#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct EmailAuthData {
    pub token: String,
    pub user: EmailAuthUser,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_provider: Option<String>,
}

#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct EmailAuthUser {
    pub id: String,
    pub email: String,
    pub name: String,
    pub profile_image: Option<String>,
}
