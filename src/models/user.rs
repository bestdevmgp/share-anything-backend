use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct User {
    pub id: String,
    pub oauth_provider: OAuthProvider,
    pub oauth_id: String,
    pub email: String,
    pub name: String,
    pub profile_image: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, ToSchema)]
#[sqlx(type_name = "ENUM", rename_all = "lowercase")]
pub enum OAuthProvider {
    #[sqlx(rename = "google")]
    Google,
    #[sqlx(rename = "naver")]
    Naver,
}

impl std::fmt::Display for OAuthProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OAuthProvider::Google => write!(f, "google"),
            OAuthProvider::Naver => write!(f, "naver"),
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
}
