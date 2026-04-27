use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct CliAuthSessionResponse {
    pub session_id: String,
    pub login_url: String,
    pub expires_in_seconds: i64,
}

#[derive(Serialize)]
pub struct CliAuthStatusResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personal_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub struct CompleteCliAuthRequest {}
