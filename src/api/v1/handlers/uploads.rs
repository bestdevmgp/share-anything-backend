use axum::{
    extract::{Multipart, State},
    Json,
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::api::v1::{
    auth::{require_scope, require_token},
    error::PublicApiError,
    V1State,
};
use crate::handlers::cli::{cli_complete_multipart, cli_multipart_init, cli_presign_parts, cli_upload, CliState};
use crate::middleware::personal_token_auth::PersonalTokenUser;
use crate::models::personal_token::Scope;
use crate::models::{
    CliCompleteMultipartRequest, CliMultipartInitRequest, CliMultipartInitResponse,
    CliPresignPartsRequest, CliPresignPartsResponse, CliUploadResponse,
};
use crate::utils::PrettyJson;

/// Successful upload response (v1 contract — excludes the legacy `curl_command` field).
#[derive(Debug, Serialize, ToSchema)]
pub struct V1UploadResponse {
    #[schema(example = "482917")]
    pub share_code: String,
    #[schema(example = json!(["report.pdf", "photo.jpg"]))]
    pub files: Vec<String>,
    #[schema(example = "2026-05-21 14:30")]
    pub expires_at: String,
}

impl From<CliUploadResponse> for V1UploadResponse {
    fn from(c: CliUploadResponse) -> Self {
        Self {
            share_code: c.share_code,
            files: c.files,
            expires_at: c.expires_at,
        }
    }
}

/// Create an upload (single-shot)
///
/// Upload one or more files as a single `multipart/form-data` request and create a new share.
///
/// **Use case:** Quick, fire-and-forget sharing of small-to-medium files (under ~100 MB). For
/// larger files use the three-step multipart flow so each part is uploaded directly to object
/// storage without passing through the API server.
///
/// **Form fields:**
/// - `file` (required, repeatable) — file binary; include one `file` part per file. Multiple
///   files are bundled into a single share and can optionally be downloaded as a ZIP.
/// - `description` (optional) — free-text note shown to the recipient on the download page.
/// - `password` (optional) — gate the share behind a password that recipients must supply.
/// - `expiration` (optional) — one of `5m`, `30m`, `1h`, `3h`, `6h`, `12h`, `24h`.
///   Defaults to `24h`. The share is permanently inaccessible after this window.
/// - `is_one_time` (optional, `"true"` / `"false"`) — if `true`, the share self-destructs after
///   the first successful download.
///
/// **Behaviour notes:**
/// - The share code in the response is the canonical identifier used in all subsequent calls.
/// - Uploading zero bytes files is rejected with `400`.
///
/// **Limits:** Single file size and per-API-key active storage limits apply. Exact values are
/// published at <https://share.mingyu.dev/api-terms-of-use>. Exceeding them returns `413` or
/// `429` respectively — error messages do not include the numeric limits.
///
/// **Required scope:** `upload`
#[utoipa::path(
    post,
    path = "/v1/uploads",
    tag = "uploads",
    responses(
        (status = 200, description = "Upload accepted and share created. The `share_code` is immediately live.", body = V1UploadResponse),
        (status = 400,
            description = "The multipart body is malformed, no `file` part was included, or an unknown \
                           `expiration` value was sent (must be one of `5m`, `30m`, `1h`, `3h`, `6h`, \
                           `12h`, `24h`). Inspect `error.message` for the specific reason.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://share.mingyu.dev/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `upload` scope (`error.code` will be `insufficient_scope`). \
                           Issue a new API key with the `upload` scope checked.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 413,
            description = "A single file exceeds the per-file size limit. \
                           `error.code` = `file_too_large`; \
                           `error.message` = `Per-file size limit exceeded.` \
                           Numeric limit at <https://share.mingyu.dev/api-terms-of-use>.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 429,
            description = "Per-API-key active storage quota exceeded — uploading this file would push \
                           the sum of file sizes of your unexpired shares over the limit. Delete some \
                           of your existing shares or wait for them to expire to reclaim quota. \
                           `error.code` = `storage_quota_exceeded`; \
                           `error.message` = `API key storage quota exceeded.` \
                           Numeric limit at <https://share.mingyu.dev/api-terms-of-use>.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn post_upload(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    multipart: Multipart,
) -> Result<PrettyJson<V1UploadResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Upload)?;

    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    let response = cli_upload(
        State(cli_state),
        Some(axum::extract::Extension(user.clone())),
        multipart,
    )
    .await
    .map_err(PublicApiError::from)?;
    Ok(PrettyJson(response.0.into()))
}

