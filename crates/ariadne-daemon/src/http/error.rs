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
            // The code already says "conflict"; the message must not repeat it
            // (`StoreError::Conflict`'s own Display prefix would).
            StoreError::Conflict(message) => Self::conflict(message.clone()),
            // Rust variant names are for the log; the envelope gets the
            // sentence a person can act on.
            StoreError::Transition(e) => {
                Self::new(StatusCode::CONFLICT, "illegal_transition", e.human())
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

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_core::state_machine::{Actor, TaskStatus, TransitionError};

    #[test]
    fn a_conflict_message_does_not_repeat_its_code() {
        let err = ApiError::from(StoreError::Conflict(
            "profile 01A is still used by 1 goal".into(),
        ));
        assert_eq!(err.status, StatusCode::CONFLICT);
        assert_eq!(err.body.error.code, "conflict");
        assert_eq!(
            err.body.error.message,
            "profile 01A is still used by 1 goal"
        );
    }

    /// The envelope carries the humanized transition message, never the
    /// PascalCase variant names of the state machine.
    #[test]
    fn a_refused_transition_is_humanized() {
        let err = ApiError::from(StoreError::Transition(TransitionError::Forbidden {
            from: TaskStatus::Pending,
            to: TaskStatus::Ready,
            actor: Actor::User,
        }));
        assert_eq!(err.status, StatusCode::CONFLICT);
        assert_eq!(err.body.error.code, "illegal_transition");
        assert_eq!(
            err.body.error.message,
            "only failed tasks can be retried (task is pending)"
        );
    }

    #[test]
    fn a_missing_entity_is_a_404_naming_the_id() {
        let err = ApiError::from(StoreError::NotFound {
            entity: "task",
            id: "badid123".into(),
        });
        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert_eq!(err.body.error.code, "not_found");
        assert_eq!(err.body.error.message, "task not found: badid123");
    }
}
