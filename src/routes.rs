use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post, put, delete},
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
    middleware::personal_token_auth::{cli_auth, CliAuthState},
    middleware::v1_auth::V1AuthState,
    middleware::rate_limiter::{RateLimiter, CliRateLimiter},
    services::{
        auth::AuthService, geolocation::GeolocationService,
        notification::NotificationService, StorageService,
        discord::DiscordNotifier, email::EmailService, signaling::SignalingState,
    },
};

pub fn create_router(
    config: Arc<Config>,
    db: DbPool,
    storage: StorageService,
    discord: Arc<DiscordNotifier>,
    email: Arc<EmailService>,
) -> Router {
    let auth_state = AuthState {
        config: config.clone(),
        db: db.clone(),
    };

    let notifications = NotificationService::new(db.clone(), email.clone());
    let geolocation = GeolocationService::new(config.ipinfo_token.clone());
    let auth_service = AuthService::new(
        db.clone(),
        config.clone(),
        discord.clone(),
        email.clone(),
        geolocation.clone(),
    );

    let app_state = handlers::auth::AppState {
        config: config.clone(),
        db: db.clone(),
        email: email.clone(),
        auth: auth_service.clone(),
    };

    let signaling_state = SignalingState::new();

    let upload_state = handlers::upload::UploadState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
        notifications: notifications.clone(),
    };

    let presigned_state = handlers::presigned::PresignedState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
        notifications: notifications.clone(),
    };

    let download_state = handlers::download::DownloadState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
        signaling: signaling_state.clone(),
        notifications: notifications.clone(),
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
        .route("/auth/email/send", post(handlers::auth::email_send))
        .route("/auth/email/verify", post(handlers::auth::email_verify))
        .route("/auth/email/verify-code", post(handlers::auth::email_verify_code))
        .route("/auth/email/status/:session_id", get(handlers::auth::email_status))
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
        .route("/user/quick-access/share/:file_id", post(handlers::quick_access::share_quick_access_file))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            require_auth,
        ))
        .with_state(quick_access_state);

    let user_routes = Router::new()
        .route("/user/uploads", get(handlers::user::get_upload_history).delete(handlers::user::delete_all_file_shares))
        .route("/user/uploads/:file_id/downloads", get(handlers::user::get_download_logs))
        .route("/user/uploads/:file_id", delete(handlers::user::delete_file_share))
        .route("/user/settings", get(handlers::user::get_notification_settings).put(handlers::user::update_notification_settings))
        .route("/user/name", put(handlers::user::update_name))
        .route("/user/account", delete(handlers::user::delete_account))
        .layer(middleware::from_fn_with_state(auth_state.clone(), require_auth))
        .with_state(user_state);

    let sessions_state = handlers::sessions::SessionsState { db: db.clone() };
    let sessions_routes = Router::new()
        .route(
            "/user/sessions",
            get(handlers::sessions::list_sessions)
                .delete(handlers::sessions::terminate_other_sessions),
        )
        .route(
            "/user/sessions/:jti",
            delete(handlers::sessions::terminate_session),
        )
        .route(
            "/user/trusted-devices",
            get(handlers::sessions::list_trusted_devices),
        )
        .route(
            "/user/trusted-devices/:id",
            delete(handlers::sessions::delete_trusted_device),
        )
        .layer(middleware::from_fn_with_state(auth_state.clone(), require_auth))
        .with_state(sessions_state);

    let device_confirm_state = handlers::device_confirm::DeviceConfirmState {
        config: config.clone(),
        db: db.clone(),
    };
    let device_confirm_routes = Router::new()
        .route(
            "/auth/device/revoke",
            get(handlers::device_confirm::revoke_device),
        )
        .with_state(device_confirm_state);

    let ws_routes = Router::new()
        .route("/ws/signaling", get(handlers::signaling::websocket_handler))
        .with_state((signaling_state.clone(), db.clone()));

    let p2p_routes = Router::new()
        .route("/p2p/status", get(handlers::p2p::check_uploader_status))
        .with_state(signaling_state.clone());

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

    let personal_token_state = handlers::personal_token::PersonalTokenState {
        db: db.clone(),
    };

    let personal_token_routes = Router::new()
        .route("/user/personal-tokens", post(handlers::personal_token::create_personal_token).get(handlers::personal_token::list_personal_tokens))
        .route("/user/personal-tokens/:token_id", delete(handlers::personal_token::delete_personal_token))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            require_auth,
        ))
        .with_state(personal_token_state);

    let api_key_state = handlers::api_key::ApiKeyState {
        db: db.clone(),
        discord: discord.clone(),
        email: email.clone(),
        frontend_url: config.server.frontend_url.clone(),
    };

    let api_key_routes = Router::new()
        .route("/user/api-keys/applications", post(handlers::api_key::apply).get(handlers::api_key::list_my_applications))
        .route("/user/api-keys/applications/:id", get(handlers::api_key::get_my_application).delete(handlers::api_key::cancel_application))
        .route("/user/api-keys", get(handlers::api_key::list_my_api_keys))
        .route("/user/api-keys/:id", delete(handlers::api_key::revoke_api_key))
        .route("/user/api-keys/reveal/:token", get(handlers::api_key::reveal_api_key))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            require_auth,
        ))
        .with_state(api_key_state);

    let admin_state = handlers::admin::AdminState {
        db: db.clone(),
        email: email.clone(),
        discord: discord.clone(),
    };

    let admin_routes = Router::new()
        .route("/admin/api-keys/applications", get(handlers::admin::admin_list_applications))
        .route("/admin/api-keys/applications/:id/approve", post(handlers::admin::admin_approve))
        .route("/admin/api-keys/applications/:id/reject", post(handlers::admin::admin_reject))
        .with_state(admin_state);

    let cli_auth_state = CliAuthState {
        db: db.clone(),
    };

    let cli_rate_limiter = CliRateLimiter::new();

    let cli_device_auth_state = handlers::cli_auth::CliAuthHandlerState {
        db: db.clone(),
        config: config.clone(),
    };

    let cli_device_auth_routes = Router::new()
        .route("/cli/auth/session", post(handlers::cli_auth::create_session))
        .route("/cli/auth/session/:session_id/status", get(handlers::cli_auth::check_status))
        .layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            crate::middleware::rate_limiter::rate_limit_middleware,
        ))
        .with_state(cli_device_auth_state.clone());

    let cli_device_auth_complete_routes = Router::new()
        .route("/cli/auth/session/:session_id/complete", post(handlers::cli_auth::complete_session))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            require_auth,
        ))
        .with_state(cli_device_auth_state);

    let cli_state = handlers::cli::CliState {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
    };

    let cli_special_routes = Router::new()
        .route("/cli/p2p/create", post(handlers::cli::cli_p2p_create))
        .route("/cli/download/:code/info", get(handlers::cli::cli_download_info))
        .route("/cli/me", get(handlers::cli::cli_me))
        .route("/cli/uploads", post(handlers::cli::cli_upload))
        .route("/cli/uploads/multipart", post(handlers::cli::cli_multipart_init))
        .route("/cli/uploads/multipart/:id/parts", post(handlers::cli::cli_presign_parts))
        .route("/cli/uploads/multipart/:id/complete", post(handlers::cli::cli_complete_multipart))
        .route("/cli/me/uploads", get(handlers::cli::cli_upload_history))
        .route("/cli/me/uploads/:code", delete(handlers::cli::cli_delete_upload))
        .route("/cli/me/uploads/:code/downloads", get(handlers::cli::cli_share_logs))
        .route("/cli/me/downloads", get(handlers::cli::cli_download_history))
        .route("/cli/shares/:code/download", get(handlers::cli::cli_download))
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            cli_rate_limiter.clone(),
            crate::middleware::rate_limiter::cli_rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            cli_auth_state.clone(),
            cli_auth,
        ))
        .with_state(cli_state);

    let v1_state = crate::api::v1::V1State {
        config: config.clone(),
        db: db.clone(),
        storage: storage.clone(),
        signaling: signaling_state.clone(),
    };
    let v1_auth_state = V1AuthState {
        db: db.clone(),
    };

    let v1_router = crate::api::v1::router(
        v1_state,
        v1_auth_state,
        cli_rate_limiter,
    );

    let install_route = Router::new()
        .route("/install", get(handlers::cli::cli_install_script));

    let health_route = Router::new().route("/health", get(|| async { "OK" }));

    let swagger_ui = SwaggerUi::new("/swagger-ui")
        .url("/api-docs/openapi.json", ApiDoc::openapi());

    Router::new()
        .merge(health_route)
        .merge(install_route)
        .merge(swagger_ui)
        .merge(auth_routes)
        .merge(upload_routes)
        .merge(p2p_upload_routes)
        .merge(presigned_routes)
        .merge(download_routes)
        .merge(user_routes)
        .merge(sessions_routes)
        .merge(device_confirm_routes)
        .merge(quick_access_routes)
        .merge(personal_token_routes)
        .merge(api_key_routes)
        .merge(admin_routes)
        .merge(cli_device_auth_routes)
        .merge(cli_device_auth_complete_routes)
        .merge(cli_special_routes)
        .merge(v1_router)
        .merge(ws_routes)
        .merge(p2p_routes)
        .merge(turn_routes)
        .merge(og_routes)
        .layer(axum::middleware::from_fn(
            crate::middleware::swagger_basic_auth::swagger_basic_auth,
        ))
        .layer(axum::middleware::from_fn(
            crate::middleware::discord_error::discord_error_middleware,
        ))
        .layer(axum::Extension(discord))
        .layer(cors)
}