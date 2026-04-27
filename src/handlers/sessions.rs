use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};

use crate::{
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{
        not_found,
        session::{BlockedDeviceResponse, SessionResponse},
        AppError,
    },
};

#[derive(Clone)]
pub struct SessionsState {
    pub db: DbPool,
}

pub async fn list_sessions(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<SessionResponse>>, AppError> {
    let sessions = repository::find_sessions_by_user(&state.db, &claims.sub).await?;

    let response: Vec<SessionResponse> = sessions
        .into_iter()
        .map(|s| SessionResponse {
            is_current: s.jti == claims.jti,
            jti: s.jti,
            device_label: s.device_label,
            ip_address: s.ip_address,
            location: s.location,
            last_seen_at: s.last_seen_at,
            created_at: s.created_at,
        })
        .collect();

    Ok(Json(response))
}

pub async fn terminate_session(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
    Path(jti): Path<String>,
) -> Result<StatusCode, AppError> {
    let rows = repository::delete_session(&state.db, &claims.sub, &jti).await?;
    if rows == 0 {
        return Err(not_found("세션을 찾을 수 없습니다"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn terminate_other_sessions(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
) -> Result<StatusCode, AppError> {
    repository::delete_other_sessions(&state.db, &claims.sub, &claims.jti).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn block_session(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
    Path(jti): Path<String>,
) -> Result<StatusCode, AppError> {
    let session = repository::find_session(&state.db, &jti)
        .await?
        .ok_or_else(|| not_found("세션을 찾을 수 없습니다"))?;

    if session.user_id != claims.sub {
        return Err(not_found("세션을 찾을 수 없습니다"));
    }

    repository::add_blocked_device(
        &state.db,
        &claims.sub,
        &session.user_agent_hash,
        session.user_agent.as_deref(),
        &session.ip_address,
        session.device_label.as_deref(),
    )
    .await?;

    repository::delete_session(&state.db, &claims.sub, &jti).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_blocked_devices(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<BlockedDeviceResponse>>, AppError> {
    let devices = repository::find_blocked_devices_by_user(&state.db, &claims.sub).await?;

    let response: Vec<BlockedDeviceResponse> = devices
        .into_iter()
        .map(|d| BlockedDeviceResponse {
            id: d.id,
            device_label: d.device_label,
            ip_address: d.ip_address,
            blocked_at: d.blocked_at,
        })
        .collect();

    Ok(Json(response))
}

pub async fn unblock_device(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rows = repository::delete_blocked_device(&state.db, &claims.sub, &id).await?;
    if rows == 0 {
        return Err(not_found("차단된 기기를 찾을 수 없습니다"));
    }
    Ok(StatusCode::NO_CONTENT)
}
