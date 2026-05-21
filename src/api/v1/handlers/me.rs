use axum::{extract::State, Json};
use serde::Serialize;
use utoipa::ToSchema;

use crate::api::v1::{
    auth::{require_scope, require_token},
    error::PublicApiError,
    V1State,
};
use crate::db::repository;
use crate::middleware::personal_token_auth::PersonalTokenUser;
use crate::models::personal_token::Scope;

#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    pub user_id: String,
    #[schema(example = "alice@example.com")]
    pub email: String,
    #[schema(example = "Alice")]
    pub name: String,
    #[schema(example = "7c9e6679-7425-40de-944b-e07fc1f90ae7")]
    pub token_id: String,
    pub scopes: Vec<Scope>,
}

/// Get authenticated principal
///
/// Returns the user account and Personal Access Token details for the token used in this request.
///
/// **Use case:** Send this as a first call after obtaining a token to confirm it is valid and to
/// discover its granted scopes before issuing other API calls.
///
/// **Behaviour notes:**
/// - The `token_id` is a stable ULID that uniquely identifies the PAT — use it to label log lines.
/// - `scopes` reflects only the permissions granted to this specific token; the user may have other
///   tokens with different scope sets.
/// - If the underlying user account has been deleted (e.g. after OAuth account removal) this endpoint
///   returns `401` even though the token signature is valid.
///
/// **Required scope:** `read`
#[utoipa::path(
    get,
    path = "/v1/me",
    tag = "me",
    responses(
        (status = 200, description = "Authenticated principal", body = MeResponse),
        (status = 401,
            description = "Personal Token is missing, malformed (must start with `sa_`), revoked, \
                           expired, or associated with a deleted user account. \
                           Re-issue a token in the web dashboard.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "Token exists and is valid but does not carry the `read` scope. \
                           Issue a new token with the `read` scope checked.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("personal_token" = []))
)]
pub async fn get_me(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
) -> Result<Json<MeResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Read)?;

    let db_user = repository::find_user_by_id(&state.db, &user.user_id)
        .await
        .map_err(|_| PublicApiError::Internal)?
        .ok_or_else(|| PublicApiError::Unauthorized("Token is associated with a deleted user.".into()))?;

    Ok(Json(MeResponse {
        user_id: db_user.id,
        email: db_user.email,
        name: db_user.name,
        token_id: user.personal_token_id.clone(),
        scopes: user.scopes.clone(),
    }))
}
