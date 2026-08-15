//! HTTP error mapping onto the uniform `{"error": {...}}` envelope.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use ariadne_api::error::ErrorBody;
use ariadne_store::StoreError;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: ErrorBody,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: ErrorBody::new(code, message),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match &e {
            StoreError::NotFound { .. } => {
                Self::new(StatusCode::NOT_FOUND, "not_found", e.to_string())
            }
            StoreError::Conflict(_) => Self::conflict(e.to_string()),
            StoreError::Transition(_) => {
                Self::new(StatusCode::CONFLICT, "illegal_transition", e.to_string())
            }
            StoreError::Invalid(_) => Self::bad_request(e.to_string()),
            StoreError::Db(inner) => {
                tracing::error!(error = %inner, "database error");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal database error",
                )
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
