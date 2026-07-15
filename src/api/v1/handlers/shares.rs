use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Response,
};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::api::v1::{
    auth::{require_scope, require_token},
    error::PublicApiError,
    V1State,
};
use crate::handlers::cli::{
    cli_download, cli_download_complete, cli_download_url, cli_file_list, CliState,
};
use crate::middleware::personal_token_auth::PersonalTokenUser;
use crate::models::personal_token::Scope;
use crate::models::{CliDownloadCompleteRequest, CliDownloadUrlResponse, CliFileListResponse};
use crate::utils::PrettyJson;

/// Inspect a share
///
/// Return public metadata and the file list for a share code without downloading any bytes.
///
/// **Use case:** Render a pre-download information page or discover `file_id` values before
/// calling `GET /v1/shares/{code}/download` for a specific file in a multi-file share.
///
/// **Behaviour notes:**
/// - Share codes are **case-sensitive** 6-character alphanumeric strings (e.g. `Ab3xK9`).
/// - `has_password: true` means the download endpoint will require a `password` query parameter.
/// - `is_one_time: true` means the share will be permanently consumed after the first successful
///   download — this endpoint itself does **not** consume it.
/// - A share may expire or be consumed between this call and the subsequent download; callers
///   should handle `404` on the download endpoint even when this call returns `200`.
///
/// **Required scope:** `read`
#[utoipa::path(
    get,
    path = "/v1/shares/{code}",
    tag = "shares",
    params(("code" = String, Path, description = "6-character case-sensitive alphanumeric share code (e.g. `Ab3xK9`)")),
    responses(
        (status = 200, description = "Share metadata and file list", body = CliFileListResponse),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://shareany.app/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `read` scope (`error.code` will be `insufficient_scope`). \
                           Issue a new API key with the `read` scope checked.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 404,
            description = "No share exists for this code, the share has passed its expiration timestamp, \
                           or it was a one-time share that has already been consumed. Codes are \
                           case-sensitive; double-check capitalisation.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn get_share(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Path(code): Path<String>,
) -> Result<PrettyJson<CliFileListResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Read)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    cli_file_list(State(cli_state), Path(code))
        .await
        .map_err(PublicApiError::from)
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct DownloadQuery {
    #[schema(example = "hunter2")]
    pub password: Option<String>,
    #[schema(example = "e1234567-89ab-cdef-0123-456789abcdef")]
    pub file_id: Option<String>,
}

/// Download a share
///
/// Stream the raw file bytes for a share.
///
/// **Use case:** Programmatically fetch a file (or set of files) that was shared via the web UI
/// or the CLI `upload` command.
///
/// **Behaviour notes:**
/// - Omit `file_id` to download the **entire share**: a single-file share returns the file
///   as-is; a multi-file share returns a ZIP archive. The ZIP uses **store-only** (no compression)
///   for speed — the archive is created on-the-fly and streamed without buffering the whole thing
///   in memory first.
/// - Supply `file_id` (obtained from `GET /v1/shares/{code}`) to download exactly one file from a
///   multi-file share without receiving the full ZIP.
/// - The `Content-Disposition: attachment; filename="…"` header carries the suggested save name.
/// - **One-time shares** are consumed when the row is removed from the database; further requests
///   return `404`. There is no retry window.
/// - Password check happens server-side; the password is never forwarded to object storage.
///
/// **Required scope:** `read`
#[utoipa::path(
    get,
    path = "/v1/shares/{code}/download",
    tag = "shares",
    params(
        ("code" = String, Path, description = "6-character case-sensitive alphanumeric share code (e.g. `Ab3xK9`)"),
        ("password" = Option<String>, Query, description = "Required when `has_password` is `true`. Supplying a wrong value returns `403`."),
        ("file_id" = Option<String>, Query, description = "ULID of a single file within the share. Omit to fetch the whole share (single file or ZIP bundle)."),
    ),
    responses(
        (status = 200, description = "Streamed file bytes. `Content-Type` matches the original upload MIME type. \
                                      `Content-Disposition` carries the suggested filename. \
                                      For multi-file whole-share downloads the content type is `application/zip`."),
        (status = 400,
            description = "This share is configured for P2P transfer and cannot be downloaded via the REST API.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://shareany.app/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "Two distinct causes: \
                           (1) `error.code = insufficient_scope` — the API key does not have the `read` scope; issue a new API key with the `read` scope checked. \
                           (2) `error.code = forbidden` — the share is password-protected and the `password` \
                           query parameter was missing or incorrect; retry with the correct password.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 404,
            description = "No share exists for this code, the `file_id` does not belong to this share, \
                           the share has passed its expiration timestamp, or it was a one-time share that \
                           has already been consumed. Codes are case-sensitive 6-char alphanumeric strings.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 500,
            description = "Storage backend failed to serve the file.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn get_share_download(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Path(code): Path<String>,
    Query(q): Query<DownloadQuery>,
    headers: HeaderMap,
) -> Result<Response, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Read)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    let cli_query = crate::models::CliDownloadQuery {
        password: q.password,
        file_id: q.file_id,
    };
    cli_download(
        State(cli_state),
        Path(code),
        Query(cli_query),
        headers,
    )
    .await
    .map_err(PublicApiError::from)
}