/// Initialize a multipart upload
///
/// Start a multipart upload session for one or more large files. The server reserves an upload
/// session, creates a share code, and returns presigned PUT URLs your client uses to write each
/// chunk directly to object storage — the file bytes never pass through this API server.
///
/// **Use case:** Files larger than ~100 MB where the single-shot `POST /v1/uploads` endpoint
/// would time out or exceed memory limits. Recommended for anything over 50 MB.
///
/// **Typical flow:**
/// 1. Call this endpoint with the file manifest (`file_name`, `file_size`, `content_type`) and
///    share options (`description`, `password`, `expiration`, `is_one_time`, `chunk_size`).
/// 2. `PUT` each chunk directly to its presigned URL. The presigned URLs expire in **1 hour**;
///    if your upload takes longer, request fresh URLs via
///    `POST /v1/uploads/multipart/{upload_session_id}/parts`.
/// 3. Collect the `ETag` header from each successful `PUT` response.
/// 4. Call `POST /v1/uploads/multipart/{upload_session_id}/complete` with the ETags to finalize.
///
/// **Behaviour notes:**
/// - `chunk_size` must match the actual byte length of every part except the last, which may be
///   smaller. Most S3-compatible stores require a minimum part size of 5 MiB (5 242 880 bytes).
/// - The share is **not publicly accessible** until `complete` is called successfully.
/// - An abandoned session (never completed) is cleaned up by a background job after 24 hours.
///
/// **Limits:** Single file size and per-API-key active storage limits apply. Exact values are
/// published at <https://share.mingyu.dev/api-terms-of-use>. Exceeding them returns `413` or
/// `429` respectively — error messages do not include the numeric limits.
///
/// **Required scope:** `upload`
#[utoipa::path(
    post,
    path = "/v1/uploads/multipart",
    tag = "uploads",
    request_body = CliMultipartInitRequest,
    responses(
        (status = 200,
            description = "Multipart upload session initialized. The `upload_session_id` must be \
                           passed to subsequent `/parts` and `/complete` calls. \
                           The `share_code` is reserved but not yet publicly accessible.",
            body = CliMultipartInitResponse),
        (status = 400,
            description = "The request body is malformed, an unknown `expiration` value was supplied, \
                           `chunk_size` is below the minimum (5 242 880 bytes), or the file list is empty. \
                           Inspect `error.message` for the specific reason.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://share.mingyu.dev/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "API key does not have the `upload` scope (`error.code` will be `insufficient_scope`). \
                           Issue a new API key with the `upload` scope checked.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 413,
            description = "A single file in the manifest exceeds the per-file size limit. \
                           `error.code` = `file_too_large`; \
                           `error.message` = `Per-file size limit exceeded.` \
                           Numeric limit at <https://share.mingyu.dev/api-terms-of-use>.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 429,
            description = "Per-API-key active storage quota exceeded — initialising this session would \
                           push the sum of file sizes of your unexpired shares over the limit. Delete \
                           some of your existing shares or wait for them to expire to reclaim quota. \
                           `error.code` = `storage_quota_exceeded`; \
                           `error.message` = `API key storage quota exceeded.` \
                           Numeric limit at <https://share.mingyu.dev/api-terms-of-use>.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn post_multipart_init(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    Json(req): Json<CliMultipartInitRequest>,
) -> Result<Json<CliMultipartInitResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Upload)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    cli_multipart_init(
        State(cli_state),
        Some(axum::extract::Extension(user.clone())),
        Json(req),
    )
    .await
    .map_err(PublicApiError::from)
}

/// Request presigned URLs for parts
///
/// Issue additional presigned PUT URLs for specific part numbers of an in-progress multipart upload.
///
/// **Use case:** Three situations call for this endpoint:
/// 1. The initial presigned URLs (from `POST /v1/uploads/multipart`) have **expired** (they are
///    valid for 1 hour) and you still have parts to upload.
/// 2. You requested only a **subset** of URLs during init to reduce memory usage and now need the
///    next batch.
/// 3. A previous `PUT` **failed** (e.g. network error) and you need a fresh URL to retry that
///    specific part number.
///
/// **Behaviour notes:**
/// - You may request any part numbers in any order; the server does not enforce sequential access.
/// - The returned URLs are also valid for **1 hour**. Plan accordingly for very large files.
/// - Each `PUT` must upload exactly `chunk_size` bytes, except the final part which may be smaller.
/// - The `ETag` from each successful `PUT` response must be collected and passed to `/complete`.
/// - Only the user who initialized the session may request URLs for it.
///
/// **Required scope:** `upload`
#[utoipa::path(
    post,
    path = "/v1/uploads/multipart/{upload_session_id}/parts",
    tag = "uploads",
    params(
        ("upload_session_id" = String, Path, description = "UUID of the upload session returned by `POST /v1/uploads/multipart`")
    ),
    request_body = CliPresignPartsRequest,
    responses(
        (status = 200,
            description = "Fresh presigned PUT URLs for the requested part numbers. \
                           URLs are valid for 1 hour. Collect the `ETag` from each successful `PUT` \
                           response to pass to the `/complete` endpoint.",
            body = CliPresignPartsResponse),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://share.mingyu.dev/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "Two distinct causes: \
                           (1) `error.code = insufficient_scope` — the API key does not have the `upload` scope. \
                           (2) `error.code = forbidden` — the session belongs to a different user.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 404,
            description = "The `upload_session_id` does not exist or has already been completed/expired.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn post_multipart_parts(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    axum::extract::Path(_session_id): axum::extract::Path<String>,
    Json(req): Json<CliPresignPartsRequest>,
) -> Result<Json<CliPresignPartsResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Upload)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    cli_presign_parts(
        State(cli_state),
        Some(axum::extract::Extension(user.clone())),
        Json(req),
    )
    .await
    .map_err(PublicApiError::from)
}

