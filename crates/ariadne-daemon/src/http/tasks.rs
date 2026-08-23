//! Task endpoints: CRUD, transitions, messages, reviews.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::Page;
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::reviews::{CreateReviewRequest, ReviewDto};
use ariadne_api::tasks::{
    CreateTaskRequest, ReturnToEngineerRequest, TaskDto, TaskListQuery, TaskTransitionDto,
    TransitionRequest, UpdateTaskRequest,
};
use ariadne_core::{Actor, ReviewVerdict, Role, TaskStatus};
use ariadne_store::{NewMessage, NewReview, NewTask, Store, Task, TaskFilter, TaskUpdate};

use super::AppState;
use super::auth::{CallCtx, call_ctx, ensure_task_scope};
use super::convert::{message_dto_of, message_dtos, review_dto, task_dto, transition_dto};
use super::error::{ApiError, ApiResult};
use super::recipients;

async fn to_dto(store: &Store, task: Task) -> ApiResult<TaskDto> {
    let reviewers = store.list_task_reviewer_pins(&task.id).await?;
    let deps = store.list_task_dependencies(&task.id).await?;
    Ok(task_dto(task, reviewers, deps))
}

/// Resolve a list of profile ids-or-names, checking each has `role`.
async fn resolve_profiles(store: &Store, specs: &[String], role: Role) -> ApiResult<Vec<String>> {
    let mut ids = Vec::with_capacity(specs.len());
    for spec in specs {
        let p = store.resolve_profile(spec).await?;
        if p.role() != role {
            return Err(ApiError::bad_request(format!(
                "profile {} has role {}, expected {}",
                p.name,
                p.role,
                role.as_str()
            )));
        }
        ids.push(p.id);
    }
    Ok(ids)
}

/// The integrator a task is created with: the one named, or the built-in.
///
/// The built-in is looked up by id rather than required: deleting a built-in
/// profile is allowed and permanent, and an install that deleted this one
/// still creates tasks — they simply name no integrator, which is the shape
/// every task had before the role existed.
async fn resolve_integrator(store: &Store, spec: Option<&str>) -> ApiResult<Option<String>> {
    if let Some(spec) = spec {
        let spec = spec.to_string();
        let mut ids =
            resolve_profiles(store, std::slice::from_ref(&spec), Role::Integrator).await?;
        return Ok(Some(ids.remove(0)));
    }
    Ok(store.builtin_integrator().await.map(|p| p.id))
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

    let engineer = resolve_profiles(
        &state.store,
        std::slice::from_ref(&req.engineer_profile),
        Role::Engineer,
    )
    .await?
    .remove(0);
    let reviewers = resolve_profiles(&state.store, &req.reviewer_profiles, Role::Reviewer).await?;
    let integrator = resolve_integrator(&state.store, req.integrator_profile.as_deref()).await?;

    let task = state
        .store
        .create_task(NewTask {
            goal_id: goal.id,
            repo_id,
            title: req.title,
            description: req.description,
            engineer_profile_id: engineer,
            integrator_profile_id: integrator,
            reviewer_profile_ids: reviewers,
            depends_on: req.depends_on,
        })
        .await?;
    let dto = to_dto(&state.store, task).await?;
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
        out.push(to_dto(&state.store, task).await?);
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
    Ok(Json(to_dto(&state.store, task).await?))
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
    let reviewer_profile_ids = match &req.reviewer_profiles {
        Some(specs) => Some(resolve_profiles(&state.store, specs, Role::Reviewer).await?),
        None => None,
    };
    let task = state
        .store
        .update_task(
            &id,
            TaskUpdate {
                title: req.title,
                description: req.description,
                reviewer_profile_ids,
            },
        )
        .await?;
    if let Some(deps) = req.depends_on {
        state.store.set_task_dependencies(&id, &deps).await?;
    }
    let task = state.store.get_task(&task.id).await?;
    Ok(Json(to_dto(&state.store, task).await?))
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
    Ok(Json(to_dto(&state.store, task).await?))
}

