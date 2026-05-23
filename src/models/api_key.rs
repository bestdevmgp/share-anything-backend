use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

use crate::models::personal_token::Scope;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: String,
    pub user_id: String,
    pub application_id: i64,
    pub key_hash: String,
    pub key_prefix: String,
    pub name: String,
    pub last_used_at: Option<DateTime<Utc>>,
    pub last_platform: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub expiration_notified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiKeyListItem {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    pub id: String,
    #[schema(example = "sk_a1b2c")]
    pub key_prefix: String,
    #[schema(example = "My API Key")]
    pub name: String,
    pub scopes: Vec<Scope>,
    #[schema(example = "2026-05-21T14:30:00Z")]
    pub last_used_at: Option<DateTime<Utc>>,
    #[schema(example = "2027-05-21T14:30:00Z")]
    pub expires_at: Option<DateTime<Utc>>,
    #[schema(example = "2026-05-21T14:30:00Z")]
    pub created_at: DateTime<Utc>,
}
