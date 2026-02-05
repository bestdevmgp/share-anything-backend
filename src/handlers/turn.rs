use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::config::Config;

#[derive(Clone)]
pub struct TurnState {
    pub config: Arc<Config>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TurnCredentialsResponse {
    pub ice_servers: Vec<IceServer>,
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

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
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
        (status = 500, description = "Failed to get TURN credentials", body = ErrorResponse)
    )
)]
pub async fn get_turn_credentials(
    State(state): State<TurnState>,
) -> Result<Json<TurnCredentialsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let client = reqwest::Client::new();

    // Cloudflare Calls API endpoint
    let url = format!(
        "https://rtc.live.cloudflare.com/v1/turn/keys/{}/credentials/generate-ice-servers",
        state.config.cloudflare_turn.key_id
    );

    // Request body with TTL (24 hours = 86400 seconds)
    let body = serde_json::json!({
        "ttl": 86400
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", state.config.cloudflare_turn.api_token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Failed to request TURN credentials: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to request TURN credentials".to_string(),
                }),
            )
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        tracing::error!("Cloudflare TURN API error: {} - {}", status, error_text);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Cloudflare TURN API error: {}", status),
            }),
        ));
    }

    let cf_response: CloudflareIceServersResponse = response.json().await.map_err(|e| {
        tracing::error!("Failed to parse TURN credentials response: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to parse TURN credentials".to_string(),
            }),
        )
    })?;

    // Build ICE servers list with Cloudflare STUN and TURN servers
    let mut ice_servers = vec![
        // Cloudflare free STUN server
        IceServer {
            urls: vec!["stun:stun.cloudflare.com:3478".to_string()],
            username: None,
            credential: None,
        },
    ];

    // Add all TURN servers from Cloudflare response
    for server in cf_response.ice_servers {
        ice_servers.push(IceServer {
            urls: server.urls.into_vec(),
            username: server.username,
            credential: server.credential,
        });
    }

    Ok(Json(TurnCredentialsResponse { ice_servers }))
}