pub(crate) async fn apply_transition(
    state: &AppState,
    ctx: &CallCtx,
    task_id: &str,
    req: TransitionRequest,
) -> ApiResult<Task> {
    // `merged` is never taken on faith: the branch must actually be an
    // ancestor of the base branch in the repo.
    if req.to == TaskStatus::Merged {
        let task = state.store.get_task(task_id).await?;
        let repo = state.store.get_repository(&task.repo_id).await?;
        let repo_path = std::path::PathBuf::from(&repo.path);
        let merged = state
            .launcher
            .git
            .is_ancestor(&repo_path, &task.branch, &repo.base_branch)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        if !merged {
            return Err(ApiError::conflict(format!(
                "merge not verified: {} is not an ancestor of {} in {}",
                task.branch, repo.base_branch, repo.path
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
    state.notify_scheduler(task_id).await;
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
    Ok(Json(to_dto(&state.store, task).await?))
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
    Ok(Json(to_dto(&state.store, task).await?))
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
    let recipient = match &req.to {
        Some(to) => {
            let participants = recipients::task_participants(&state.store, &task).await?;
            Some(recipients::resolve(&state.store, to, &participants).await?)
        }
        None => None,
    };
    let msg = state
        .store
        .create_message(NewMessage {
            goal_id: task.goal_id,
            task_id: Some(id),
            author_role: ctx.author_role,
            author_session_id: ctx.session.map(|s| s.id),
            recipient,
            body: req.body,
        })
        .await?;
    // Addressed or not, the scheduler is told: what an unaddressed message
    // wakes is nobody, and that is its decision to make rather than the
    // handler's.
    state.notify_scheduler_message(&msg.id).await;
    Ok((
        StatusCode::CREATED,
        Json(message_dto_of(&state.store, msg).await?),
    ))
}

/// Reviews of a task.
#[utoipa::path(get, path = "/v1/tasks/{id}/reviews", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = [ReviewDto])))]
pub async fn list_reviews(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ReviewDto>>> {
    state.store.get_task(&id).await?;
    let rows = state.store.list_reviews(&id, None).await?;
    Ok(Json(rows.into_iter().map(review_dto).collect()))
}

/// Submit a review verdict for the current round.
#[utoipa::path(post, path = "/v1/tasks/{id}/reviews", tag = "tasks",
    request_body = CreateReviewRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 201, body = ReviewDto), (status = 409)))]
pub async fn post_review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateReviewRequest>,
) -> ApiResult<(StatusCode, Json<ReviewDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let task = state.store.get_task(&id).await?;
    if task.status() != TaskStatus::UnderReview {
        return Err(ApiError::conflict(format!(
            "task is {}, reviews are only accepted under_review",
            task.status
        )));
    }

    // Reviewer identity: from the session, or explicit for user-submitted reviews.
    let reviewer_profile_id = match (&ctx.session, &req.reviewer_profile) {
        (Some(session), _) => {
            if session.role() != Role::Reviewer {
                return Err(ApiError::forbidden(
                    "only reviewer sessions may submit reviews",
                ));
            }
            session.profile_id.clone()
        }
        (None, Some(spec)) => state.store.resolve_profile(spec).await?.id,
        (None, None) => {
            return Err(ApiError::bad_request(
                "reviewer_profile is required for user-submitted reviews",
            ));
        }
    };
    let assigned = state.store.list_task_reviewers(&id).await?;
    if !assigned.contains(&reviewer_profile_id) {
        return Err(ApiError::forbidden(format!(
            "profile {reviewer_profile_id} is not an assigned reviewer of task {id}"
        )));
    }

    let review = state
        .store
        .create_review(NewReview {
            task_id: id.clone(),
            round: task.review_round,
            reviewer_profile_id,
            session_id: ctx.session.map(|s| s.id),
            verdict: req.verdict,
            body: req.body,
        })
        .await?;
    state.notify_scheduler(&id).await;
    Ok((StatusCode::CREATED, Json(review_dto(review))))
}

/// Hand an integrating task back to its engineer (integrator).
///
/// The feedback is recorded as a change-request verdict on the round that was
/// approved, so it reaches the engineer exactly the way a reviewer's does — in
/// the resume briefing, and in `get_reviews` beside the approvals it follows.
#[utoipa::path(post, path = "/v1/tasks/{id}/return-to-engineer", tag = "tasks",
    request_body = ReturnToEngineerRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 200, body = TaskDto),
        (status = 403, description = "not an integrator session"),
        (status = 409, description = "the task is not being integrated")
    ))]
pub async fn return_to_engineer(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ReturnToEngineerRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let Some(session) = ctx
        .session
        .as_ref()
        .filter(|s| s.role() == Role::Integrator)
    else {
        return Err(ApiError::forbidden(
            "only the integrator of a task may send it back to its engineer",
        ));
    };
    let task = state.store.get_task(&id).await?;
    if task.status() != TaskStatus::Integrating {
        return Err(ApiError::conflict(format!(
            "task is {}, only a task being integrated can be sent back to its engineer",
            task.status
        )));
    }

    // The feedback is recorded before the transition, so that the engineer the
    // scheduler resumes on it has it to read. A transition that then fails
    // leaves a verdict on a round nobody is waiting on, which is inert — the
    // next round is a new one.
    state
        .store
        .create_review(NewReview {
            task_id: id.clone(),
            round: task.review_round,
            reviewer_profile_id: session.profile_id.clone(),
            session_id: Some(session.id.clone()),
            verdict: ReviewVerdict::RequestChanges,
            body: Some(feedback_body(&req)),
        })
        .await?;
    let task = apply_transition(
        &state,
        &ctx,
        &id,
        TransitionRequest {
            to: TaskStatus::ChangesRequested,
            reason: Some(req.summary),
            merge_commit: None,
        },
    )
    .await?;
    Ok(Json(to_dto(&state.store, task).await?))
}

/// The send-back as the engineer reads it: what happened, then the list of
/// what to do about it.
fn feedback_body(req: &ReturnToEngineerRequest) -> String {
    let mut body = req.summary.trim().to_string();
    if !req.changes.is_empty() {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(
            &req.changes
                .iter()
                .map(|change| format!("- {}", change.trim()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    body
}

/// Diff of the task branch against its base (`git diff base...branch`), or,
/// once the task is merged, the diff its merge commit brought into the base —
/// after the merge the branch is contained in the base, so the three-dot diff
/// would be forever empty.
#[utoipa::path(get, path = "/v1/tasks/{id}/diff", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, content_type = "text/plain", body = String), (status = 404), (status = 409)))]
pub async fn diff(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<String> {
    let task = state.store.get_task(&id).await?;
    let repo = state.store.get_repository(&task.repo_id).await?;
    let repo_path = std::path::PathBuf::from(&repo.path);

    if let Some(merge_commit) = &task.merge_commit {
        // Also the only diff that still exists once the merged branch and
        // worktree have been cleaned up.
        return state
            .launcher
            .git
            .diff_against_first_parent(&repo_path, merge_commit)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()));
    }

    if !state
        .launcher
        .git
        .branch_exists(&repo_path, &task.branch)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?
    {
        return Err(ApiError::conflict(format!(
            "branch {} does not exist yet (task not started?)",
            task.branch
        )));
    }
    state
        .launcher
        .git
        .diff(&repo_path, &repo.base_branch, &task.branch)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))
}
