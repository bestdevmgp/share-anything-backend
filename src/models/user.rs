use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct User {
    pub id: String,
    #[serde(deserialize_with = "deserialize_oauth_provider")]
    pub oauth_provider: OAuthProvider,
    pub oauth_id: String,
    pub email: String,
    pub name: String,
    pub profile_image: Option<String>,
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
            _ => return Err(sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid oauth_provider: {}", provider_str)
            )))),
        };

        Ok(User {
            id: row.try_get("id")?,
            oauth_provider,
            oauth_id: row.try_get("oauth_id")?,
            email: row.try_get("email")?,
            name: row.try_get("name")?,
            profile_image: row.try_get("profile_image")?,
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
        _ => Err(serde::de::Error::custom(format!("Invalid oauth_provider: {}", s))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, ToSchema)]
#[sqlx(type_name = "VARCHAR", rename_all = "lowercase")]
pub enum OAuthProvider {
    Google,
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
