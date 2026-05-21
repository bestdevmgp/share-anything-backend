use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

use crate::models::AppError;

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicErrorBody {
    /// Machine-readable error code. Stable values: `bad_request`, `unauthorized`, `forbidden`,
    /// `insufficient_scope`, `not_found`, `conflict`, `gone`, `too_many_requests`, `internal_error`.
    #[schema(example = "insufficient_scope")]
    pub code: &'static str,
    /// Human-readable description of what went wrong and how to fix it.
    #[schema(example = "This endpoint requires the 'upload' scope.")]
    pub message: String,
    /// Opaque request correlation ID for support; may be `null`.
    #[schema(example = "01HXY1REQ1234567890ABCDEFG")]
    pub request_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PublicErrorEnvelope {
    pub error: PublicErrorBody,
}

#[derive(Debug, Error)]
pub enum PublicApiError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("gone: {0}")]
    Gone(String),
    #[error("rate limited")]
    TooManyRequests,
    #[error("insufficient scope: requires '{0}'")]
    InsufficientScope(&'static str),
    #[error("internal error")]
    Internal,
}

impl PublicApiError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::Gone(_) => (StatusCode::GONE, "gone"),
            Self::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            Self::InsufficientScope(_) => (StatusCode::FORBIDDEN, "insufficient_scope"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        }
    }

    fn message(&self) -> String {
        match self {
            Self::BadRequest(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::NotFound(m)
            | Self::Conflict(m)
            | Self::Gone(m) => m.clone(),
            Self::TooManyRequests => "Rate limit exceeded.".to_string(),
            Self::InsufficientScope(s) => {
                format!("This endpoint requires the '{}' scope.", s)
            }
            Self::Internal => "An internal error occurred.".to_string(),
        }
    }
}

impl IntoResponse for PublicApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let body = PublicErrorEnvelope {
            error: PublicErrorBody {
                code,
                message: self.message(),
                request_id: None,
            },
        };
        (status, Json(body)).into_response()
    }
}

impl From<AppError> for PublicApiError {
    fn from(e: AppError) -> Self {
        match e {
            AppError::BadRequest(_) => PublicApiError::BadRequest("Bad request.".into()),
            AppError::Unauthorized(_) => PublicApiError::Unauthorized("Authentication required.".into()),
            AppError::Forbidden(_) => PublicApiError::Forbidden("Forbidden.".into()),
            AppError::NotFound(_) => PublicApiError::NotFound("Resource not found.".into()),
            AppError::Conflict(_) => PublicApiError::Conflict("Conflict.".into()),
            AppError::Gone(_) => PublicApiError::Gone("Resource is no longer available.".into()),
            AppError::TooManyRequests(_) => PublicApiError::TooManyRequests,
            AppError::Internal(_) | AppError::Database(_) | AppError::Http(_) => {
                tracing::error!(error = ?e, "v1 internal error");
                PublicApiError::Internal
            }
        }
    }
}
