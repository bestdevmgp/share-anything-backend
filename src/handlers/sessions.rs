use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};

use crate::{
    db::{repository, DbPool},
    middleware::auth::Claims,
    models::{
        bad_request, not_found,
        session::{SessionResponse, TrustedDeviceResponse},
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
    if jti == claims.jti {
        return Err(bad_request("현재 사용 중인 세션은 종료할 수 없습니다"));
    }
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

pub async fn list_trusted_devices(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<TrustedDeviceResponse>>, AppError> {
    let devices = repository::find_trusted_devices_by_user(&state.db, &claims.sub).await?;

    let response: Vec<TrustedDeviceResponse> = devices
        .into_iter()
        .map(|d| TrustedDeviceResponse {
            id: d.id,
            device_label: d.device_label,
            ip_address: d.ip_address,
            location: d.location,
            trusted_at: d.trusted_at,
        })
        .collect();

    Ok(Json(response))
}

pub async fn delete_trusted_device(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rows = repository::delete_trusted_device(&state.db, &claims.sub, &id).await?;
    if rows == 0 {
        return Err(not_found("신뢰 기기를 찾을 수 없습니다"));
    }
    Ok(StatusCode::NO_CONTENT)
}
