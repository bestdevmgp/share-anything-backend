use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(example = "read")]
pub enum Scope {
    Read,
    Upload,
    Delete,
    P2pTransfer,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Upload => "upload",
            Scope::Delete => "delete",
            Scope::P2pTransfer => "p2p_transfer",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Scope::Read),
            "upload" => Some(Scope::Upload),
            "delete" => Some(Scope::Delete),
            "p2p_transfer" => Some(Scope::P2pTransfer),
            _ => None,
        }
    }

    pub fn parse_list(csv: &str) -> Vec<Scope> {
        csv.split(',').filter_map(|s| Scope::parse(s.trim())).collect()
    }

    pub fn format_list(scopes: &[Scope]) -> String {
        scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PersonalToken {
    pub id: String,
    pub user_id: String,
    pub token_hash: String,
    pub token_prefix: String,
    pub name: String,
    pub last_used_at: Option<DateTime<Utc>>,
    pub last_platform: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PersonalTokenResponse {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    pub id: String,
    #[schema(example = "sat_a1b2c")]
    pub token_prefix: String,
    #[schema(example = "My Token")]
    pub name: String,
    #[schema(example = "2026-05-21T14:30:00Z")]
    pub last_used_at: Option<DateTime<Utc>>,
    #[schema(example = "2027-05-21T14:30:00Z")]
    pub expires_at: Option<DateTime<Utc>>,
    #[schema(example = "2026-05-21T14:30:00Z")]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreatePersonalTokenResponse {
    pub id: String,
    pub personal_token: String,
    pub token_prefix: String,
    pub name: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreatePersonalTokenRequest {
    pub name: Option<String>,
    pub expires_in_days: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_roundtrip() {
        let scopes = vec![Scope::Read, Scope::Upload];
        let s = Scope::format_list(&scopes);
        assert_eq!(s, "read,upload");
        assert_eq!(Scope::parse_list(&s), scopes);
    }

    #[test]
    fn scope_parse_ignores_unknown() {
        assert_eq!(
            Scope::parse_list("read, bogus ,delete"),
            vec![Scope::Read, Scope::Delete]
        );
    }

    #[test]
    fn scope_parse_empty() {
        assert!(Scope::parse_list("").is_empty());
    }
}