/// Issue a presigned download URL
///
/// Request a short-lived URL that points directly at object storage. Download the file with
/// a plain `GET` — do not attach an API key. The presigned URL is the **fast path**; it
/// bypasses this API server so neither its bandwidth nor latency limits the transfer.
///
/// **One-time consumption is atomic and happens at URL-issue time.** For one-time shares,
/// receiving a `200` here permanently consumes the share — subsequent calls return `404`.
/// There is no retry window even if the actual R2 download fails.
///
/// **Download counting.** This endpoint does **not** record a download for multi-use shares.
/// Call `POST /v1/shares/{code}/download-complete` after the file has been pulled from R2 so
/// the count reflects actual completions, not URL issues.
///
/// **Required scope:** `read`
#[utoipa::path(
    post,
    path = "/v1/shares/{code}/download-url",
    tag = "shares",
    params(
        ("code" = String, Path, description = "6-character case-sensitive alphanumeric share code"),
        ("password" = Option<String>, Query, description = "Required when `has_password` is `true`. Supplying a wrong value returns `403`."),
        ("file_id" = Option<String>, Query, description = "ULID of a single file within the share. Omit to issue a URL for the first file."),
    ),
    responses(
        (status = 200, description = "Presigned download URL issued", body = CliDownloadUrlResponse),
        (status = 400,
            description = "This share is configured for P2P transfer and cannot be downloaded via the REST API.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "Either the API key lacks the `read` scope, or the share is password-protected \
                           and the `password` parameter was missing/incorrect.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 404,
            description = "No share exists for this code, the `file_id` does not belong to this share, \
                           or this was a one-time share that has already been consumed.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 500,
            description = "Failed to issue a presigned URL from object storage.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn post_share_download_url(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Path(code): Path<String>,
    Query(q): Query<DownloadQuery>,
    headers: HeaderMap,
) -> Result<PrettyJson<CliDownloadUrlResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Read)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    let cli_query = crate::models::CliDownloadQuery {
        password: q.password,
        file_id: q.file_id,
    };
    cli_download_url(
        State(cli_state),
        Path(code),
        Query(cli_query),
        headers,
    )
    .await
    .map_err(PublicApiError::from)
}

/// Record a completed direct download
///
/// Call this after the file has been successfully fetched from the presigned URL returned by
/// `POST /v1/shares/{code}/download-url`. It writes a `download_logs` row so the share's
/// download count reflects real completions rather than URL issues.
///
/// This call is **best-effort**: it returns `204` even when the share has already been
/// deleted (one-time consumption, expiry, manual delete), so the CLI does not need to handle
/// failure here.
///
/// **Required scope:** `read`
#[utoipa::path(
    post,
    path = "/v1/shares/{code}/download-complete",
    tag = "shares",
    params(("code" = String, Path, description = "6-character case-sensitive alphanumeric share code")),
    request_body = CliDownloadCompleteRequest,
    responses(
        (status = 204, description = "Completion recorded (or no-op if share is already gone)"),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `read` scope.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn post_share_download_complete(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Path(code): Path<String>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<CliDownloadCompleteRequest>,
) -> Result<axum::http::StatusCode, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Read)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    cli_download_complete(
        State(cli_state),
        Path(code),
        headers,
        axum::Json(body),
    )
    .await
    .map_err(PublicApiError::from)
}
