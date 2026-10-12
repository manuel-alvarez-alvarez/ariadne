//! An agent's flag that it waits for the user: `wait_for_user` (013).
//!
//! The question itself is in the agent's reply, where the console shows it
//! as the conversation it is. What comes here is the one-line hint the Needs
//! attention list shows (031), and nothing longer: a hint that tries to carry
//! the question is refused with the words that tell the agent where the
//! question goes.

use super::error::{ApiError, ApiResult, Json};
use super::{AppState, caller};
use ariadne_api::attention::CreateAgentRequest;
use ariadne_api::events::IngestEventRequest;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;

/// The most characters a hint may run to. A hint is a line of the Needs
/// attention list, read beside a dozen others; the question it stands for
/// is in the console.
pub(crate) const HINT_MAX_CHARS: usize = 100;

/// The event kind recorded in the session's own stream when a request
/// opens, so a console shows the wait where the question was asked.
pub(crate) const AGENT_REQUEST_EVENT: &str = "agent_request";

#[utoipa::path(post, path = "/v1/sessions/{id}/agent-requests", tag = "sessions", request_body = CreateAgentRequest, params(("id" = String, Path)), responses((status = 200)))]
pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<CreateAgentRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    scoped_session(&state, &headers, &id).await?;
    let hint = request.summary.trim();
    if hint.is_empty() {
        return Err(ApiError::bad_request("the hint cannot be empty"));
    }
    if hint.lines().count() > 1 || hint.chars().count() > HINT_MAX_CHARS {
        return Err(ApiError::bad_request(format!(
            "the hint is one line of at most {HINT_MAX_CHARS} characters, not the question: \
             write the question in your reply, then call `wait_for_user` with a short hint"
        )));
    }
    let (row, created) = state.store.create_agent_request(&id, hint).await?;
    // The wait is shown where the question was asked: one event in the
    // session's own stream, for the row this call opened — a repeat of a
    // pending hint is the same request, and adds no second line.
    if created {
        super::ingest_event(
            &state.store,
            &IngestEventRequest {
                session_id: id.clone(),
                launch: None,
                kind: AGENT_REQUEST_EVENT.into(),
                payload: serde_json::json!({"request_id": row.id, "hint": hint}),
            },
        )
        .await?;
    }
    Ok(Json(serde_json::json!({"id": row.id})))
}

#[utoipa::path(post, path = "/v1/sessions/{session_id}/agent-requests/{request_id}", tag = "sessions", params(("session_id" = String, Path), ("request_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn withdraw(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((session_id, request_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    scoped_session(&state, &headers, &session_id).await?;
    state
        .store
        .withdraw_agent_request(&request_id, &session_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn scoped_session(state: &AppState, headers: &HeaderMap, id: &str) -> ApiResult<()> {
    let context = caller::call_ctx(&state.store, headers).await?;
    if context.session.as_ref().map(|session| session.id.as_str()) != Some(id) {
        return Err(ApiError::forbidden(
            "agent requests belong to their own session",
        ));
    }
    Ok(())
}
