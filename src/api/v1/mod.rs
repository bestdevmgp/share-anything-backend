pub mod auth;
pub mod docs;
pub mod error;
pub mod handlers;

use axum::{extract::DefaultBodyLimit, http::{HeaderName, Method}, Router};
use tower_http::cors::{Any, CorsLayer};

use crate::{
    db::DbPool,
    middleware::personal_token_auth::CliAuthState,
    middleware::rate_limiter::CliRateLimiter,
    services::StorageService,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct V1State {
    pub config: Arc<crate::config::Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

pub fn router(
    state: V1State,
    cli_auth_state: CliAuthState,
    cli_rate_limiter: CliRateLimiter,
) -> Router {
    use axum::routing::{delete, get, post};
    use axum::middleware;
    use crate::middleware::personal_token_auth::cli_auth;
    use crate::middleware::rate_limiter::cli_rate_limit_middleware;

    // Spec §6.4: /v1/* allows open CORS because auth is header-based (PAT cannot be
    // exfiltrated via CSRF). This permissive CorsLayer is applied as the outermost layer
    // on the v1 sub-router so it handles OPTIONS preflights before the global strict CORS
    // at the top-level router has a chance to override them.
    // Expected: OPTIONS /v1/me with Origin: https://example.com → Access-Control-Allow-Origin: *
    let v1_cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("x-personal-token"),
            HeaderName::from_static("authorization"),
        ])
        .expose_headers([
            HeaderName::from_static("content-disposition"),
        ]);

    Router::new()
        .route("/v1/me", get(handlers::me::get_me))
        .route("/v1/uploads", post(handlers::uploads::post_upload))
        .route("/v1/uploads/multipart", post(handlers::uploads::post_multipart_init))
        .route("/v1/uploads/multipart/:id/parts", post(handlers::uploads::post_multipart_parts))
        .route("/v1/uploads/multipart/:id/complete", post(handlers::uploads::post_multipart_complete))
        .route("/v1/shares/:code", get(handlers::shares::get_share))
        .route("/v1/shares/:code/download", get(handlers::shares::get_share_download))
        .route("/v1/me/uploads", get(handlers::history::list_my_uploads))
        .route("/v1/me/uploads/:code", delete(handlers::history::delete_my_upload))
        .route("/v1/me/uploads/:code/downloads", get(handlers::history::list_share_downloads))
        .route("/v1/me/downloads", get(handlers::history::list_my_downloads))
        .route("/v1/openapi.json", get(docs::openapi_json))
        .route("/reference", get(docs::scalar_html))
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(cli_rate_limiter, cli_rate_limit_middleware))
        .layer(middleware::from_fn_with_state(cli_auth_state, cli_auth))
        .layer(v1_cors)
        .with_state(state)
}
