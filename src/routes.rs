use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post, delete},
    Router,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    config::Config,
    db::DbPool,
    docs::ApiDoc,
    handlers,
    middleware::auth::{optional_auth, require_auth, AuthState},
    middleware::rate_limiter::RateLimiter,
    services::StorageService,
};

pub fn create_router(
    config: Arc<Config>,
    db: DbPool,
    storage: StorageService,
) -> Router {
    let auth_state = AuthState {
        config: config.clone(),
    };

    let app_state = handlers::auth::AppState {
        config: config.clone(),
        db: db.clone(),
    };

    let upload_state = handlers::upload::UploadState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
    };

    let download_state = handlers::download::DownloadState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
    };

    let user_state = handlers::user::UserState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
    };

    let rate_limiter = RateLimiter::new();

    let cors = CorsLayer::new()
        .allow_origin(
            config
                .cors
                .allowed_origins
                .iter()
                .map(|s| s.parse().unwrap())
                .collect::<Vec<_>>(),
        )
        .allow_methods(tower_http::cors::AllowMethods::mirror_request())
        .allow_headers(tower_http::cors::AllowHeaders::mirror_request())
        .allow_credentials(true);

    let auth_routes = Router::new()
        .route("/auth/google", get(handlers::auth::google_login))
        .route("/auth/google/callback", get(handlers::auth::google_callback))
        .route("/auth/callback/google", get(handlers::auth::google_callback_handler))
        .route("/auth/naver", get(handlers::auth::naver_login))
        .route("/auth/naver/callback", get(handlers::auth::naver_callback))
        .route("/auth/callback/naver", get(handlers::auth::naver_callback_handler))
        .with_state(app_state);

    let upload_routes = Router::new()
        .route("/file/upload", post(handlers::upload::upload_file))
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            optional_auth,
        ))
        .with_state(upload_state);

    let download_routes = Router::new()
        .route("/download", get(handlers::download::download_file))
        .route("/download/file", get(handlers::download::download_single_file))
        .route("/download/bulk", post(handlers::download::download_multiple_files))
        .route("/preview/file", get(handlers::download::preview_file))
        .route("/files/list", get(handlers::download::get_file_list))
        .route("/file/info", get(handlers::download::get_file_info))
        .route("/file/verify-password", post(handlers::download::verify_password))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            crate::middleware::rate_limiter::rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            optional_auth,
        ))
        .with_state(download_state);

    let user_routes = Router::new()
        .route("/user/uploads", get(handlers::user::get_upload_history))
        .route("/user/uploads/:file_id/downloads", get(handlers::user::get_download_logs))
        .route("/user/uploads/:file_id", delete(handlers::user::delete_file_share))
        .layer(middleware::from_fn_with_state(auth_state, require_auth))
        .with_state(user_state);

    let health_route = Router::new().route("/health", get(|| async { "OK" }));

    let swagger_ui = SwaggerUi::new("/swagger-ui")
        .url("/api-docs/openapi.json", ApiDoc::openapi());

    Router::new()
        .merge(health_route)
        .merge(swagger_ui)
        .merge(auth_routes)
        .merge(upload_routes)
        .merge(download_routes)
        .merge(user_routes)
        .layer(cors)
}