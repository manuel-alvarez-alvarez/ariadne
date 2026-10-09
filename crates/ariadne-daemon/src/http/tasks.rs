//! Task endpoints: CRUD and transitions. What a task's agents say to each
//! other and the request its `pr` column opens are in [`super::channel`];
//! the two step calls that move it through its columns are in
//! [`super::steps`].

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::tasks::{
    AgentAssignment, CreateTaskRequest, TaskDto, TaskListQuery, TaskTransitionDto,
    TransitionRequest, UpdateTaskRequest,
};
use ariadne_core::{Actor, TaskStatus};
use ariadne_store::{NewTask, NewTaskAgent, Task, TaskFilter, TaskUpdate};

use super::AppState;
use super::caller::{CallCtx, call_ctx, ensure_task_scope};
use super::convert::{task_dto_of, transition_dto};
use super::error::{ApiError, ApiResult, Json};
use super::pins;

/// The agents an assignment list asks for, in the order it names them.
///
/// An agent has nothing behind it to fall back to: every assignment names its
/// CLI and its model, and one that names no model is refused.
pub(super) async fn resolve_agents(
    state: &AppState,
    assignments: &[AgentAssignment],
) -> ApiResult<Vec<NewTaskAgent>> {
    let mut agents = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        agents.push(NewTaskAgent {
            step: assignment.step.clone(),
            skills: assignment.skills.clone(),
            pin: pins::chosen(
                &state.store,
                &state.agent_registry,
                Some(&assignment.model),
                assignment.effort.as_deref(),
            )
            .await?,
            brief: assignment.brief.clone(),
        });
    }
    Ok(agents)
}

/// Create a task in a goal (orchestrator via MCP, or the user).
#[utoipa::path(post, path = "/v1/goals/{goal_id}/tasks", tag = "tasks",
    request_body = CreateTaskRequest,
    params(("goal_id" = String, Path, description = "goal id")),
    responses((status = 201, body = TaskDto), (status = 400), (status = 409)))]
pub(super) async fn create(
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
        && session.goal_id.as_deref() != Some(goal_id.as_str())
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

    let agents = resolve_agents(&state, &req.agents).await?;

    let task = state
        .store
        .create_task(NewTask {
            goal_id: goal.id,
            repo_id,
            title: req.title,
            description: req.description,
            agents,
            depends_on: req.depends_on,
        })
        .await?;
    let dto = task_dto_of(&state.store, task).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// List tasks.
#[utoipa::path(get, path = "/v1/tasks", tag = "tasks",
    params(TaskListQuery),
    responses((status = 200, body = [TaskDto])))]
pub(super) async fn list(
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
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<TaskDto>> {
    let task = state.store.get_task(&id).await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

/// Edit a task that is not running (orchestrator or user): pending, ready,
/// or failed and waiting for a retry.
#[utoipa::path(patch, path = "/v1/tasks/{id}", tag = "tasks",
    request_body = UpdateTaskRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 404), (status = 409)))]
pub(super) async fn update(
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
    let agents = match &req.agents {
        Some(a) => Some(resolve_agents(&state, a).await?),
        None => None,
    };
    let task = state
        .store
        .update_task(
            &id,
            TaskUpdate {
                agents,
                title: req.title,
                description: req.description,
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
pub(super) async fn transition(
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

/// The one status move an agent makes through this route is to give its task
/// up; every other move of a task is a step call, or the daemon's.
pub(crate) async fn apply_transition(
    state: &AppState,
    ctx: &CallCtx,
    task_id: &str,
    req: TransitionRequest,
) -> ApiResult<Task> {
    if ctx.actor == Actor::Agent && req.to != TaskStatus::Failed {
        return Err(ApiError::forbidden(
            "use the step route to complete a column",
        ));
    }
    // A retry starts the task on its first column again, and runs it through
    // every column: one with nobody on it is named now, not met later with
    // the task stuck in it.
    if req.to == TaskStatus::Ready {
        let task = state.store.get_task(task_id).await?;
        let unstaffed = state.store.unstaffed_columns(&task).await?;
        if !unstaffed.is_empty() {
            return Err(ApiError::conflict(format!(
                "task {task_id} has no agent on column {}; staff it with update_task before \
                 the retry",
                unstaffed.join(", ")
            )));
        }
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
/// refuses a column's agent reaching for it.
#[utoipa::path(post, path = "/v1/tasks/{id}/cancel", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 403), (status = 409)))]
pub(super) async fn cancel(
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

/// Retry a failed task: failed -> ready, which starts it on its first column
/// again. The user's call, and the orchestrator's — the daemon wakes it when
/// a task fails, and retrying is one of the three answers it has. A task
/// with a column nobody staffs is refused by that column's name.
#[utoipa::path(post, path = "/v1/tasks/{id}/retry", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 403), (status = 409)))]
pub(super) async fn retry(
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
pub(super) async fn list_transitions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<TaskTransitionDto>>> {
    state.store.get_task(&id).await?;
    let rows = state.store.list_task_transitions(&id).await?;
    Ok(Json(rows.into_iter().map(transition_dto).collect()))
}
