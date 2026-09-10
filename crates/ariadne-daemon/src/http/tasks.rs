//! Task endpoints: CRUD and transitions. What a task being reviewed and
//! landed goes through is in [`super::landing`].

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::sessions::{AdoptOutsideSessionRequest, SessionDto};
use ariadne_api::tasks::{
    AgentAssignment, CreateTaskRequest, TaskDto, TaskListQuery, TaskTransitionDto,
    TransitionRequest, UpdateTaskRequest,
};
use ariadne_core::{Actor, MessageKind, Seat, TaskStatus};
use ariadne_store::{NewTask, NewTaskAgent, Store, Task, TaskFilter, TaskUpdate};

use super::AppState;
use super::caller::{CallCtx, call_ctx, ensure_task_scope};
use super::convert::{session_dto_of, task_dto_of, transition_dto};
use super::error::{ApiError, ApiResult, Json};
use super::landing;
use super::pins::{self, Repin, Standing};

/// The agents an assignment list asks for, in the order it names them.
///
/// An agent has nothing behind it to fall back to: every assignment names its
/// CLI and its model, and one that names no model is refused.
async fn resolve_agents(
    store: &Store,
    assignments: &[AgentAssignment],
) -> ApiResult<Vec<NewTaskAgent>> {
    let mut agents = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        agents.push(NewTaskAgent {
            seat: assignment.seat,
            skills: assignment.skills.clone(),
            pin: pins::chosen(store, Some(&assignment.model), assignment.effort.as_deref()).await?,
            brief: assignment.brief.clone(),
        });
    }
    Ok(agents)
}

/// The reviewers of an assignment list, refusing an author among them: the
/// author of a task is the one agent an edit cannot replace.
async fn resolve_reviewers(
    store: &Store,
    assignments: &[AgentAssignment],
) -> ApiResult<Vec<NewTaskAgent>> {
    if assignments.iter().any(|a| a.seat != Seat::Reviewer) {
        return Err(ApiError::bad_request(
            "only reviewers can be re-staffed; a task keeps the author it started with",
        ));
    }
    resolve_agents(store, assignments).await
}

/// Make an outside session the author of a ready task.
#[utoipa::path(post, path = "/v1/tasks/{id}/author-session", tag = "tasks",
    request_body = AdoptOutsideSessionRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = SessionDto), (status = 404), (status = 409)))]
pub async fn adopt_author_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<AdoptOutsideSessionRequest>,
) -> ApiResult<Json<SessionDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let available = crate::outside_sessions::discover(&state.store, &state.launcher.cfg.agent_home)
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                error.to_string(),
            )
        })?;
    if !available.iter().any(|session| {
        session.agent_kind == req.agent_kind
            && session.internal_session_id == req.internal_session_id
    }) {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("outside session not found: {}", req.internal_session_id),
        ));
    }
    let session = state
        .launcher
        .adopt_author(&id, req.agent_kind, &req.internal_session_id)
        .await
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    state.notify_scheduler(&id);
    Ok(Json(session_dto_of(&state.store, session).await?))
}

/// Create a task in a goal (orchestrator via MCP, or the user).
#[utoipa::path(post, path = "/v1/goals/{goal_id}/tasks", tag = "tasks",
    request_body = CreateTaskRequest,
    params(("goal_id" = String, Path, description = "goal id")),
    responses((status = 201, body = TaskDto), (status = 400), (status = 409)))]
