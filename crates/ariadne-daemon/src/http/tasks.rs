//! Task endpoints: CRUD, transitions and the task conversation. What a task
//! being reviewed and landed goes through is in [`super::landing`].

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::Page;
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::tasks::{
    CreateTaskRequest, ReviewerAssignment, TaskDto, TaskListQuery, TaskTransitionDto,
    TransitionRequest, UpdateTaskRequest,
};
use ariadne_core::{Actor, Role, TaskStatus};
use ariadne_store::{NewTask, Profile, ReviewerSlot, Store, Task, TaskFilter, TaskUpdate};

use super::AppState;
use super::convert::{message_dtos, task_dto_of, transition_dto};
use super::error::{ApiError, ApiResult};
use super::landing;
use super::pins::{self, Repin, Standing};
use super::recipients::{self, CallCtx, Thread, call_ctx, ensure_task_scope};
use crate::notify;

/// Resolve one profile id-or-name, checking it has `role`: the shape every
/// profile assignment takes.
///
/// The profile itself rather than its id, because what it is pinned to is what
/// an effort written with no model beside it is run at.
async fn resolve_profile(store: &Store, spec: &str, role: Role) -> ApiResult<Profile> {
    let p = store.resolve_profile(spec).await?;
    if p.role() != role {
        return Err(ApiError::bad_request(format!(
            "profile {} has role {}, expected {}",
            p.name,
            p.role,
            role.as_str()
        )));
    }
    Ok(p)
}

/// What a profile is pinned to, as the fallback an effort of its own is
/// checked against and pinned to.
fn standing(profile: &Profile) -> Standing<'_> {
    Standing {
        agent_kind: profile.agent_kind(),
        model: profile.model.as_deref(),
    }
}

/// The reviewer slots an assignment list asks for, in the order it names them:
/// each profile resolved as any other, each slot carrying the model and effort
/// chosen for it or, where none was, nothing — which is the store's cue to pin
/// the profile's own.
async fn resolve_reviewers(
    store: &Store,
    assignments: &[ReviewerAssignment],
) -> ApiResult<Vec<ReviewerSlot>> {
    let mut slots = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        let profile = resolve_profile(store, &assignment.profile, Role::Reviewer).await?;
        slots.push(ReviewerSlot {
            pin: pins::chosen(
                assignment.model.as_deref(),
                assignment.effort.as_deref(),
                standing(&profile),
            )
            .await?,
            profile_id: profile.id,
        });
    }
    Ok(slots)
}

/// Create a task in a goal (planner via MCP, or the user).
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
    if !matches!(ctx.actor, Actor::Planner | Actor::User) {
        return Err(ApiError::forbidden(
            "only the planner or the user may create tasks",
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

    let engineer = resolve_profile(&state.store, &req.engineer_profile, Role::Engineer).await?;
    let reviewers = resolve_reviewers(&state.store, &req.reviewers).await?;
    let pin = pins::chosen(
        req.model.as_deref(),
        req.effort.as_deref(),
        standing(&engineer),
    )
    .await?;

    let task = state
        .store
        .create_task(NewTask {
            goal_id: goal.id,
            repo_id,
            title: req.title,
            description: req.description,
            engineer_profile_id: engineer.id,
            pin,
            reviewers,
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

/// Edit a pending/ready task (planner or user).
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
    if !matches!(ctx.actor, Actor::Planner | Actor::User) {
        return Err(ApiError::forbidden(
            "only the planner or the user may edit tasks",
        ));
    }
    let reviewers = match &req.reviewers {
        Some(assignments) => Some(resolve_reviewers(&state.store, assignments).await?),
        None => None,
    };
    // What the task is pinned to now: an effort written on its own is run at
    // that model, and moves without disturbing it.
    let current = state.store.get_task(&id).await?;
    let (pin, effort) = match pins::rechosen(
        req.model.as_deref(),
        req.effort.as_deref(),
        Standing {
            agent_kind: current.agent_kind(),
            model: current.model.as_deref(),
        },
    )
    .await?
    {
        Repin::Untouched => (None, None),
        Repin::Profile => (Some(None), None),
        Repin::To(pin) => (Some(Some(pin)), None),
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
    // `merged` is never taken on faith.
    if req.to == TaskStatus::Merged {
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
    // A task that ended is told to the user here, where every transition a
    // person or an agent asks for goes through: `notify::task_ended` writes
    // nothing for the statuses that are not endings.
    if let Some(msg) = notify::task_ended(&state.store, &task, req.reason.as_deref()).await? {
        state.notify_scheduler_message(&msg.id);
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

/// Cancel a task (user).
#[utoipa::path(post, path = "/v1/tasks/{id}/cancel", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 409)))]
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = CallCtx::user();
    let task = apply_transition(
        &state,
        &ctx,
        &id,
        TransitionRequest {
            to: TaskStatus::Cancelled,
            reason: Some("cancelled by user".into()),
            merge_commit: None,
        },
    )
    .await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

/// Retry a failed task (user): failed -> ready.
#[utoipa::path(post, path = "/v1/tasks/{id}/retry", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 409)))]
pub async fn retry(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = CallCtx::user();
    let task = apply_transition(
        &state,
        &ctx,
        &id,
        TransitionRequest {
            to: TaskStatus::Ready,
            reason: Some("retried by user".into()),
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

/// Task conversation.
#[utoipa::path(get, path = "/v1/tasks/{id}/messages", tag = "tasks",
    params(("id" = String, Path, description = "task id"), Page),
    responses((status = 200, body = [MessageDto])))]
pub async fn list_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<MessageDto>>> {
    state.store.get_task(&id).await?;
    let msgs = state
        .store
        .list_task_messages(&id, page.after.as_deref(), page.limit())
        .await?;
    Ok(Json(message_dtos(&state.store, msgs).await?))
}

/// Post into the task conversation.
#[utoipa::path(post, path = "/v1/tasks/{id}/messages", tag = "tasks",
    request_body = CreateMessageRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 201, body = MessageDto),
        (status = 400, description = "unknown addressee, or one taking no part in the task")
    ))]
pub async fn post_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateMessageRequest>,
) -> ApiResult<(StatusCode, Json<MessageDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let task = state.store.get_task(&id).await?;
    recipients::post(&state, ctx, Thread::Task(task), req).await
}
