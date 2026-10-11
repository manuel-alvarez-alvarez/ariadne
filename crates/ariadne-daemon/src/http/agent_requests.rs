//! Explicit human-answer requests from agent integrations.

use super::error::{ApiError, ApiResult, Json};
use super::{AppState, caller};
use ariadne_api::attention::CreateAgentRequest;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;

#[utoipa::path(post, path = "/v1/sessions/{id}/agent-requests", tag = "sessions", request_body = CreateAgentRequest, params(("id" = String, Path)), responses((status = 200)))]
pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<CreateAgentRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    scoped_session(&state, &headers, &id).await?;
    if request.summary.trim().is_empty() {
        return Err(ApiError::bad_request("request summary cannot be empty"));
    }
    let row = state
        .store
        .create_agent_request(&id, request.summary.trim())
        .await?;
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
