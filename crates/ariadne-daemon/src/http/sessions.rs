//! Agent-session endpoints.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use ariadne_api::sessions::{
    OutsideSessionListQuery, OutsideSessionPageDto, SessionDto, SessionListQuery,
};
use ariadne_store::SessionFilter;

use super::AppState;
use super::convert::session_dto_of;
use super::error::{ApiError, ApiResult, Json};
use crate::acp_sessions::{Filter, QueryError};

/// List sessions Ariadne did not start: one filtered page of the daemon's
/// snapshot of every ACP agent's stored sessions, newest first.
#[utoipa::path(get, path = "/v1/outside-sessions", tag = "sessions",
    params(OutsideSessionListQuery),
    responses((status = 200, body = OutsideSessionPageDto), (status = 400)))]
pub async fn list_outside(
    State(state): State<AppState>,
    Query(q): Query<OutsideSessionListQuery>,
) -> ApiResult<Json<OutsideSessionPageDto>> {
    // Read the query before any agent is asked for a snapshot it would
    // not answer from.
    let filter = Filter::parse(&q).map_err(|error| match error {
        QueryError::InvalidCursor => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_cursor", error.to_string())
        }
        QueryError::InvalidFilter(_) => ApiError::bad_request(error.to_string()),
    })?;
    let snapshot = state
        .outside_sessions
        .snapshot(&state.agent_registry, q.refresh.unwrap_or(false))
        .await;
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

/// List agent sessions.
#[utoipa::path(get, path = "/v1/sessions", tag = "sessions",
    params(SessionListQuery),
    responses((status = 200, body = [SessionDto])))]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<SessionListQuery>,
) -> ApiResult<Json<Vec<SessionDto>>> {
    let sessions = state
        .store
        .list_sessions(SessionFilter {
            goal_id: q.goal,
            task_id: q.task,
            status: q.status,
            live_only: false,
            attention_only: q.attention.unwrap_or(false),
        })
        .await?;
    let mut out = Vec::with_capacity(sessions.len());
    for session in sessions {
        out.push(session_dto_of(&state.store, session).await?);
    }
    Ok(Json(out))
}

/// Inspect a session.
#[utoipa::path(get, path = "/v1/sessions/{id}", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = SessionDto), (status = 404)))]
pub async fn get(
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
/// `409` when there is nothing to come back to: no stored agent id, a
/// worktree that was cleaned up — or a goal that has finished, whose live
/// sessions the scheduler takes down anyway.
#[utoipa::path(post, path = "/v1/sessions/{id}/resume", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = SessionDto), (status = 404), (status = 409)))]
pub async fn resume(
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
pub async fn kill(
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
pub struct DebugSpawnRequest {
    pub seat: ariadne_core::Seat,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    /// Id of the reviewing task agent when seat = reviewer.
    pub agent_id: Option<String>,
}

/// Manually spawn an agent session (debug/testing path until the scheduler
/// drives spawns automatically). Not part of the public OpenAPI surface.
pub async fn debug_spawn(
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
