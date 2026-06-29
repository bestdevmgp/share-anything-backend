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

/// List all active sessions (web and CLI) for the authenticated user.
#[utoipa::path(
    get,
    path = "/user/sessions",
    tag = "sessions",
    responses(
        (status = 200, description = "Sessions listed successfully", body = Vec<SessionResponse>),
        (status = 401, description = "Unauthorized")
    ),
    security(("cookie_auth" = []))
)]
pub async fn list_sessions(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<SessionResponse>>, AppError> {
    let sessions = repository::find_sessions_by_user(&state.db, &claims.sub).await?;
    let cli_sessions = repository::find_active_cli_sessions_by_user(&state.db, &claims.sub).await?;

    let mut response: Vec<SessionResponse> = sessions
        .into_iter()
        .map(|s| SessionResponse {
            is_current: s.jti == claims.jti,
            jti: s.jti,
            device_label: s.device_label,
            ip_address: s.ip_address,
            location: s.location,
            last_seen_at: s.last_seen_at,
            created_at: s.created_at,
            kind: "web".to_string(),
        })
        .collect();

    for t in cli_sessions {
        if let Some(last_used_at) = t.last_used_at {
            let label = match &t.last_platform {
                Some(p) if !p.is_empty() => format!("CLI on {}", p),
                _ => "CLI".to_string(),
            };
            response.push(SessionResponse {
                jti: t.id,
                device_label: Some(label),
                ip_address: String::from("-"),
                location: None,
                last_seen_at: last_used_at,
                created_at: t.created_at,
                is_current: false,
                kind: "cli".to_string(),
            });
        }
    }

    response.sort_by(|a, b| b.last_seen_at.cmp(&a.last_seen_at));

    Ok(Json(response))
}

/// Terminate a specific session by JTI (or CLI token ID).
#[utoipa::path(
    delete,
    path = "/user/sessions/{jti}",
    tag = "sessions",
    params(
        ("jti" = String, Path, description = "Session JTI or CLI token ID")
    ),
    responses(
        (status = 204, description = "Session terminated"),
        (status = 400, description = "Cannot terminate current session"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Session not found")
    ),
    security(("cookie_auth" = []))
)]
pub async fn terminate_session(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
    Path(jti): Path<String>,
) -> Result<StatusCode, AppError> {
    if jti == claims.jti {
        return Err(bad_request("Cannot terminate the active session"));
    }
    let rows = repository::delete_session(&state.db, &claims.sub, &jti).await?;
    if rows > 0 {
        return Ok(StatusCode::NO_CONTENT);
    }
    let cli_rows = repository::revoke_personal_token(&state.db, &jti, &claims.sub).await?;
    if cli_rows == 0 {
        return Err(not_found("Session not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Terminate all sessions other than the current one.
#[utoipa::path(
    delete,
    path = "/user/sessions",
    tag = "sessions",
    responses(
        (status = 204, description = "Other sessions terminated"),
        (status = 401, description = "Unauthorized")
    ),
    security(("cookie_auth" = []))
)]
pub async fn terminate_other_sessions(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
) -> Result<StatusCode, AppError> {
    repository::delete_other_sessions(&state.db, &claims.sub, &claims.jti).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// List trusted devices for the authenticated user.
#[utoipa::path(
    get,
    path = "/user/trusted-devices",
    tag = "sessions",
    responses(
        (status = 200, description = "Trusted devices listed", body = Vec<TrustedDeviceResponse>),
        (status = 401, description = "Unauthorized")
    ),
    security(("cookie_auth" = []))
)]
pub async fn list_trusted_devices(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<TrustedDeviceResponse>>, AppError> {
    let devices = repository::find_trusted_devices_by_user(&state.db, &claims.sub).await?;

    let response: Vec<TrustedDeviceResponse> = devices
        .into_iter()
        .map(|d| TrustedDeviceResponse {
            id: d.id,
            device_id: d.device_id,
            device_label: d.device_label,
            ip_address: d.ip_address,
            location: d.location,
            trusted_at: d.trusted_at,
        })
        .collect();

    Ok(Json(response))
}

/// Remove a trusted device by ID.
#[utoipa::path(
    delete,
    path = "/user/trusted-devices/{id}",
    tag = "sessions",
    params(
        ("id" = String, Path, description = "Trusted device ID")
    ),
    responses(
        (status = 204, description = "Trusted device removed"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Trusted device not found")
    ),
    security(("cookie_auth" = []))
)]
pub async fn delete_trusted_device(
    State(state): State<SessionsState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rows = repository::delete_trusted_device(&state.db, &claims.sub, &id).await?;
    if rows == 0 {
        return Err(not_found("Trusted device not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
