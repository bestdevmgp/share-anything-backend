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
    ice_servers: CloudflareIceServers,
}

#[derive(Debug, Deserialize)]
struct CloudflareIceServers {
    urls: Vec<String>,
    username: String,
    credential: String,
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
    let ice_servers = vec![
        // Cloudflare free STUN server
        IceServer {
            urls: vec!["stun:stun.cloudflare.com:3478".to_string()],
            username: None,
            credential: None,
        },
        // Cloudflare TURN server with credentials
        IceServer {
            urls: cf_response.ice_servers.urls,
            username: Some(cf_response.ice_servers.username),
            credential: Some(cf_response.ice_servers.credential),
        },
    ];

    Ok(Json(TurnCredentialsResponse { ice_servers }))
}