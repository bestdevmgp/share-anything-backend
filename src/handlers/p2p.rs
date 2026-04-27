use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    models::AppError,
    services::signaling::SignalingState,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct P2pStatusQuery {
    pub code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct P2pStatusResponse {
    pub share_code: String,
    pub uploader_online: bool,
}

#[utoipa::path(
    get,
    path = "/p2p/status",
    tag = "p2p",
    params(
        ("code" = String, Query, description = "Share code")
    ),
    responses(
        (status = 200, description = "P2P status retrieved", body = P2pStatusResponse),
    )
)]
pub async fn check_uploader_status(
    State(state): State<SignalingState>,
    Query(query): Query<P2pStatusQuery>,
) -> Result<Json<P2pStatusResponse>, AppError> {
    let is_online = state.find_uploader(&query.code).is_some();

    Ok(Json(P2pStatusResponse {
        share_code: query.code,
        uploader_online: is_online,
    }))
}
