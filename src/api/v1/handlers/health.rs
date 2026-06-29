use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    #[schema(example = "healthy")]
    pub status: String,
}

/// Health check
///
/// Liveness probe for the public API. Returns `{ "status": "healthy" }` while the
/// service is up and accepting requests.
///
/// **Use case:** Uptime monitoring and readiness checks before sending traffic.
///
/// **Authentication:** none — this endpoint is public.
#[utoipa::path(
    get,
    path = "/v1/health",
    tag = "health",
    responses(
        (status = 200, description = "Service is healthy and accepting requests", body = HealthResponse)
    )
)]
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
    })
}
