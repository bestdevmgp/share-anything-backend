use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::api::v1::{
    auth::{require_scope, require_token},
    error::PublicApiError,
    V1State,
};
use crate::handlers::cli::{
    cli_delete_upload, cli_download_history, cli_share_logs, cli_upload_history, CliState,
};
use crate::middleware::personal_token_auth::PersonalTokenUser;
use crate::models::personal_token::Scope;
use crate::models::{AppError, CliUploadHistoryQuery};
use crate::utils::PrettyJson;

/// Convert "this share belongs to another user" (CLI returns `403`) into a
/// `404` on the v1 surface. This makes share-code presence / ownership
/// **indistinguishable** to the caller — exactly the contract published in
/// the OpenAPI docs ("404 in both cases to prevent share-code enumeration").
/// Without this, an attacker could enumerate which 6-character codes exist by
/// watching `403` vs `404` responses.
fn hide_forbidden_as_not_found(e: AppError) -> PublicApiError {
    match e {
        AppError::Forbidden(_) => PublicApiError::NotFound("Resource not found".into()),
        other => other.into(),
    }
}

/// List my uploads
///
/// Return shares created by the authenticated user, ordered by creation time descending (newest first).
///
/// **Use case:** Build a dashboard of active shares, check download counts, or find the share code
/// of a file uploaded earlier from the CLI.
///
/// **Behaviour notes:**
/// - Pagination is zero-indexed: set `offset = 0` and `limit = 20` for the first page.
/// - The response envelope contains `items` (array), `total` (total count across all pages),
///   `limit`, and `offset`.
/// - Expired shares are **included** in the list until they are explicitly deleted; the caller can
///   filter by `expires_at` client-side.
/// - Download counts (`download_count`) reflect all downloads since creation, including anonymous ones.
///
/// **Required scope:** `read`
#[utoipa::path(
    get,
    path = "/v1/me/uploads",
    tag = "history",
    params(CliUploadHistoryQuery),
    responses(
        (status = 200,
            description = "Paginated list of the caller's shares. \
                           Response shape: `{ items: [...], total: N, limit: 20, offset: 0 }`. \
                           Each item includes `share_code`, `expires_at`, file metadata, and `download_count`."),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://shareany.app/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `read` scope (`error.code` will be `insufficient_scope`). \
                           Issue a new API key with the `read` scope checked.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn list_my_uploads(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Query(q): Query<CliUploadHistoryQuery>,
) -> Result<PrettyJson<serde_json::Value>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Read)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    cli_upload_history(
        State(cli_state),
        Some(axum::extract::Extension(user.clone())),
        Query(q),
    )
    .await
    .map_err(PublicApiError::from)
}

/// Delete one of my shares
///
/// Permanently remove a share created by the authenticated user.
///
/// **Use case:** Revoke access to a share immediately — for example, if it was shared with the
/// wrong recipient or if the content is no longer valid.
///
/// **Behaviour notes:**
/// - Deletion is **irreversible**. The share record, every associated file in object storage, and
///   the full download log are removed atomically.
/// - Any in-progress downloads at the moment of deletion may fail mid-stream with a `5xx` from the
///   storage backend; this is expected.
/// - Attempting to delete a share that does not exist **or** that belongs to a different user both
///   return `404` — the API intentionally does not distinguish between the two cases to prevent
///   enumeration of other users' share codes.
/// - A successful response body is empty; rely solely on the `204` status code.
///
/// **Required scope:** `delete`
#[utoipa::path(
    delete,
    path = "/v1/me/uploads/{code}",
    tag = "history",
    params(("code" = String, Path, description = "6-character case-sensitive alphanumeric share code to delete")),
    responses(
        (status = 204, description = "Share permanently deleted. Response body is empty."),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://shareany.app/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `delete` scope (`error.code` will be `insufficient_scope`). \
                           Issue a new API key with the `delete` scope checked.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 404,
            description = "Share does not exist or belongs to a different user. \
                           The API intentionally returns `404` in both cases to prevent share-code enumeration.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn delete_my_upload(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Path(code): Path<String>,
) -> Result<StatusCode, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Delete)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    cli_delete_upload(
        State(cli_state),
        Some(axum::extract::Extension(user.clone())),
        Path(code),
    )
    .await
    .map_err(hide_forbidden_as_not_found)
}