/// Complete a multipart upload
///
/// Finalize a multipart upload session by submitting the ETags collected from every `PUT` response.
///
/// **Use case:** Called after all parts have been successfully uploaded to object storage. This
/// call instructs the storage backend to assemble the parts into a single object, then publishes
/// the share so recipients can download it.
///
/// **Behaviour notes:**
/// - The `files` array must contain **every file** declared during init, in any order.
/// - The `parts` array for each file must contain **every part** that was uploaded, ordered by
///   `part_number` ascending. Missing or out-of-order parts cause the storage backend to reject
///   the request with `400`.
/// - ETags must be the **raw** value from the `ETag` response header, including surrounding double
///   quotes (e.g. `"d41d8cd98f00b204e9800998ecf8427e"`).
/// - On success the share is **immediately live** and publicly accessible.
/// - The response shape is identical to `POST /v1/uploads`, including the ready-to-paste
///   `curl_command` field.
/// - This call is idempotent for the same session ID only if the previous attempt failed before
///   the storage backend acknowledged. Calling it a second time after success returns `409`.
///
/// **Required scope:** `upload`
#[utoipa::path(
    post,
    path = "/v1/uploads/multipart/{upload_session_id}/complete",
    tag = "uploads",
    params(
        ("upload_session_id" = String, Path, description = "UUID of the upload session returned by `POST /v1/uploads/multipart`")
    ),
    request_body = CliCompleteMultipartRequest,
    responses(
        (status = 200,
            description = "Multipart upload finalized and share published. \
                           The `share_code` is now live and recipients can download immediately.",
            body = V1UploadResponse),
        (status = 400,
            description = "The request body is malformed, `files` or `parts` arrays are missing entries, \
                           ETags are malformed, or the storage backend rejected the assembly request. \
                           Inspect `error.message` for the specific reason.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 401,
            description = "API key is missing, malformed (must start with `sak_`), revoked, or expired. \
                           Issue a new API key at [Settings → API Keys](https://share.mingyu.dev/settings?tab=api-keys).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 403,
            description = "Two distinct causes: \
                           (1) `error.code = insufficient_scope` — the API key does not have the `upload` scope. \
                           (2) `error.code = forbidden` — the session belongs to a different user.",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 404,
            description = "The `upload_session_id` does not exist or has already expired (abandoned sessions \
                           are cleaned up after 24 hours).",
            body = crate::api::v1::error::PublicErrorEnvelope),
        (status = 409,
            description = "The session has already been completed successfully. \
                           Use the share code from the original response.",
            body = crate::api::v1::error::PublicErrorEnvelope),
    ),
    security(("api_key" = []))
)]
pub async fn post_multipart_complete(
    State(state): State<V1State>,
    token_user: Option<axum::extract::Extension<PersonalTokenUser>>,
    axum::extract::Path(_session_id): axum::extract::Path<String>,
    Json(req): Json<CliCompleteMultipartRequest>,
) -> Result<PrettyJson<V1UploadResponse>, PublicApiError> {
    let user = require_token(token_user.as_ref())?;
    require_scope(user, Scope::Upload)?;
    let cli_state = CliState {
        config: state.config.clone(),
        db: state.db.clone(),
        storage: state.storage.clone(),
    };
    let response = cli_complete_multipart(
        State(cli_state),
        Some(axum::extract::Extension(user.clone())),
        Json(req),
    )
    .await
    .map_err(PublicApiError::from)?;
    Ok(PrettyJson(response.0.into()))
}
