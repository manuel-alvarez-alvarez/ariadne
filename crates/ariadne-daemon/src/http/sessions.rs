//! Agent-session endpoints.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::sessions::{SessionDto, SessionPageDto, SessionPageQuery, SwitchSessionRequest};

use super::AppState;
use super::caller::call_ctx;
use super::convert::session_dto_of;
use super::error::{ApiError, ApiResult, Json};
use crate::acp_sessions::{Filter, QueryError};

/// List Ariadne sessions, newest activity first.
#[utoipa::path(get, path = "/v1/sessions", tag = "sessions",
    params(SessionPageQuery),
    responses((status = 200, body = SessionPageDto), (status = 400)))]
pub(super) async fn list(
    State(state): State<AppState>,
    Query(q): Query<SessionPageQuery>,
) -> ApiResult<Json<SessionPageDto>> {
    let filter = Filter::parse(&q).map_err(|error| match error {
        QueryError::InvalidCursor => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_cursor", error.to_string())
        }
        QueryError::InvalidFilter(_) => ApiError::bad_request(error.to_string()),
    })?;
    let page = crate::acp_sessions::page(&state.store, &state.launcher.live, &filter)
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

/// Switch a session to another model or agent. A same-agent switch keeps the
/// session and conversation; another agent starts a new session on the seat.
///
/// A user may switch manually to a disabled manual-only model. An agent
/// session must switch only to a model automatic staffing can use. A session
/// of a cancelled goal is refused.
#[utoipa::path(post, path = "/v1/sessions/{id}/switch", tag = "sessions",
    operation_id = "switch_session",
    params(("id" = String, Path, description = "session id")),
    request_body = SwitchSessionRequest,
    responses((status = 200, body = SessionDto), (status = 400), (status = 404), (status = 409)))]
pub(super) async fn switch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<SwitchSessionRequest>,
) -> ApiResult<Json<SessionDto>> {
    let session = state.store.get_session(&id).await?;
    let ctx = call_ctx(&state.store, &headers).await?;
    let pin = if ctx.session.is_none() {
        super::pins::chosen(
            &state.store,
            &state.agent_registry,
            Some(&req.model),
            req.effort.as_deref(),
        )
        .await?
    } else {
        super::pins::staffed(
            &state.store,
            &state.agent_registry,
            Some(&req.model),
            req.effort.as_deref(),
        )
        .await?
    };
    if let Some(goal_id) = session.goal_id.as_deref()
        && state.store.get_goal(goal_id).await?.status() == ariadne_core::GoalStatus::Cancelled
    {
        return Err(ApiError::conflict(
            "cannot switch a session of a cancelled goal",
        ));
    }
    let switched = state
        .launcher
        .switch_session(&id, &pin.model, pin.effort.as_deref(), "requested")
        .await
        .map_err(|error| ApiError::conflict(format!("{error:#}")))?;
    Ok(Json(session_dto_of(&state.store, switched).await?))
}

/// Body of the internal debug-spawn endpoint.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DebugSpawnRequest {
    pub seat: ariadne_core::Seat,
    pub goal_id: Option<String>,
}

/// Manually spawn an orchestrator session (debug/testing path). Not part of
/// the public OpenAPI surface: the scheduler starts every column's agent,
/// and the pull request scheduler every review session.
pub(super) async fn debug_spawn(
    State(state): State<AppState>,
    Json(req): Json<DebugSpawnRequest>,
) -> ApiResult<Json<SessionDto>> {
    use ariadne_core::Seat;
    let session = match req.seat {
        Seat::Orchestrator => {
            let goal = req
                .goal_id
                .ok_or_else(|| ApiError::bad_request("goal_id required"))?;
            state.launcher.spawn_orchestrator(&goal).await
        }
        Seat::Agent | Seat::Reviewer | Seat::Keeper => {
            return Err(ApiError::conflict(
                "the scheduler starts the current column agent and every review or keeper session",
            ));
        }
    }
    .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(Json(session_dto_of(&state.store, session).await?))
}
