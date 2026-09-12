//! Agent-session endpoints.

use std::path::Path as FsPath;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::sessions::{
    AdoptOutsideSessionRequest, AdoptOutsideSessionResponse, OutsideSessionGoal,
    OutsideSessionListQuery, OutsideSessionPageDto, SessionDto, SessionListQuery,
};
use ariadne_core::models::agent_of;
use ariadne_core::{Actor, GoalStatus, Seat, TaskStatus};
use ariadne_store::{Goal, NewGoal, NewTask, Repository, SessionFilter};

use super::AppState;
use super::caller::call_ctx;
use super::convert::{goal_dto_of, session_dto_of, task_dto_of};
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

/// Create a task for one outside session, in a new goal or an active goal,
/// and adopt that session as its author before the scheduler can start one.
#[utoipa::path(post, path = "/v1/outside-sessions/adopt", tag = "sessions",
    request_body = AdoptOutsideSessionRequest,
    responses(
        (status = 201, body = AdoptOutsideSessionResponse),
        (status = 400),
        (status = 403),
        (status = 404),
        (status = 409)
    ))]
pub async fn adopt_outside(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AdoptOutsideSessionRequest>,
) -> ApiResult<(StatusCode, Json<AdoptOutsideSessionResponse>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if ctx.session.is_some() {
        return Err(ApiError::forbidden(
            "only the user may adopt an outside session",
        ));
    }

    let snapshot = state
        .outside_sessions
        .snapshot(&state.agent_registry, false)
        .await;
    let outside = snapshot
        .find(&state.store, &req.agent_id, &req.internal_session_id)
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                error.to_string(),
            )
        })?;
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

    let pending = match req.goal {
        OutsideSessionGoal::Existing(wanted) => {
            let goal = state.store.get_goal(&wanted.id).await?;
            if goal.status() != GoalStatus::Active {
                return Err(ApiError::conflict(format!(
                    "goal is {}, expected active",
                    goal.status
                )));
            }
            let repositories = state.store.list_goal_repositories(&goal.id).await?;
            let repository = repository_for_task(
                &repositories,
                req.repo_id.as_deref(),
                &outside.working_directory,
                Some(&goal.id),
            )?;
            PendingGoal::Existing {
                goal,
                repository: repository.clone(),
            }
        }
        OutsideSessionGoal::New(wanted) => {
            let repositories = match wanted.repository_ids {
                Some(ids) => {
                    if ids.is_empty() {
                        return Err(ApiError::bad_request("a goal needs at least one repo"));
                    }
                    let mut repositories = Vec::with_capacity(ids.len());
                    for id in ids {
                        let repository = state.store.get_repository(&id).await?;
                        if !repositories.iter().any(|known: &Repository| known.id == id) {
                            repositories.push(repository);
                        }
                    }
                    repositories
                }
                None => {
                    let repositories = state.store.list_repositories().await?;
                    let Some(repository) =
                        repository_containing(&repositories, &outside.working_directory)
                    else {
                        return Err(ApiError::conflict(format!(
                            "no registered repository contains the session working directory {}",
                            outside.working_directory
                        )));
                    };
                    vec![repository.clone()]
                }
            };
            let repository = repository_for_task(
                &repositories,
                req.repo_id.as_deref(),
                &outside.working_directory,
                None,
            )?
            .clone();
            PendingGoal::New {
                title: wanted.title,
                description: wanted.description.unwrap_or_default(),
                repositories,
                repository,
            }
        }
    };

    let Some(author) = req.agents.first() else {
        return Err(ApiError::bad_request("a task takes at least one author"));
    };
    if author.seat != Seat::Author
        || req
            .agents
            .iter()
            .skip(1)
            .any(|assignment| assignment.seat != Seat::Reviewer)
    {
        return Err(ApiError::bad_request(
            "staff one author first, then the reviewers",
        ));
    }
    let pinned = agent_of(&author.model);
    if pinned != req.agent_id {
        return Err(ApiError::conflict(format!(
            "outside session belongs to agent {}, but task author uses {pinned}",
            req.agent_id
        )));
    }
    let agents = super::tasks::resolve_agents(&state, &req.agents).await?;

    let title = req
        .title
        .unwrap_or_else(|| outside.first_prompt.chars().take(120).collect::<String>());
    if title.trim().is_empty() {
        return Err(ApiError::bad_request("a task needs a title"));
    }

    let (goal, repository) = match pending {
        PendingGoal::Existing { goal, repository } => (goal, repository),
        PendingGoal::New {
            title,
            description,
            repositories,
            repository,
        } => {
            let pin = agents
                .first()
                .map(|agent| agent.pin.clone())
                .expect("an author was checked above");
            let goal = state
                .store
                .create_adopted_goal(NewGoal {
                    title,
                    description,
                    repository_ids: repositories.into_iter().map(|repo| repo.id).collect(),
                    pin,
                })
                .await?;
            (goal, repository)
        }
    };
    let task = state
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repository.id,
            title,
            description: req.description,
            agents,
            depends_on: Vec::new(),
            landing: req.landing,
            permission_mode: req.permission_mode,
        })
        .await?;
    state
        .store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await?;
    let session = state
        .launcher
        .adopt_author(&task.id, &req.agent_id, &req.internal_session_id)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    if state.store.get_task(&task.id).await?.status() == TaskStatus::Ready {
        let advanced = state
            .store
            .transition_task(&task.id, TaskStatus::InProgress, Actor::Daemon, None, None)
            .await;
        if let Err(error) = advanced
            && state.store.get_task(&task.id).await?.status() != TaskStatus::InProgress
        {
            return Err(error.into());
        }
    }

    let goal = goal_dto_of(&state.store, state.store.get_goal(&goal.id).await?).await?;
    let task = task_dto_of(&state.store, state.store.get_task(&task.id).await?).await?;
    let session = session_dto_of(&state.store, session).await?;
    state.notify_scheduler(&task.id);
    Ok((
        StatusCode::CREATED,
        Json(AdoptOutsideSessionResponse {
            goal,
            task,
            session,
        }),
    ))
}

enum PendingGoal {
    Existing {
        goal: Goal,
        repository: Repository,
    },
    New {
        title: String,
        description: String,
        repositories: Vec<Repository>,
        repository: Repository,
    },
}

fn repository_for_task<'a>(
    repositories: &'a [Repository],
    repo_id: Option<&str>,
    working_directory: &str,
    goal_id: Option<&str>,
) -> ApiResult<&'a Repository> {
    if let Some(repo_id) = repo_id {
        // Adoption treats an unusable repository selection as the requested 409 conflict.
        return repositories
            .iter()
            .find(|repository| repository.id == repo_id)
            .ok_or_else(|| match goal_id {
                Some(goal_id) => {
                    ApiError::conflict(format!("repo {repo_id} does not belong to goal {goal_id}"))
                }
                None => {
                    ApiError::conflict(format!("repo {repo_id} does not belong to the new goal"))
                }
            });
    }
    if let [repository] = repositories {
        return Ok(repository);
    }
    repository_containing(repositories, working_directory).ok_or_else(|| {
        ApiError::conflict(format!(
            "the session working directory {working_directory} does not select a repository; specify repo_id"
        ))
    })
}

fn repository_containing<'a>(
    repositories: &'a [Repository],
    working_directory: &str,
) -> Option<&'a Repository> {
    let working_directory = FsPath::new(working_directory);
    repositories
        .iter()
        .filter(|repository| working_directory.starts_with(FsPath::new(&repository.path)))
        .max_by_key(|repository| FsPath::new(&repository.path).components().count())
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