pub async fn create(
    State(state): State<AppState>,
    Path(goal_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateTaskRequest>,
) -> ApiResult<(StatusCode, Json<TaskDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if !matches!(ctx.actor, Actor::Orchestrator | Actor::User) {
        return Err(ApiError::forbidden(
            "only the orchestrator or the user may create tasks",
        ));
    }
    if let Some(session) = &ctx.session
        && session.goal_id != goal_id
    {
        return Err(ApiError::forbidden("session belongs to a different goal"));
    }

    let goal = state.store.get_goal(&goal_id).await?;
    let repos = state.store.list_goal_repositories(&goal.id).await?;
    let repo_id = match &req.repo_id {
        Some(id) => {
            if !repos.iter().any(|r| &r.id == id) {
                return Err(ApiError::bad_request(format!(
                    "repo {id} does not belong to goal {goal_id}"
                )));
            }
            id.clone()
        }
        None if repos.len() == 1 => repos[0].id.clone(),
        None => {
            return Err(ApiError::bad_request(
                "goal has multiple repos; specify repo_id",
            ));
        }
    };

    let agents = resolve_agents(&state.store, &req.agents).await?;

    let task = state
        .store
        .create_task(NewTask {
            goal_id: goal.id,
            repo_id,
            title: req.title,
            description: req.description,
            agents,
            depends_on: req.depends_on,
            landing: req.landing,
        })
        .await?;
    let dto = task_dto_of(&state.store, task).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// List tasks.
#[utoipa::path(get, path = "/v1/tasks", tag = "tasks",
    params(TaskListQuery),
    responses((status = 200, body = [TaskDto])))]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<TaskListQuery>,
) -> ApiResult<Json<Vec<TaskDto>>> {
    let tasks = state
        .store
        .list_tasks(TaskFilter {
            goal_id: q.goal,
            status: q.status,
        })
        .await?;
    let mut out = Vec::with_capacity(tasks.len());
    for task in tasks {
        out.push(task_dto_of(&state.store, task).await?);
    }
    Ok(Json(out))
}

/// Inspect a task.
#[utoipa::path(get, path = "/v1/tasks/{id}", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 404)))]
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<TaskDto>> {
    let task = state.store.get_task(&id).await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

/// Edit a pending/ready task (orchestrator or user).
#[utoipa::path(patch, path = "/v1/tasks/{id}", tag = "tasks",
    request_body = UpdateTaskRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 404), (status = 409)))]
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<UpdateTaskRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if !matches!(ctx.actor, Actor::Orchestrator | Actor::User) {
        return Err(ApiError::forbidden(
            "only the orchestrator or the user may edit tasks",
        ));
    }
    let reviewers = match &req.reviewers {
        Some(assignments) => Some(resolve_reviewers(&state.store, assignments).await?),
        None => None,
    };
    // What the author is pinned to now: an effort written on its own is run at
    // that model, and moves without disturbing it.
    let author = state.store.task_author(&id).await?;
    let (pin, effort) = match pins::rechosen(
        &state.store,
        req.model.as_deref(),
        req.effort.as_deref(),
        Standing {
            agent_kind: author.agent_kind(),
            model: &author.model,
        },
    )
    .await?
    {
        Repin::Untouched => (None, None),
        Repin::To(pin) => (Some(pin), None),
        Repin::Effort(effort) => (None, Some(effort)),
    };
    let task = state
        .store
        .update_task(
            &id,
            TaskUpdate {
                title: req.title,
                description: req.description,
                pin,
                effort,
                reviewers,
                landing: req.landing,
            },
        )
        .await?;
    if let Some(deps) = req.depends_on {
        state.store.set_task_dependencies(&id, &deps).await?;
    }
    let task = state.store.get_task(&task.id).await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

/// Request a status transition. The actor is derived from the call context.
#[utoipa::path(post, path = "/v1/tasks/{id}/transitions", tag = "tasks",
    request_body = TransitionRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 404), (status = 409)))]
