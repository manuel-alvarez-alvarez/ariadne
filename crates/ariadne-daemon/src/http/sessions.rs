//! Agent-session endpoints.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use ariadne_api::sessions::{
    SessionDto, SessionInputRequest, SessionListQuery, SessionLogsResponse,
};
use ariadne_store::SessionFilter;

use super::AppState;
use super::convert::session_dto;
use super::error::{ApiError, ApiResult};

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
    Ok(Json(sessions.into_iter().map(session_dto).collect()))
}

/// Inspect a session.
#[utoipa::path(get, path = "/v1/sessions/{id}", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = SessionDto), (status = 404)))]
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<SessionDto>> {
    Ok(Json(session_dto(state.store.get_session(&id).await?)))
}

/// Revive an ended session: new tmux, same agent conversation (resumed via
/// the stored internal session id). Returns the session to attach to, which
/// is this one either way — relaunched under its own id and tmux name, or
/// untouched when its tmux turned out to be alive already.
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
    Ok(Json(session_dto(session)))
}

/// Kill a session's tmux process.
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
    Ok(Json(session_dto(state.store.get_session(&id).await?)))
}

/// Type into a session's pane: the write counterpart of the log stream.
///
/// The bytes go to tmux verbatim, so the agent sees exactly what was typed in
/// front of it and the echo comes back through `/logs/stream` like any other
/// pane output. Nothing is appended — a submit carries its own `\r`.
///
/// Both halves of "live" are checked, as in `logs_stream`: the row's status,
/// because a finished session must not be typed into, and tmux itself,
/// because tmux names are reused and a `send-keys` at a stale name would land
/// in a successor's pane.
#[utoipa::path(post, path = "/v1/sessions/{id}/input", tag = "sessions",
    request_body = SessionInputRequest,
    params(("id" = String, Path, description = "session id")),
    responses((status = 204, description = "Input handed to the pane"),
        (status = 404), (status = 409)))]
pub async fn input(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SessionInputRequest>,
) -> ApiResult<StatusCode> {
    let session = state.store.get_session(&id).await?;
    if !session.status().is_live() {
        return Err(ApiError::conflict(format!(
            "session {id} is {} and cannot take input",
            session.status
        )));
    }
    if !state.launcher.tmux.has_session(&session.tmux_session).await {
        return Err(ApiError::conflict(format!(
            "session {id} has no live pane ({})",
            session.tmux_session
        )));
    }
    state
        .launcher
        .tmux
        .send_raw(&session.tmux_session, req.data.as_bytes())
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Recent tmux pane output of a session.
#[utoipa::path(get, path = "/v1/sessions/{id}/logs", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = SessionLogsResponse), (status = 404), (status = 409)))]
pub async fn logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<SessionLogsResponse>> {
    let session = state.store.get_session(&id).await?;
    let logs = if state.launcher.tmux.has_session(&session.tmux_session).await {
        state
            .launcher
            .tmux
            .capture_pane(&session.tmux_session, 1000)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?
    } else {
        // Fall back to the piped console log of a finished session.
        std::fs::read_to_string(
            state
                .launcher
                .cfg
                .run_dir
                .join(&session.id)
                .join("console.log"),
        )
        .unwrap_or_default()
    };
    Ok(Json(SessionLogsResponse {
        session_id: session.id,
        tmux_session: session.tmux_session,
        logs,
    }))
}

/// Body of the internal debug-spawn endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct DebugSpawnRequest {
    pub role: ariadne_core::Role,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    /// Reviewer profile (id or name) when role = reviewer.
    pub profile: Option<String>,
}

/// Manually spawn an agent session (debug/testing path until the scheduler
/// drives spawns automatically). Not part of the public OpenAPI surface.
pub async fn debug_spawn(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<DebugSpawnRequest>,
) -> ApiResult<Json<SessionDto>> {
    use ariadne_core::Role;
    let launcher = &state.launcher;
    let session = match req.role {
        Role::Planner => {
            let goal = req
                .goal_id
                .ok_or_else(|| ApiError::bad_request("goal_id required"))?;
            launcher.spawn_planner(&goal).await
        }
        Role::Engineer => {
            let task = req
                .task_id
                .ok_or_else(|| ApiError::bad_request("task_id required"))?;
            launcher.spawn_engineer(&task).await
        }
        Role::Reviewer => {
            let task = req
                .task_id
                .ok_or_else(|| ApiError::bad_request("task_id required"))?;
            let spec = req
                .profile
                .ok_or_else(|| ApiError::bad_request("profile required"))?;
            let profile = state.store.resolve_profile(&spec).await?;
            launcher.spawn_reviewer(&task, &profile.id).await
        }
    }
    .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(Json(session_dto(session)))
}
