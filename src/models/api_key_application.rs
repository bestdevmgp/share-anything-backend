use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

use crate::models::personal_token::Scope;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(example = "pending")]
pub enum ApplicationStatus {
    Pending,
    Approved,
    Rejected,
    Cancelled,
}

impl ApplicationStatus {
    pub fn from_db(s: &str) -> Self {
        match s {
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct ApiKeyApplication {
    #[schema(example = 42)]
    pub id: i64,
    #[schema(example = "user_abc123")]
    pub user_id: String,
    #[schema(example = "MyDrive Cloud Backup")]
    pub service_name: String,
    #[schema(example = "https://mydrive.example.com")]
    pub service_url: String,
    #[schema(example = "We use ShareAnything to let our users share files larger than email allows...")]
    pub purpose: String,
    #[schema(example = "read,upload,delete")]
    pub scopes: String,
    #[schema(example = "2027-05-30T00:00:00Z")]
    pub requested_expires_at: Option<DateTime<Utc>>,
    #[schema(example = "pending")]
    pub status: String,
    #[schema(example = "Service URL is not reachable; please provide a valid public URL.")]
    pub reject_reason: Option<String>,
    #[schema(example = "tok_abc123")]
    pub api_key_id: Option<String>,
    #[schema(example = "203.0.113.5")]
    pub applicant_ip: Option<String>,
    #[schema(example = "web")]
    pub applicant_platform: Option<String>,
    #[schema(example = "2026-05-21T14:30:00Z")]
    pub created_at: DateTime<Utc>,
    #[schema(example = "2026-05-21T14:30:00Z")]
    pub reviewed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApplicationRequest {
    #[schema(example = "MyDrive Cloud Backup")]
    pub service_name: String,
    #[schema(example = "https://mydrive.example.com")]
    pub service_url: String,
    #[schema(example = "We use ShareAnything to let our users share files larger than email allows...")]
    pub purpose: String,
    pub scopes: Option<Vec<Scope>>,
    #[schema(example = "2027-05-30T00:00:00Z")]
    pub requested_expires_at: Option<DateTime<Utc>>,
    /// Client timezone offset in minutes from UTC. Positive east of UTC (e.g. KST = 540).
    #[schema(example = 540)]
    pub tz_offset_minutes: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApplicationResponse {
    #[schema(example = 42)]
    pub id: i64,
    #[schema(example = "MyDrive Cloud Backup")]
    pub service_name: String,
    #[schema(example = "https://mydrive.example.com")]
    pub service_url: String,
    #[schema(example = "We use ShareAnything to let our users share files larger than email allows...")]
    pub purpose: String,
    pub scopes: Vec<Scope>,
    #[schema(example = "2027-05-30T00:00:00Z")]
    pub requested_expires_at: Option<DateTime<Utc>>,
    pub status: ApplicationStatus,
    #[schema(example = "Service URL is not reachable; please provide a valid public URL.")]
    pub reject_reason: Option<String>,
    #[schema(example = "tok_abc123")]
    pub api_key_id: Option<String>,
    #[schema(example = "a1b2c3d4...")]
    pub reveal_token: Option<String>,
    #[schema(example = "2026-05-21T14:30:00Z")]
    pub created_at: DateTime<Utc>,
    #[schema(example = "2026-05-21T14:30:00Z")]
    pub reviewed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RejectRequest {
    #[schema(example = "Service URL is not reachable; please provide a valid public URL.")]
    pub reject_reason: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiKeyResponse {
    #[schema(example = "sk_a1b2c3")]
    pub key_prefix: String,
    #[schema(example = "API Key for MyDrive Cloud Backup")]
    pub name: String,
    #[schema(example = "2026-05-21T14:30:00Z")]
    pub created_at: DateTime<Utc>,
}

impl From<ApiKeyApplication> for ApplicationResponse {
    fn from(app: ApiKeyApplication) -> Self {
        Self {
            id: app.id,
            service_name: app.service_name,
            service_url: app.service_url,
            purpose: app.purpose,
            scopes: Scope::parse_list(&app.scopes),
            requested_expires_at: app.requested_expires_at,
            status: ApplicationStatus::from_db(&app.status),
            reject_reason: app.reject_reason,
            api_key_id: app.api_key_id,
            reveal_token: None,
            created_at: app.created_at,
            reviewed_at: app.reviewed_at,
        }
    }
}
