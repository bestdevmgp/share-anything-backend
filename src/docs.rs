use utoipa::OpenApi;
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::Modify;

use crate::models::{
    DownloadLog, DownloadLogResponse, ExpirationPeriod, FileShare, FileShareResponse,
    FileShareWithStats, MultipleFileUploadResponse, OAuthProvider, User, FileListResponse,
    FileInfoInGroup, DownloadFilesRequest, UploadHistoryResponse,
    FileInfoResponse, VerifyPasswordRequest, AuthResponse, UserResponse,
};

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
            )
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::handlers::auth::google_login,
        crate::handlers::auth::google_callback,
        crate::handlers::auth::google_callback_handler,
        crate::handlers::auth::naver_login,
        crate::handlers::auth::naver_callback,
        crate::handlers::auth::naver_callback_handler,
        crate::handlers::auth::kakao_login,
        crate::handlers::auth::kakao_callback,
        crate::handlers::auth::kakao_callback_handler,
        crate::handlers::auth::apple_login,
        crate::handlers::auth::apple_callback,
        crate::handlers::auth::apple_callback_handler,
        crate::handlers::upload::upload_file,
        crate::handlers::download::get_file_list,
        crate::handlers::download::download_file,
        crate::handlers::download::download_single_file,
        crate::handlers::download::preview_file,
        crate::handlers::download::download_multiple_files,
        crate::handlers::download::get_file_info,
        crate::handlers::download::verify_password,
        crate::handlers::user::get_upload_history,
        crate::handlers::user::get_download_logs,
        crate::handlers::user::delete_file_share,
        crate::handlers::user::delete_all_file_shares,
        crate::handlers::api_key::apply,
        crate::handlers::api_key::list_my_applications,
        crate::handlers::api_key::get_my_application,
        crate::handlers::api_key::list_my_api_keys,
        crate::handlers::api_key::revoke_api_key,
        crate::handlers::admin::admin_list_applications,
        crate::handlers::admin::admin_approve,
        crate::handlers::admin::admin_reject,
    ),
    components(
        schemas(
            User,
            OAuthProvider,
            FileShare,
            FileShareResponse,
            FileShareWithStats,
            MultipleFileUploadResponse,
            FileListResponse,
            FileInfoInGroup,
            DownloadFilesRequest,
            ExpirationPeriod,
            DownloadLog,
            DownloadLogResponse,
            AuthResponse,
            UserResponse,
            FileInfoResponse,
            VerifyPasswordRequest,
            UploadHistoryResponse,
            crate::models::api_key_application::ApplicationStatus,
            crate::models::api_key_application::ApiKeyApplication,
            crate::models::api_key_application::CreateApplicationRequest,
            crate::models::api_key_application::ApplicationResponse,
            crate::models::api_key_application::RejectRequest,
            crate::models::api_key_application::ApiKeyResponse,
            crate::models::personal_token::PersonalTokenResponse,
            crate::models::personal_token::Scope,
        )
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Authentication endpoints (Google, Naver, Apple, Kakao OAuth)"),
        (name = "upload", description = "File upload endpoints"),
        (name = "download", description = "File download endpoints"),
        (name = "user", description = "User-specific endpoints (requires authentication)"),
        (name = "api-keys", description = "User-facing API key application and key management"),
        (name = "admin", description = "Admin actions (requires X-Admin-Password header)"),
    ),
    info(
        title = "Share Anything API",
        version = "0.1.0",
        description = "File sharing service API with OAuth authentication, QR codes, and expiration management",
        contact(
            name = "API Support",
            email = "support@example.com"
        )
    )
)]
pub struct ApiDoc;
