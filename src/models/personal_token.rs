use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(example = "read")]
pub enum Scope {
    Read,
    Upload,
    Delete,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Upload => "upload",
            Scope::Delete => "delete",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Scope::Read),
            "upload" => Some(Scope::Upload),
            "delete" => Some(Scope::Delete),
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
    pub scopes: String,
    pub last_used_at: Option<DateTime<Utc>>,
    pub last_platform: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl PersonalToken {
    pub fn scopes_vec(&self) -> Vec<Scope> {
        Scope::parse_list(&self.scopes)
    }
}

#[derive(Debug, Serialize)]
pub struct PersonalTokenResponse {
    pub id: String,
    pub token_prefix: String,
    pub name: String,
    pub scopes: Vec<Scope>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CreatePersonalTokenResponse {
    pub id: String,
    pub personal_token: String,
    pub token_prefix: String,
    pub name: String,
    pub scopes: Vec<Scope>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePersonalTokenRequest {
    pub name: Option<String>,
    pub scopes: Option<Vec<Scope>>,
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
