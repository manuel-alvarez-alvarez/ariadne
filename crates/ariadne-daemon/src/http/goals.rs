//! Goal endpoints (incl. the goal-level message thread).

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use utoipa::IntoParams;

use ariadne_api::Page;
use ariadne_api::goals::{CreateGoalRequest, FinalizePlanRequest, GoalDto};
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_core::{GoalStatus, Role};
use ariadne_store::{NewGoal, NewMessage};

use super::AppState;
use super::auth::call_ctx;
use super::convert::{goal_dto, message_dto};
use super::error::{ApiError, ApiResult};
use crate::gitutil;

#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct GoalListQuery {
    /// Filter by status.
    pub status: Option<GoalStatus>,
}

/// Create a goal. Validates repos and resolves base branches; the planner
/// session is spawned by the scheduler once agent execution lands.
#[utoipa::path(post, path = "/v1/goals", tag = "goals",
    request_body = CreateGoalRequest,
    responses((status = 201, body = GoalDto), (status = 400)))]
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateGoalRequest>,
) -> ApiResult<(StatusCode, Json<GoalDto>)> {
    if req.repos.is_empty() {
        return Err(ApiError::bad_request("a goal needs at least one repo"));
    }

    let planner = state.store.resolve_profile(&req.planner_profile).await?;
    if planner.role() != Role::Planner {
        return Err(ApiError::bad_request(format!(
            "profile {} has role {}, expected planner",
            planner.name, planner.role
        )));
    }

    let mut repos = Vec::with_capacity(req.repos.len());
    for spec in &req.repos {
        let path = PathBuf::from(&spec.path);
        if !path.is_absolute() {
            return Err(ApiError::bad_request(format!(
                "repo path must be absolute: {}",
                spec.path
            )));
        }
        gitutil::validate_repo(&path)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        let base_branch = match &spec.base_branch {
            Some(b) => {
                if !gitutil::branch_exists(&path, b)
                    .await
                    .map_err(|e| ApiError::bad_request(e.to_string()))?
                {
                    return Err(ApiError::bad_request(format!(
                        "branch {b} does not exist in {}",
                        spec.path
                    )));
                }
                b.clone()
            }
            None => gitutil::current_branch(&path)
                .await
                .map_err(|e| ApiError::bad_request(e.to_string()))?,
        };
        // A branch name alone is not enough: a freshly `git init`ed repo has
        // an unborn branch that worktrees cannot be created from.
        gitutil::ensure_branch_has_commits(&path, &base_branch)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        repos.push((spec.path.clone(), base_branch));
    }

    let goal = state
        .store
        .create_goal(NewGoal {
            title: req.title,
            description: req.description,
            planner_profile_id: planner.id,
            max_tasks: req.max_tasks,
            required_approvals: req.required_approvals.unwrap_or(1),
            repos,
        })
        .await?;
    // The scheduler spawns the planner session for goals in planning.
    state.notify_scheduler_goal(&goal.id).await;
    let repos = state.store.list_goal_repos(&goal.id).await?;
    Ok((StatusCode::CREATED, Json(goal_dto(goal, repos))))
}

/// List goals.
#[utoipa::path(get, path = "/v1/goals", tag = "goals",
    params(GoalListQuery),
    responses((status = 200, body = [GoalDto])))]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<GoalListQuery>,
) -> ApiResult<Json<Vec<GoalDto>>> {
    let goals = state.store.list_goals(q.status).await?;
    let mut out = Vec::with_capacity(goals.len());
    for goal in goals {
        let repos = state.store.list_goal_repos(&goal.id).await?;
        out.push(goal_dto(goal, repos));
    }
    Ok(Json(out))
}

/// Inspect a goal.
#[utoipa::path(get, path = "/v1/goals/{id}", tag = "goals",
    params(("id" = String, Path, description = "goal id")),
    responses((status = 200, body = GoalDto), (status = 404)))]
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<GoalDto>> {
    let goal = state.store.get_goal(&id).await?;
    let repos = state.store.list_goal_repos(&goal.id).await?;
    Ok(Json(goal_dto(goal, repos)))
}