/// List downloads of one of my shares
///
/// Return the full download log for a specific share owned by the authenticated user, newest first.
///
/// **Use case:** Audit who has downloaded a share — useful for compliance, verifying delivery, or
/// detecting unexpected access patterns.
///
/// **Behaviour notes:**
/// - Each log entry includes: `downloaded_at` (ISO 8601), `ip_address` (e.g. `"203.0.113.42"`),
///   `platform` (e.g. `"macOS"`, `"iOS"`, `"Linux"`), and `downloader_name` (the display name of
///   the downloader if they were authenticated, otherwise `null`).
/// - IP addresses are logged as-received; they may be IPv4 or IPv6 depending on the CDN.
/// - The log is not paginated and returns all entries for the share at once.
/// - Returns `404` for non-existent codes **and** for codes that belong to a different user —
///   intentionally indistinguishable to prevent enumeration.
///
/// **Required scope:** `read`
#[utoipa::path(
    get,
    path = "/v1/me/uploads/{code}/downloads",
    tag = "history",
    params(("code" = String, Path, description = "6-character case-sensitive alphanumeric share code")),
    responses(
        (status = 200,
            description = "Array of download log entries for this share, newest first. \
                           Each entry contains `downloaded_at`, `ip_address`, `platform`, and `downloader_name` (nullable)."),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://shareany.app/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `read` scope (`error.code` will be `insufficient_scope`). \
                           Issue a new API key with the `read` scope checked.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 404,
            description = "Share does not exist or belongs to a different user. \
                           Both cases return `404` to prevent share-code enumeration.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn list_share_downloads(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Path(code): Path<String>,
) -> Result<PrettyJson<serde_json::Value>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Read)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    cli_share_logs(
        State(cli_state),
        Some(axum::extract::Extension(user.clone())),
        Path(code),
    )
    .await
    .map_err(hide_forbidden_as_not_found)
}

/// List my downloads
///
/// Return shares that the authenticated user has downloaded, ordered by download time descending
/// (most recent first).
///
/// **Use case:** Build a personal activity feed, recover the share code of a file you fetched
/// earlier, or audit your own download history.
///
/// **Behaviour notes:**
/// - Only downloads made while **authenticated** (i.e. using an API key or a session cookie)
///   appear in this list. Anonymous downloads are not attributed.
/// - If you downloaded the same share multiple times, each download appears as a separate entry.
/// - Pagination is zero-indexed (`offset = 0`, `limit = 20` for the first page). The response
///   envelope contains `items`, `total`, `limit`, and `offset`.
///
/// **Required scope:** `read`
#[utoipa::path(
    get,
    path = "/v1/me/downloads",
    tag = "history",
    params(CliUploadHistoryQuery),
    responses(
        (status = 200,
            description = "Paginated list of the caller's download history, newest first. \
                           Response shape: `{ items: [...], total: N, limit: 20, offset: 0 }`. \
                           Each item includes `share_code`, `downloaded_at`, and basic file metadata."),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://shareany.app/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `read` scope (`error.code` will be `insufficient_scope`). \
                           Issue a new API key with the `read` scope checked.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn list_my_downloads(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Query(q): Query<CliUploadHistoryQuery>,
) -> Result<PrettyJson<serde_json::Value>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Read)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    cli_download_history(
        State(cli_state),
        Some(axum::extract::Extension(user.clone())),
        Query(q),
    )
    .await
    .map_err(PublicApiError::from)
}
