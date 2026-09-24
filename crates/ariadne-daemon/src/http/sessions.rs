//! Agent-session endpoints.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::sessions::{
    ResumeOutsideSessionRequest, SessionDto, SessionPageDto, SessionPageQuery,
};
use ariadne_core::models::agent_of;
use ariadne_store::SessionFilter;

use super::AppState;
use super::caller::call_ctx;
use super::convert::session_dto_of;
use super::error::{ApiError, ApiResult, Json};
use crate::acp_sessions::{Filter, QueryError};

/// Resume an outside conversation without creating a goal or task.
#[utoipa::path(post, path = "/v1/outside-sessions/resume", tag = "sessions",
    request_body = ResumeOutsideSessionRequest,
    responses((status = 200, body = SessionDto), (status = 403), (status = 404), (status = 409)))]
pub(super) async fn resume_outside(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ResumeOutsideSessionRequest>,
) -> ApiResult<Json<SessionDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if ctx.session.is_some() {
        return Err(ApiError::forbidden(
            "only the user may resume an outside session",
        ));
    }
    let _guard = state.outside_sessions.resumes.lock().await;
    for session in state.store.list_sessions(SessionFilter::default()).await? {
        if session.seat.is_none()
            && session.goal_id.is_none()
            && session.task_id.is_none()
            && agent_of(&session.model) == req.agent_id
            && session.internal_session_id.as_deref() == Some(req.internal_session_id.as_str())
        {
            let session = state
                .launcher
                .revive_session(&session.id, None)
                .await
                .map_err(|error| ApiError::conflict(error.to_string()))?;
            return Ok(Json(session_dto_of(&state.store, session).await?));
        }
    }
    let snapshot = state
        .outside_sessions
        .snapshot(&state.agent_registry, false)
        .await;
    let outside = snapshot
        .find(&state.store, &req.agent_id, &req.internal_session_id)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let Some(outside) = outside else {
        state
            .outside_sessions
            .snapshot(&state.agent_registry, true)
            .await;
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("outside session not found: {}", req.internal_session_id),
        ));
    };
    let session = state
        .launcher
        .resume_outside(&outside)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(Json(session_dto_of(&state.store, session).await?))
}

/// List sessions: one page of the sessions Ariadne runs and the
/// conversations the ACP agents stored themselves, newest activity first.
#[utoipa::path(get, path = "/v1/sessions", tag = "sessions",
    params(SessionPageQuery),
    responses((status = 200, body = SessionPageDto), (status = 400)))]
pub(super) async fn list(
    State(state): State<AppState>,
    Query(q): Query<SessionPageQuery>,
) -> ApiResult<Json<SessionPageDto>> {
    // Read the query before any agent is asked for a snapshot it would
    // not answer from.
    let filter = Filter::parse(&q).map_err(|error| match error {
        QueryError::InvalidCursor => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_cursor", error.to_string())
        }
        QueryError::InvalidFilter(_) => ApiError::bad_request(error.to_string()),
    })?;
    // A page no outside session can be in asks no agent for one.
    let snapshot = if filter.takes_outside() {
        state
            .outside_sessions
            .snapshot(&state.agent_registry, q.refresh.unwrap_or(false))
            .await
    } else {
        state.outside_sessions.cached().await
    };
    let page = crate::acp_sessions::page(&snapshot, &state.store, &filter)
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                error.to_string(),
            )
        })?;
    Ok(Json(page))
}

/// Inspect a session.
#[utoipa::path(get, path = "/v1/sessions/{id}", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = SessionDto), (status = 404)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<SessionDto>> {
    let session = state.store.get_session(&id).await?;
    Ok(Json(session_dto_of(&state.store, session).await?))
}

/// Revive an ended session: a new agent process, same agent conversation
/// (resumed via the stored internal session id). Returns the session to
/// attach to, which is this one either way — relaunched under its own id, or
/// untouched when its agent turned out to be alive already.
///
/// A missing worktree falls back to the repository checkout. A completed
/// goal stays completed while its session runs again.
#[utoipa::path(post, path = "/v1/sessions/{id}/resume", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = SessionDto), (status = 404), (status = 409)))]
pub(super) async fn resume(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<SessionDto>> {
    let session = state
        .launcher
        .revive_session(&id, None)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(Json(session_dto_of(&state.store, session).await?))
}

/// Kill a session's agent process.
#[utoipa::path(post, path = "/v1/sessions/{id}/kill", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = SessionDto), (status = 404)))]
pub(super) async fn kill(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<SessionDto>> {
    // An id that names no session is a 404, not a conflict: the store is asked
    // first so its "session not found: <id>" is what comes back.
    state.store.get_session(&id).await?;
    state
        .launcher
        .kill_session(&id)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    let session = state.store.get_session(&id).await?;
    Ok(Json(session_dto_of(&state.store, session).await?))
}

/// Body of the internal debug-spawn endpoint.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DebugSpawnRequest {
    pub seat: ariadne_core::Seat,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    /// Id of the reviewing task agent when seat = reviewer.
    pub agent_id: Option<String>,
}

/// Manually spawn an agent session (debug/testing path until the scheduler
/// drives spawns automatically). Not part of the public OpenAPI surface.
pub(super) async fn debug_spawn(
    State(state): State<AppState>,
    Json(req): Json<DebugSpawnRequest>,
) -> ApiResult<Json<SessionDto>> {
    use ariadne_core::Seat;
    let launcher = &state.launcher;
    let session = match req.seat {
        Seat::Orchestrator => {
            let goal = req
                .goal_id
                .ok_or_else(|| ApiError::bad_request("goal_id required"))?;
            launcher.spawn_orchestrator(&goal).await
        }
        Seat::Author => {
            let task = req
                .task_id
                .ok_or_else(|| ApiError::bad_request("task_id required"))?;
            launcher.spawn_author(&task).await
        }
        Seat::Reviewer => {
            let task = req
                .task_id
                .ok_or_else(|| ApiError::bad_request("task_id required"))?;
            let agent_id = req
                .agent_id
                .ok_or_else(|| ApiError::bad_request("agent_id required"))?;
            launcher.spawn_reviewer(&task, &agent_id).await
        }
    }
    .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(Json(session_dto_of(&state.store, session).await?))
}