/// Cancel a goal.
#[utoipa::path(post, path = "/v1/goals/{id}/cancel", tag = "goals",
    params(("id" = String, Path, description = "goal id")),
    responses((status = 200, body = GoalDto), (status = 404), (status = 409)))]
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<GoalDto>> {
    let goal = state.store.get_goal(&id).await?;
    if matches!(goal.status(), GoalStatus::Completed | GoalStatus::Cancelled) {
        return Err(ApiError::conflict(format!(
            "goal is already {}",
            goal.status
        )));
    }
    let goal = state
        .store
        .set_goal_status(&id, GoalStatus::Cancelled)
        .await?;
    // The scheduler tears down sessions/worktrees of cancelled goals.
    state.notify_scheduler_goal(&goal.id).await;
    let repos = state.store.list_goal_repos(&goal.id).await?;
    Ok(Json(goal_dto(goal, repos)))
}

/// Finalize planning: goal moves planning -> active (planner or user).
#[utoipa::path(post, path = "/v1/goals/{id}/finalize", tag = "goals",
    request_body = FinalizePlanRequest,
    params(("id" = String, Path, description = "goal id")),
    responses((status = 200, body = GoalDto), (status = 409)))]
pub async fn finalize(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<FinalizePlanRequest>,
) -> ApiResult<Json<GoalDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if !matches!(
        ctx.author_role,
        ariadne_core::AuthorRole::Planner | ariadne_core::AuthorRole::User
    ) {
        return Err(ApiError::forbidden(
            "only the planner or the user may finalize the plan",
        ));
    }
    let goal = state.store.get_goal(&id).await?;
    if goal.status() != GoalStatus::Planning {
        return Err(ApiError::conflict(format!(
            "goal is {}, expected planning",
            goal.status
        )));
    }
    let task_count = state
        .store
        .list_tasks(ariadne_store::TaskFilter {
            goal_id: Some(id.clone()),
            status: None,
        })
        .await?
        .len();
    if task_count == 0 {
        return Err(ApiError::conflict("cannot finalize a plan with no tasks"));
    }
    state
        .store
        .create_message(NewMessage {
            goal_id: id.clone(),
            task_id: None,
            author_role: ctx.author_role,
            author_session_id: ctx.session.map(|s| s.id),
            body: format!("Plan finalized: {}", req.summary),
        })
        .await?;
    let goal = state.store.set_goal_status(&id, GoalStatus::Active).await?;
    // Wake the scheduler: pending tasks with no deps become ready now.
    for task in state
        .store
        .list_tasks(ariadne_store::TaskFilter {
            goal_id: Some(id.clone()),
            status: None,
        })
        .await?
    {
        state.notify_scheduler(&task.id).await;
    }
    let repos = state.store.list_goal_repos(&goal.id).await?;
    Ok(Json(goal_dto(goal, repos)))
}

/// Goal-level message thread (planner discussion).
#[utoipa::path(get, path = "/v1/goals/{id}/messages", tag = "goals",
    params(("id" = String, Path, description = "goal id"), Page),
    responses((status = 200, body = [MessageDto])))]
pub async fn list_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<MessageDto>>> {
    state.store.get_goal(&id).await?;
    let msgs = state
        .store
        .list_goal_messages(&id, page.after.as_deref(), page.limit())
        .await?;
    Ok(Json(msgs.into_iter().map(message_dto).collect()))
}

/// Post to the goal-level thread.
#[utoipa::path(post, path = "/v1/goals/{id}/messages", tag = "goals",
    request_body = CreateMessageRequest,
    params(("id" = String, Path, description = "goal id")),
    responses((status = 201, body = MessageDto)))]
pub async fn post_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateMessageRequest>,
) -> ApiResult<(StatusCode, Json<MessageDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    state.store.get_goal(&id).await?;
    let msg = state
        .store
        .create_message(NewMessage {
            goal_id: id,
            task_id: None,
            author_role: ctx.author_role,
            author_session_id: ctx.session.map(|s| s.id),
            body: req.body,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(message_dto(msg))))
}
