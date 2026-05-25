use axum::{response::Html, Json};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::{Modify, OpenApi};

struct ApiKeySecurity;

impl Modify for ApiKeySecurity {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "api_key",
                SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                    "X-API-Key",
                    "API keys start with sak_ followed by 40 alphanumeric characters.",
                ))),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::v1::handlers::me::get_me,
        crate::api::v1::handlers::uploads::post_upload,
        crate::api::v1::handlers::uploads::post_multipart_init,
        crate::api::v1::handlers::uploads::post_multipart_parts,
        crate::api::v1::handlers::uploads::post_multipart_complete,
        crate::api::v1::handlers::shares::get_share,
        crate::api::v1::handlers::shares::get_share_download,
        crate::api::v1::handlers::history::list_my_uploads,
        crate::api::v1::handlers::history::delete_my_upload,
        crate::api::v1::handlers::history::list_share_downloads,
        crate::api::v1::handlers::history::list_my_downloads,
    ),
    components(
        schemas(
            crate::api::v1::error::PublicErrorBody,
            crate::api::v1::error::PublicErrorEnvelope,
            crate::models::personal_token::Scope,
            crate::api::v1::handlers::me::MeResponse,
            crate::api::v1::handlers::uploads::V1UploadResponse,
            crate::models::CliMultipartInitRequest,
            crate::models::CliMultipartFileInfo,
            crate::models::CliMultipartInitResponse,
            crate::models::CliMultipartFileInit,
            crate::models::CliPresignPartsRequest,
            crate::models::CliPresignPartsResponse,
            crate::models::CliPartUrl,
            crate::models::CliCompleteMultipartRequest,
            crate::models::CliCompleteFileInfo,
            crate::models::CliCompletedPart,
            crate::models::CliFileListResponse,
            crate::models::CliFileDetail,
            crate::api::v1::handlers::shares::DownloadQuery,
        )
    ),
    modifiers(&ApiKeySecurity),
    tags(
        (name = "me", description = "Authenticated principal"),
        (name = "uploads", description = "Create and manage uploads"),
        (name = "shares", description = "Inspect and download shares"),
        (name = "history", description = "Owner-side history"),
    ),
    info(
        title = "ShareAnything Public API",
        version = "1.1.0",
        description = "Programmatic access to ShareAnything. Authenticate with an API Key in the 'X-API-Key' header. Issue API keys at [Settings → API Keys](https://share.mingyu.dev/settings?tab=api-keys). Use of this API is governed by the [OpenAPI Terms of Use](https://share.mingyu.dev/api-terms-of-use).",
        contact(name = "ShareAnything", email = "shareanything@mingyu.dev"),
        license(name = "Proprietary"),
    ),
    servers(
        (url = "https://share-api.mingyu.dev", description = "Production"),
    )
)]
pub struct PublicApiDoc;

pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(PublicApiDoc::openapi())
}

const SCALAR_HTML: &str = r#"<!DOCTYPE html>
<html>
  <head>
    <title>ShareAnything API Reference</title>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <link rel="icon" type="image/svg+xml" href="https://share.mingyu.dev/favicon.svg" />
    <link rel="apple-touch-icon" href="https://share.mingyu.dev/apple-touch-icon.png" />
  </head>
  <body>
    <script
      id="api-reference"
      data-url="/v1/openapi.json"
      data-configuration='{"hiddenClients":[],"layout":"modern","defaultHttpClient":{"targetKey":"shell","clientKey":"curl"},"authentication":{"preferredSecurityScheme":"api_key"}}'
    ></script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
  </body>
</html>"#;

pub async fn scalar_html() -> Html<&'static str> {
    Html(SCALAR_HTML)
}
