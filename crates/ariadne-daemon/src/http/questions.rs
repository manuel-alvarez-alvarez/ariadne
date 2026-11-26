//! The questions an agent asks the user with ACP `elicitation/create` (021):
//! the open ones of a session, and the answer that closes one.

use ariadne_api::sessions::{AnswerQuestionRequest, QuestionDto};
use axum::extract::{Path, State};
use axum::http::StatusCode;

use super::AppState;
use super::error::{ApiError, ApiResult, Json};
use crate::acp_questions::{Answered, answer_problem, result_of};

/// The questions the session's agent waits on, oldest first, each with the
/// form the agent sent, unchanged.
#[utoipa::path(get, path = "/v1/sessions/{id}/questions", tag = "sessions",
    operation_id = "list_questions",
    params(("id" = String, Path)),
    responses((status = 200, body = [QuestionDto]), (status = 404)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<QuestionDto>>> {
    state.store.get_session(&id).await?;
    Ok(Json(state.launcher.acp.open_questions(&id).await))
}

/// Answer one question: `accept` with the form content, `decline` or
/// `cancel`. The agent reads it as the result of its request. One answer
/// closes the question, and a second is refused.
#[utoipa::path(post, path = "/v1/sessions/{id}/questions/{question_id}/answer",
    tag = "sessions",
    operation_id = "answer_question",
    request_body = AnswerQuestionRequest,
    params(("id" = String, Path), ("question_id" = String, Path)),
    responses((status = 204, description = "The answer was sent to the agent"),
        (status = 400), (status = 404), (status = 409)))]
pub(crate) async fn answer(
    State(state): State<AppState>,
    Path((id, question_id)): Path<(String, String)>,
    Json(request): Json<AnswerQuestionRequest>,
) -> ApiResult<StatusCode> {
    take_answer(&state, &id, &question_id, &request).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The answer route's own work, for the console the daemon hosts too.
pub(super) async fn take_answer(
    state: &AppState,
    id: &str,
    question_id: &str,
    request: &AnswerQuestionRequest,
) -> ApiResult<()> {
    state.store.get_session(id).await?;
    if let Some(problem) = answer_problem(request) {
        return Err(ApiError::bad_request(problem));
    }
    let result = result_of(request.action, request.content.as_ref());
    match state.launcher.acp.answer_question(id, question_id, result) {
        Answered::Sent => Ok(()),
        Answered::Closed => Err(ApiError::new(
            StatusCode::CONFLICT,
            "question_closed",
            format!("question {question_id} is closed"),
        )),
        Answered::Unknown => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("session {id} has no open question {question_id}"),
        )),
    }
}
