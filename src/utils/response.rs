use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub struct PrettyJson<T>(pub T);

impl<T: Serialize> IntoResponse for PrettyJson<T> {
    fn into_response(self) -> Response {
        match serde_json::to_string_pretty(&self.0) {
            Ok(body) => Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}
