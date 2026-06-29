use axum::{extract::State, http::HeaderMap, Extension, Json};
use serde::Serialize;

use crate::db::{repository, DbPool};
use crate::middleware::auth::Claims;
use crate::models::AppError;

#[derive(Serialize, utoipa::ToSchema)]
pub struct DailyQuotaResponse {
    pub used_bytes: i64,
    pub limit_bytes: i64,
    pub remaining_bytes: i64,
    pub resets_at: String,
    pub authenticated: bool,
}

#[utoipa::path(
    get,
    path = "/file/quota",
    tag = "upload",
    responses(
        (status = 200, description = "Daily upload quota for the caller (anonymous or authenticated)", body = DailyQuotaResponse)
    )
)]
pub async fn get_daily_quota(
    State(db): State<DbPool>,
    claims: Option<Extension<Claims>>,
    headers: HeaderMap,
) -> Result<Json<DailyQuotaResponse>, AppError> {
    let claims = claims.map(|e| e.0);
    let user_id = claims.as_ref().map(|c| c.sub.as_str());

    let identity = crate::utils::quota_identity(user_id, &headers);
    let used = repository::get_daily_upload_usage(&db, &identity, crate::utils::kst_today()).await?;
    let limit = crate::utils::daily_limit_for(user_id);
    let remaining = (limit - used).max(0);

    Ok(Json(DailyQuotaResponse {
        used_bytes: used,
        limit_bytes: limit,
        remaining_bytes: remaining,
        resets_at: crate::utils::next_kst_reset().to_rfc3339(),
        authenticated: user_id.is_some(),
    }))
}
