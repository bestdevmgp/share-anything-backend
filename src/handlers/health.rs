//! Dependency-specific health probes for external uptime monitoring.
//!
//! These are intentionally separate from the plain `GET /health` liveness route
//! (which stays dependency-free). Each probe is unauthenticated, idempotent and
//! cheap, exercises exactly one dependency, and returns a minimal `{status}` body
//! with no internal/credential detail (details are logged server-side instead).
//!
//! `AppError` has no 503 variant (its `Database`/`Internal` map to 500), so the
//! "down" responses here are built manually with `SERVICE_UNAVAILABLE` rather than
//! via `?` — a 500 would also (a) misreport the status and (b) trip
//! `discord_error_middleware`. That middleware additionally skips `/health*`, so a
//! real outage polled every few minutes does not spam the Discord webhook.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::db::DbPool;
use crate::services::StorageService;

/// Probe budget — keeps a saturated pool / hung dependency from making the probe
/// itself hang (which would otherwise flap the monitor).
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct HealthState {
    pub db: DbPool,
    pub storage: StorageService,
    pub config: Arc<Config>,
}

fn ok() -> Response {
    (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response()
}

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "status": "unavailable" })),
    )
        .into_response()
}

/// `GET /health/db` — MySQL reachability via a real `SELECT 1` round-trip.
///
/// A round-trip (not `pool.acquire()`) is used on purpose: `acquire()` can hand
/// back a cached connection to a dead/failed-over server (false healthy), and can
/// also block until the acquire timeout when the pool is merely saturated by real
/// traffic (false unhealthy).
pub async fn health_db(State(state): State<HealthState>) -> Response {
    match tokio::time::timeout(PROBE_TIMEOUT, sqlx::query("SELECT 1").execute(&state.db)).await {
        Ok(Ok(_)) => ok(),
        Ok(Err(e)) => {
            tracing::warn!("DB health check (SELECT 1) failed: {}", e);
            unavailable()
        }
        Err(_) => {
            tracing::warn!("DB health check timed out after {:?}", PROBE_TIMEOUT);
            unavailable()
        }
    }
}

/// `GET /health/r2` — R2/S3 connectivity via a cheap credentialed `HeadObject` on
/// a reserved key (a 404 proves R2 is reachable + credentials valid). No object
/// listing or data transfer. See `StorageService::health_check`.
pub async fn health_r2(State(state): State<HealthState>) -> Response {
    match tokio::time::timeout(PROBE_TIMEOUT, state.storage.health_check()).await {
        Ok(true) => ok(),
        Ok(false) => unavailable(),
        Err(_) => {
            tracing::warn!("R2 health check timed out after {:?}", PROBE_TIMEOUT);
            unavailable()
        }
    }
}

/// `GET /health/turn` — P2P transfer dependency: verifies the backend can mint TURN
/// credentials from Cloudflare's RTC API (a successful `generate-ice-servers` call
/// proves reachability + a valid Turn key/token). No credentials are returned to the
/// caller — the body is a minimal `{status}`. P2P/secure transfer can't connect peers
/// without this; standard uploads are unaffected.
pub async fn health_turn(State(state): State<HealthState>) -> Response {
    let url = format!(
        "https://rtc.live.cloudflare.com/v1/turn/keys/{}/credentials/generate-ice-servers",
        state.config.cloudflare_turn.key_id
    );
    let req = reqwest::Client::new()
        .post(&url)
        .header(
            "Authorization",
            format!("Bearer {}", state.config.cloudflare_turn.api_token),
        )
        .json(&json!({ "ttl": 600 }))
        .send();

    match tokio::time::timeout(PROBE_TIMEOUT, req).await {
        Ok(Ok(resp)) if resp.status().is_success() => ok(),
        Ok(Ok(resp)) => {
            tracing::warn!("TURN health check: Cloudflare RTC returned {}", resp.status());
            unavailable()
        }
        Ok(Err(e)) => {
            tracing::warn!("TURN health check request failed: {}", e);
            unavailable()
        }
        Err(_) => {
            tracing::warn!("TURN health check timed out after {:?}", PROBE_TIMEOUT);
            unavailable()
        }
    }
}
