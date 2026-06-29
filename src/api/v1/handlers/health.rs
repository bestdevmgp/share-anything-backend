use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    #[schema(example = "healthy")]
    pub status: String,
}

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
