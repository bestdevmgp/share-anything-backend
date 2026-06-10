use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub enum UserStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "deactivated")]
    Deactivated,
    #[serde(rename = "deleted")]
    Deleted,
}

impl std::fmt::Display for UserStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserStatus::Active => write!(f, "active"),
            UserStatus::Deactivated => write!(f, "deactivated"),
            UserStatus::Deleted => write!(f, "deleted"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct User {
    pub id: String,
    #[serde(deserialize_with = "deserialize_oauth_provider")]
    pub oauth_provider: OAuthProvider,
    pub oauth_id: String,
    pub email: String,
    pub name: String,
    pub profile_image: Option<String>,
    pub status: UserStatus,
    pub notify_upload: bool,
    pub notify_download: bool,
    pub notify_download_alert: bool,
    pub notify_security: bool,
    pub notify_language: String,
    pub default_expiration: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FromRow<'_, sqlx::mysql::MySqlRow> for User {
    fn from_row(row: &sqlx::mysql::MySqlRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        let provider_str: String = row.try_get("oauth_provider")?;
        let oauth_provider = match provider_str.as_str() {
            "google" => OAuthProvider::Google,
            "naver" => OAuthProvider::Naver,
            "kakao" => OAuthProvider::Kakao,
            "apple" => OAuthProvider::Apple,
            "email" => OAuthProvider::Email,
            _ => return Err(sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid oauth_provider: {}", provider_str)
            )))),
        };

        let status_str: String = row.try_get("status")?;
        let status = match status_str.as_str() {
            "active" => UserStatus::Active,
            "deactivated" => UserStatus::Deactivated,
            "deleted" => UserStatus::Deleted,
            _ => UserStatus::Active,
        };

        Ok(User {
            id: row.try_get("id")?,
            oauth_provider,
            oauth_id: row.try_get("oauth_id")?,
            email: row.try_get("email")?,
            name: row.try_get("name")?,
            profile_image: row.try_get("profile_image")?,
            status,
            notify_upload: row.try_get("notify_upload")?,
            notify_download: row.try_get("notify_download")?,
            notify_download_alert: row.try_get("notify_download_alert")?,
            notify_security: row.try_get("notify_security")?,
            notify_language: row.try_get("notify_language")?,
            default_expiration: row.try_get("default_expiration")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

fn deserialize_oauth_provider<'de, D>(deserializer: D) -> Result<OAuthProvider, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "google" => Ok(OAuthProvider::Google),
        "naver" => Ok(OAuthProvider::Naver),
        "kakao" => Ok(OAuthProvider::Kakao),
        "apple" => Ok(OAuthProvider::Apple),
        "email" => Ok(OAuthProvider::Email),
        _ => Err(serde::de::Error::custom(format!("Invalid oauth_provider: {}", s))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, ToSchema)]
#[sqlx(type_name = "VARCHAR", rename_all = "lowercase")]
pub enum OAuthProvider {
    Google,
    Naver,
    Kakao,
    Apple,
    Email,
}

impl std::fmt::Display for OAuthProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OAuthProvider::Google => write!(f, "google"),
            OAuthProvider::Naver => write!(f, "naver"),
            OAuthProvider::Kakao => write!(f, "kakao"),
            OAuthProvider::Apple => write!(f, "apple"),
            OAuthProvider::Email => write!(f, "email"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateUserDto {
    pub oauth_provider: OAuthProvider,
    pub oauth_id: String,
    pub email: String,
    pub name: String,
    pub profile_image: Option<String>,
    pub notify_language: String,
}

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadHistoryResponse {
    pub items: Vec<crate::models::FileShareWithStats>,
    pub total: usize,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct NotificationSettingsResponse {
    pub notify_upload: bool,
    pub notify_download: bool,
    pub notify_download_alert: bool,
    pub notify_security: bool,
    pub notify_language: String,
    #[schema(example = "thirty_minutes")]
    pub default_expiration: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateNotificationSettingsRequest {
    pub notify_upload: bool,
    pub notify_download: bool,
    pub notify_download_alert: bool,
    pub notify_security: bool,
    pub notify_language: String,
    #[schema(example = "thirty_minutes")]
    pub default_expiration: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateNameRequest {
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateNameResponse {
    pub name: String,
}
