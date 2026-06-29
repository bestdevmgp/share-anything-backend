use axum::{extract::State, Json};
use serde::Serialize;
use utoipa::ToSchema;

use crate::api::v1::{auth::require_token, error::PublicApiError, V1State};
use crate::middleware::personal_token_auth::PersonalTokenUser;
use crate::middleware::rate_limiter::Bucket;
use crate::services::signaling::{MAX_ACTIVE_PER_KEY, MAX_CONNECT_ATTEMPTS_PER_MINUTE};

#[derive(Serialize, ToSchema)]
pub struct ResourceLimit {
    #[schema(example = 500)]
    pub limit: u32,
    #[schema(example = 12)]
    pub used: u32,
    #[schema(example = 488)]
    pub remaining: u32,
    #[schema(example = 1719829200)]
    pub reset: u64,
}

#[derive(Serialize, ToSchema)]
pub struct ResourceLimits {
    pub read: ResourceLimit,
    pub upload: ResourceLimit,
    pub download: ResourceLimit,
}

#[derive(Serialize, ToSchema)]
pub struct ConcurrencyGauge {
    #[schema(example = 10)]
    pub limit: u32,
    #[schema(example = 2)]
    pub active: u32,
    #[schema(example = 8)]
    pub available: u32,
}

#[derive(Serialize, ToSchema)]
pub struct AttemptLimit {
    #[schema(example = 30)]
    pub limit: u32,
    #[schema(example = 60)]
    pub window_seconds: u32,
    #[schema(example = 5)]
    pub used: u32,
    #[schema(example = 25)]
    pub remaining: u32,
    #[schema(example = 1719829260)]
    pub reset: u64,
}

#[derive(Serialize, ToSchema)]
pub struct P2pLimits {
    pub concurrent_connections: ConcurrencyGauge,
    pub connect_attempts: AttemptLimit,
}

#[derive(Serialize, ToSchema)]
pub struct RateLimitResponse {
    pub resources: ResourceLimits,
    pub p2p: P2pLimits,
}

#[utoipa::path(
    get,
    path = "/v1/rate-limit",
    tag = "rate-limit",
    responses(
        (status = 200,
            description = "Current rate-limit usage for the API key making the request. The hourly request buckets (read, upload, download) report limit, used, remaining, and a reset time (Unix epoch seconds). P2P `concurrent_connections` is a live gauge — the number of signaling connections open right now out of the cap, with no reset because a slot frees the moment a connection closes. P2P `connect_attempts` is the per-minute signaling rate limit. Calling this endpoint does not count against any limit.",
            body = RateLimitResponse),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn get_rate_limit(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
) -> Result<Json<RateLimitResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    let key = user.user_id.as_str();

    let resource = |bucket: Bucket| {
        let status = state.cli_rate_limiter.peek(key, bucket);
        ResourceLimit {
            limit: status.limit,
            used: status.limit.saturating_sub(status.remaining),
            remaining: status.remaining,
            reset: status.reset_unix,
        }
    };

    let active = state.signaling.active_count(key) as u32;
    let (attempts_used, attempts_reset) = state.signaling.attempt_status(key);
    let active_limit = MAX_ACTIVE_PER_KEY as u32;
    let attempt_limit = MAX_CONNECT_ATTEMPTS_PER_MINUTE;

    Ok(Json(RateLimitResponse {
        resources: ResourceLimits {
            read: resource(Bucket::Read),
            upload: resource(Bucket::Upload),
            download: resource(Bucket::Download),
        },
        p2p: P2pLimits {
            concurrent_connections: ConcurrencyGauge {
                limit: active_limit,
                active,
                available: active_limit.saturating_sub(active),
            },
            connect_attempts: AttemptLimit {
                limit: attempt_limit,
                window_seconds: 60,
                used: attempts_used,
                remaining: attempt_limit.saturating_sub(attempts_used),
                reset: attempts_reset,
            },
        },
    }))
}
