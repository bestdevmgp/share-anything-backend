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
            | Self::TooManyRequests(m)
            | Self::Internal(m) => m.clone(),
            Self::Database(_) => "데이터베이스 오류가 발생했습니다".to_string(),
            Self::Http(_) => "외부 요청 중 오류가 발생했습니다".to_string(),
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

impl From<StatusCode> for AppError {
    fn from(status: StatusCode) -> Self {
        match status {
            StatusCode::BAD_REQUEST => AppError::BadRequest("잘못된 요청입니다".into()),
            StatusCode::UNAUTHORIZED => AppError::Unauthorized("인증이 필요합니다".into()),
            StatusCode::FORBIDDEN => AppError::Forbidden("접근 권한이 없습니다".into()),
            StatusCode::NOT_FOUND => AppError::NotFound("찾을 수 없습니다".into()),
            StatusCode::CONFLICT => AppError::Conflict("충돌이 발생했습니다".into()),
            StatusCode::GONE => AppError::Gone("만료되었거나 더 이상 사용할 수 없습니다".into()),
            StatusCode::TOO_MANY_REQUESTS => {
                AppError::TooManyRequests("요청이 너무 많습니다".into())
            }
            _ => AppError::Internal(format!("서버 오류 ({})", status.as_u16())),
        }
    }
}
