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
    tracing::info!("[P2P-TURN] credentials requested (key_id={})", state.config.cloudflare_turn.key_id);
    let client = reqwest::Client::new();

    let url = format!(
        "https://rtc.live.cloudflare.com/v1/turn/keys/{}/credentials/generate-ice-servers",
        state.config.cloudflare_turn.key_id
    );

    let body = serde_json::json!({
        "ttl": 60 * 60 * 24,
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", state.config.cloudflare_turn.api_token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    tracing::info!("[P2P-TURN] cloudflare responded status={}", status);
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        tracing::error!("[P2P-TURN] Cloudflare TURN API error: {} - {}", status, error_text);
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

    let turn_relays = ice_servers
        .iter()
        .filter(|s| s.urls.iter().any(|u| u.starts_with("turn:") || u.starts_with("turns:")))
        .count();
    tracing::info!(
        "[P2P-TURN] returning {} ice servers, {} turn relays",
        ice_servers.len(),
        turn_relays
    );

    Ok(Json(TurnCredentialsResponse { ice_servers }))
}
