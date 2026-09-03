//! HTTP error mapping onto the uniform `{"error": {...}}` envelope, and the
//! JSON body extractor that answers in it.

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;

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
        (self.status, axum::Json(self.body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// The JSON body of a request, and the JSON body of a reply: `axum::Json`
/// with its rejection folded into the envelope every other refusal uses.
///
/// Every request DTO is `#[serde(deny_unknown_fields)]`, so a body carrying a
/// field the DTO does not declare is refused here rather than dropped in
/// silence. axum answers such a body with a plain-text `422`, which no client
/// can branch on; this one answers with `invalid_request` and serde's own
/// sentence, which names the field. The status axum chose is kept.
pub struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(body)) => Ok(Self(body)),
            Err(rejection) => Err(refused(&rejection)),
        }
    }
}

impl<T: Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// The refusal a rejected body earns: the status axum chose, and serde's own
/// sentence rather than axum's wrapper around it, which names Rust types the
/// sender cannot see. A rejection with nothing under it — a missing content
/// type — keeps its own words.
fn refused(rejection: &JsonRejection) -> ApiError {
    let message = std::error::Error::source(rejection)
        .map_or_else(|| rejection.body_text(), |cause| cause.to_string());
    ApiError::new(rejection.status(), "invalid_request", message)
}

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
