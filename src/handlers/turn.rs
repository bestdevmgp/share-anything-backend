use axum::{extract::State, Json};
use serde::Deserialize;
use std::sync::Arc;

use crate::{
    config::Config,
    models::{internal_error, AppError, IceServer, TurnCredentialsResponse},
};

#[derive(Clone)]
pub struct TurnState {
    pub config: Arc<Config>,
}

#[derive(Debug, Deserialize)]
struct CloudflareIceServersResponse {
    #[serde(rename = "iceServers")]
    ice_servers: Vec<CloudflareIceServer>,
}

#[derive(Debug, Deserialize)]
struct CloudflareIceServer {
    urls: StringOrVec,
    username: Option<String>,
    credential: Option<String>,
}

// Cloudflare API can return urls as either a string or array
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StringOrVec {
    Single(String),
    Multiple(Vec<String>),
}

impl StringOrVec {
    fn into_vec(self) -> Vec<String> {
        match self {
            StringOrVec::Single(s) => vec![s],
            StringOrVec::Multiple(v) => v,
        }
    }
}

/// Get TURN server credentials
///
/// Returns ICE server configuration including STUN and TURN servers
/// with temporary credentials from Cloudflare Calls.
#[utoipa::path(
    get,
    path = "/turn/credentials",
    tag = "turn",
    responses(
        (status = 200, description = "TURN credentials retrieved successfully", body = TurnCredentialsResponse),
        (status = 500, description = "Failed to get TURN credentials", body = crate::models::ErrorResponse)
    )
)]
pub async fn get_turn_credentials(
    State(state): State<TurnState>,
) -> Result<Json<TurnCredentialsResponse>, AppError> {
    let client = reqwest::Client::new();

    let url = format!(
        "https://rtc.live.cloudflare.com/v1/turn/keys/{}/credentials/generate-ice-servers",
        state.config.cloudflare_turn.key_id
    );

    // 24 hours TTL
    let body = serde_json::json!({
        "ttl": 86400
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", state.config.cloudflare_turn.api_token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        tracing::error!("Cloudflare TURN API error: {} - {}", status, error_text);
        return Err(internal_error(format!("Cloudflare TURN API error: {}", status)));
    }

    let cf_response: CloudflareIceServersResponse = response.json().await?;

    let mut ice_servers = vec![
        IceServer {
            urls: vec!["stun:stun.cloudflare.com:3478".to_string()],
            username: None,
            credential: None,
        },
    ];

    for server in cf_response.ice_servers {
        ice_servers.push(IceServer {
            urls: server.urls.into_vec(),
            username: server.username,
            credential: server.credential,
        });
    }

    Ok(Json(TurnCredentialsResponse { ice_servers }))
}
