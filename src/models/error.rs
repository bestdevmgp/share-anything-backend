use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

impl ErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Gone(String),
    #[error("{0}")]
    PayloadTooLarge(String),
    #[error("{0}")]
    StorageQuotaExceeded(String),
    #[error("{0}")]
    DailyQuotaExceeded(String),
    #[error("{0}")]
    TooManyRequests(String),
    #[error("{0}")]
    Internal(String),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

impl AppError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "BAD_REQUEST"),
            Self::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "CONFLICT"),
            Self::Gone(_) => (StatusCode::GONE, "GONE"),
            Self::PayloadTooLarge(_) => (StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE"),
            Self::StorageQuotaExceeded(_) => (StatusCode::TOO_MANY_REQUESTS, "STORAGE_QUOTA_EXCEEDED"),
            Self::DailyQuotaExceeded(_) => (StatusCode::TOO_MANY_REQUESTS, "DAILY_QUOTA_EXCEEDED"),
            Self::TooManyRequests(_) => (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMIT_EXCEEDED"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_SERVER_ERROR"),
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR"),
            Self::Http(_) => (StatusCode::INTERNAL_SERVER_ERROR, "HTTP_ERROR"),
        }
    }

    fn public_message(&self) -> String {
        match self {
            Self::BadRequest(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::NotFound(m)
            | Self::Conflict(m)
            | Self::Gone(m)
            | Self::PayloadTooLarge(m)
            | Self::StorageQuotaExceeded(m)
            | Self::DailyQuotaExceeded(m)
            | Self::TooManyRequests(m) => m.clone(),
            Self::Internal(_) => "An internal error occurred.".to_string(),
            Self::Database(_) => "An internal database error occurred.".to_string(),
            Self::Http(_) => "An external request failed.".to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let message = self.public_message();

        if status.is_server_error() {
            tracing::error!(error_code = code, error = %self, "request failed");
        }

        (status, Json(ErrorResponse::new(code, message))).into_response()
    }
}

pub fn bad_request(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}

pub fn unauthorized(message: impl Into<String>) -> AppError {
    AppError::Unauthorized(message.into())
}

pub fn forbidden(message: impl Into<String>) -> AppError {
    AppError::Forbidden(message.into())
}

pub fn not_found(message: impl Into<String>) -> AppError {
    AppError::NotFound(message.into())
}

pub fn internal_error(message: impl Into<String>) -> AppError {
    AppError::Internal(message.into())
}

pub fn payload_too_large(message: impl Into<String>) -> AppError {
    AppError::PayloadTooLarge(message.into())
}

pub fn storage_quota_exceeded(message: impl Into<String>) -> AppError {
    AppError::StorageQuotaExceeded(message.into())
}

pub fn daily_quota_exceeded(message: impl Into<String>) -> AppError {
    AppError::DailyQuotaExceeded(message.into())
}

impl From<StatusCode> for AppError {
    fn from(status: StatusCode) -> Self {
        match status {
            StatusCode::BAD_REQUEST => AppError::BadRequest("Bad request".into()),
            StatusCode::UNAUTHORIZED => AppError::Unauthorized("Authentication required".into()),
            StatusCode::FORBIDDEN => AppError::Forbidden("Forbidden".into()),
            StatusCode::NOT_FOUND => AppError::NotFound("Not found".into()),
            StatusCode::CONFLICT => AppError::Conflict("Conflict".into()),
            StatusCode::GONE => AppError::Gone("Gone".into()),
            StatusCode::TOO_MANY_REQUESTS => {
                AppError::TooManyRequests("Too many requests".into())
            }
            _ => AppError::Internal(format!("Server error ({})", status.as_u16())),
        }
    }
}