pub async fn transition(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<TransitionRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let task = apply_transition(&state, &ctx, &id, req).await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

pub(crate) async fn apply_transition(
    state: &AppState,
    ctx: &CallCtx,
    task_id: &str,
    req: TransitionRequest,
) -> ApiResult<Task> {
    // `finished` is never taken on faith.
    if req.to == TaskStatus::Finished {
        let task = state.store.get_task(task_id).await?;
        let repo = state.store.get_repository(&task.repo_id).await?;
        landing::verify_merged(state, &task, &repo, req.merge_commit.as_deref()).await?;
    }
    let task = state
        .store
        .transition_task(
            task_id,
            req.to,
            ctx.actor,
            req.reason.as_deref(),
            req.merge_commit.as_deref(),
        )
        .await?;
    // Asking for a review is the author writing to its reviewers, so the
    // channel carries it like everything else the agents say. One message per
    // reviewer, because one recipient each is what makes "has it seen this
    // yet" answerable at all.
    if req.to == TaskStatus::UnderReview {
        announce_review(state, ctx, &task, req.reason.as_deref()).await;
    }
    // A task going back to `ready` is a task starting over, and the only way
    // there is a retry of a failed one. Whatever it was published as is not
    // its request any more — a request closed unmerged is what fails a
    // published task in the first place — so the record goes with the retry
    // rather than pointing the user at something nobody will merge. A no-op
    // for the tasks that were never published, which is most of them.
    let task = match req.to == TaskStatus::Ready {
        true => {
            state.store.clear_task_pull_request(task_id).await?;
            state.store.get_task(task_id).await?
        }
        false => task,
    };
    state.notify_scheduler(task_id);
    Ok(task)
}

/// Cancel a task: the user's, or the orchestrator's, which holds the plan
/// the task belongs to.
///
/// Who called it is read from the session header rather than assumed, so the
/// transition log says which of the two it was — and so the state machine
/// refuses an author or a reviewer reaching for it.
#[utoipa::path(post, path = "/v1/tasks/{id}/cancel", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 403), (status = 409)))]
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let reason = format!("cancelled by {}", ctx.actor.as_str());
    let task = apply_transition(
        &state,
        &ctx,
        &id,
        TransitionRequest {
            to: TaskStatus::Cancelled,
            reason: Some(reason),
            merge_commit: None,
        },
    )
    .await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

/// Retry a failed task: failed -> ready. The user's call, and the
/// orchestrator's — the daemon wakes it when a task fails, and retrying is
/// one of the three answers it has.
#[utoipa::path(post, path = "/v1/tasks/{id}/retry", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 403), (status = 409)))]
pub async fn retry(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let reason = format!("retried by {}", ctx.actor.as_str());
    let task = apply_transition(
        &state,
        &ctx,
        &id,
        TransitionRequest {
            to: TaskStatus::Ready,
            reason: Some(reason),
            merge_commit: None,
        },
    )
    .await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

/// Transition audit log of a task.
#[utoipa::path(get, path = "/v1/tasks/{id}/transitions", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = [TaskTransitionDto])))]
pub async fn list_transitions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<TaskTransitionDto>>> {
    state.store.get_task(&id).await?;
    let rows = state.store.list_task_transitions(&id).await?;
    Ok(Json(rows.into_iter().map(transition_dto).collect()))
}

/// Tell every reviewer of `task` that there is a round to look at, carrying
/// the summary the author asked with.
///
/// Best effort: the round is open whether or not the channel took the news,
/// and a task that could not be announced is one the scheduler still starts
/// its reviewers for.
async fn announce_review(state: &AppState, ctx: &CallCtx, task: &Task, summary: Option<&str>) {
    let Ok(reviewers) = state.store.list_task_reviewers(&task.id).await else {
        return;
    };
    let body = summary
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("The author asks you to review this round.");
    for reviewer in reviewers {
        let sent = state
            .store
            .send_message(ariadne_store::NewMessage {
                goal_id: task.goal_id.clone(),
                task_id: Some(task.id.clone()),
                kind: MessageKind::ReviewRequest,
                from_actor: ctx.actor,
                from_agent_id: ctx.session.as_ref().and_then(|s| s.task_agent_id.clone()),
                from_session: ctx.session.as_ref().map(|s| s.id.clone()),
                to_actor: Actor::Reviewer,
                to_agent_id: Some(reviewer.id.clone()),
                body: body.to_string(),
            })
            .await;
        if let Err(e) = sent {
            tracing::warn!(task = %task.id, reviewer = %reviewer.id, error = %e, "announcing the review round failed");
        }
    }
}
