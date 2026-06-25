pub mod auth;
pub mod code_samples;
pub mod docs;
pub mod error;
pub mod handlers;

use axum::{extract::DefaultBodyLimit, http::{HeaderName, Method}, Router};
use tower_http::cors::{Any, CorsLayer};

use crate::{
    db::DbPool,
    middleware::v1_auth::V1AuthState,
    middleware::rate_limiter::CliRateLimiter,
    services::{signaling::SignalingState, StorageService},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct V1State {
    pub config: Arc<crate::config::Config>,
    pub db: DbPool,
    pub storage: StorageService,
    pub signaling: SignalingState,
}

pub fn router(
    state: V1State,
    v1_auth_state: V1AuthState,
    cli_rate_limiter: CliRateLimiter,
    rate_limiter: crate::middleware::rate_limiter::RateLimiter,
) -> Router {
    use axum::routing::{delete, get, post};
    use axum::middleware;
    use crate::middleware::v1_auth::v1_auth;
    use crate::middleware::rate_limiter::cli_rate_limit_middleware;

    let v1_cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("x-api-key"),
            HeaderName::from_static("authorization"),
        ])
        .expose_headers([
            HeaderName::from_static("content-disposition"),
        ]);

    let auth_protected = Router::new()
        .route("/v1/me", get(handlers::me::get_me))
        .route("/v1/uploads", post(handlers::uploads::post_upload))
        .route("/v1/uploads/multipart", post(handlers::uploads::post_multipart_init))
        .route("/v1/uploads/multipart/:id/parts", post(handlers::uploads::post_multipart_parts))
        .route("/v1/uploads/multipart/:id/complete", post(handlers::uploads::post_multipart_complete))
        .route("/v1/shares/:code", get(handlers::shares::get_share))
        .route("/v1/shares/:code/download", get(handlers::shares::get_share_download))
        .route("/v1/shares/:code/download-url", post(handlers::shares::post_share_download_url))
        .route("/v1/shares/:code/download-complete", post(handlers::shares::post_share_download_complete))
        .route("/v1/me/uploads", get(handlers::history::list_my_uploads))
        .route("/v1/me/uploads/:code", delete(handlers::history::delete_my_upload))
        .route("/v1/me/uploads/:code/downloads", get(handlers::history::list_share_downloads))
        .route("/v1/me/downloads", get(handlers::history::list_my_downloads))
        .route("/v1/p2p/sessions", post(handlers::p2p::post_p2p_session))
        .route(
            "/v1/p2p/sessions/:code/status",
            get(handlers::p2p::get_p2p_status),
        )
        .route(
            "/v1/turn/credentials",
            get(handlers::p2p::get_v1_turn_credentials),
        )
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            (cli_rate_limiter, rate_limiter),
            cli_rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(v1_auth_state, v1_auth))
        .with_state(state.clone());

    let ws_router = Router::new()
        .route("/v1/ws/signaling", get(handlers::p2p::signaling_ws))
        .with_state(state);

    let docs_router = Router::new()
        .route("/v1/openapi.json", get(docs::openapi_json))
        .route("/reference", get(docs::scalar_html));

    let public_router = Router::new().route(
        "/v1/health",
        get(|| async { axum::Json(serde_json::json!({ "status": "healthy" })) }),
    );

    auth_protected
        .merge(ws_router)
        .merge(docs_router)
        .merge(public_router)
        .layer(v1_cors)
}
