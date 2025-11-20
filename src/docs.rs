use utoipa::OpenApi;
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::Modify;

use crate::models::{
    DownloadLog, DownloadLogResponse, ExpirationPeriod, FileShare, FileShareResponse,
    FileShareWithStats, MultipleFileUploadResponse, OAuthProvider, User, FileListResponse,
    FileInfoInGroup, DownloadFilesRequest,
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
        crate::handlers::upload::upload_file,
        crate::handlers::download::get_file_list,
        crate::handlers::download::download_file,
        crate::handlers::download::download_single_file,
        crate::handlers::download::download_multiple_files,
        crate::handlers::download::get_file_info,
        crate::handlers::download::verify_password,
        crate::handlers::user::get_upload_history,
        crate::handlers::user::get_download_logs,
        crate::handlers::user::delete_file_share,
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
            crate::handlers::auth::AuthResponse,
            crate::handlers::auth::UserResponse,
            crate::handlers::download::FileInfoResponse,
            crate::handlers::download::VerifyPasswordRequest,
            crate::handlers::user::UploadHistoryResponse,
        )
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Authentication endpoints (Google, Naver OAuth)"),
        (name = "upload", description = "File upload endpoints"),
        (name = "download", description = "File download endpoints"),
        (name = "user", description = "User-specific endpoints (requires authentication)"),
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
