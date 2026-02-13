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
    services::{StorageService, signaling::SignalingState},
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

    let signaling_state = SignalingState::new();

    let upload_state = handlers::upload::UploadState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
    };

    let presigned_state = handlers::presigned::PresignedState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
    };

    let download_state = handlers::download::DownloadState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
        signaling: signaling_state.clone(),
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
        .route("/auth/kakao", get(handlers::auth::kakao_login))
        .route("/auth/kakao/callback", get(handlers::auth::kakao_callback))
        .route("/auth/callback/kakao", get(handlers::auth::kakao_callback_handler))
        .route("/auth/apple", get(handlers::auth::apple_login))
        .route("/auth/apple/callback", post(handlers::auth::apple_callback))
        .route("/auth/callback/apple", get(handlers::auth::apple_callback_handler))
        .with_state(app_state);

    let upload_routes = Router::new()
        .route("/file/upload", post(handlers::upload::upload_file))
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            optional_auth,
        ))
        .with_state(upload_state.clone());

    let p2p_upload_routes = Router::new()
        .route("/file/p2p/create", post(handlers::upload::create_p2p_session))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            optional_auth,
        ))
        .with_state(upload_state);

    let presigned_routes = Router::new()
        .route("/file/presign", post(handlers::presigned::request_presigned_upload))
        .route("/file/complete", post(handlers::presigned::complete_presigned_upload))
        .route("/file/multipart/init", post(handlers::presigned::init_multipart_upload))
        .route("/file/multipart/presign-parts", post(handlers::presigned::get_part_presigned_urls))
        .route("/file/multipart/complete", post(handlers::presigned::complete_multipart_upload))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            optional_auth,
        ))
        .with_state(presigned_state);

    let download_routes = Router::new()
        .route("/download", get(handlers::download::download_file))
        .route("/download/file", get(handlers::download::download_single_file))
        .route("/download/url", get(handlers::download::get_download_url))
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

    let quick_access_state = handlers::quick_access::QuickAccessState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
    };

    let quick_access_routes = Router::new()
        .route("/user/quick-access/init", post(handlers::quick_access::init_quick_access_upload))
        .route("/user/quick-access", get(handlers::quick_access::list_quick_access_files))
        .route("/user/quick-access/:file_id", delete(handlers::quick_access::delete_quick_access_file))
        .route("/user/quick-access/preview/:file_id", get(handlers::quick_access::preview_quick_access_file))
        .route("/user/quick-access/download/:file_id", get(handlers::quick_access::download_quick_access_file))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            require_auth,
        ))
        .with_state(quick_access_state);

    let user_routes = Router::new()
        .route("/user/uploads", get(handlers::user::get_upload_history))
        .route("/user/uploads/:file_id/downloads", get(handlers::user::get_download_logs))
        .route("/user/uploads/:file_id", delete(handlers::user::delete_file_share))
        .layer(middleware::from_fn_with_state(auth_state, require_auth))
        .with_state(user_state);

    let ws_routes = Router::new()
        .route("/ws/signaling", get(handlers::signaling::websocket_handler))
        .with_state((signaling_state.clone(), db.clone()));

    let p2p_routes = Router::new()
        .route("/p2p/status", get(handlers::p2p::check_uploader_status))
        .with_state(signaling_state);

    let turn_state = handlers::turn::TurnState {
        config: config.clone(),
    };

    let turn_routes = Router::new()
        .route("/turn/credentials", get(handlers::turn::get_turn_credentials))
        .with_state(turn_state);

    let og_state = handlers::og::OgState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
    };

    let og_routes = Router::new()
        .route("/og/:code", get(handlers::og::get_og_page))
        .route("/og/:code/image", get(handlers::og::get_og_image))
        .with_state(og_state);

    let health_route = Router::new().route("/health", get(|| async { "OK" }));

    let swagger_ui = SwaggerUi::new("/swagger-ui")
        .url("/api-docs/openapi.json", ApiDoc::openapi());

    Router::new()
        .merge(health_route)
        .merge(swagger_ui)
        .merge(auth_routes)
        .merge(upload_routes)
        .merge(p2p_upload_routes)
        .merge(presigned_routes)
        .merge(download_routes)
        .merge(user_routes)
        .merge(quick_access_routes)
        .merge(ws_routes)
        .merge(p2p_routes)
        .merge(turn_routes)
        .merge(og_routes)
        .layer(cors)
}