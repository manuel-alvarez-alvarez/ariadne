//! Goal endpoints (incl. the goal-level message thread).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use utoipa::IntoParams;

use ariadne_api::Page;
use ariadne_api::goals::{CreateGoalRequest, FinalizePlanRequest, GoalDto, SubmitPlanRequest};
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_core::{GoalStatus, Role};
use ariadne_store::{Goal, NewGoal, NewMessage, SessionFilter, Store, Task, TaskFilter};

use super::AppState;
use super::convert::{goal_dto, message_dtos};
use super::error::{ApiError, ApiResult};
use super::recipients::{self, Thread, call_ctx};

#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct GoalListQuery {
    /// Filter by status: one status, or several comma-separated
    /// (`status=active,completed`), matching goals in any of them.
    #[param(value_type = Option<String>, example = "active,completed")]
    pub status: Option<String>,
}

/// A goal with the repositories it references, which is how every one of
/// these endpoints answers.
async fn to_dto(store: &Store, goal: Goal) -> ApiResult<Json<GoalDto>> {
    let repos = store.list_goal_repositories(&goal.id).await?;
    Ok(Json(goal_dto(goal, repos)))
}

impl GoalListQuery {
    /// The requested statuses; empty means "every goal". An unknown value is
    /// a 400, the same as a single unparseable status was.
    fn statuses(&self) -> Result<Vec<GoalStatus>, ApiError> {
        let Some(raw) = &self.status else {
            return Ok(Vec::new());
        };
        raw.split(',')
            .map(|s| s.parse::<GoalStatus>().map_err(ApiError::bad_request))
            .collect()
    }
}

/// Create a goal on registered repositories; the planner session is spawned
/// by the scheduler once agent execution lands.
///
/// The repos are referenced, not copied: whatever `POST /v1/repositories`
/// validated about a checkout holds for every goal that names it, and an edit
/// there moves this goal too.
#[utoipa::path(post, path = "/v1/goals", tag = "goals",
    request_body = CreateGoalRequest,
    responses(
        (status = 201, body = GoalDto),
        (status = 400),
        (status = 404, description = "no such repository or planner profile")
    ))]
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateGoalRequest>,
) -> ApiResult<(StatusCode, Json<GoalDto>)> {
    if req.repository_ids.is_empty() {
        return Err(ApiError::bad_request("a goal needs at least one repo"));
    }

    let planner = state.store.resolve_profile(&req.planner_profile).await?;
    if planner.role() != Role::Planner {
        return Err(ApiError::bad_request(format!(
            "profile {} has role {}, expected planner",
            planner.name, planner.role
        )));
    }
    // Resolved here as well as in the store, so an unknown id is a 404 about
    // the repository rather than a goal that half-exists.
    for id in &req.repository_ids {
        state.store.get_repository(id).await?;
    }

    let goal = state
        .store
        .create_goal(NewGoal {
            title: req.title,
            description: req.description,
            planner_profile_id: planner.id,
            max_tasks: req.max_tasks,
            required_approvals: req.required_approvals.unwrap_or(1),
            repository_ids: req.repository_ids,
        })
        .await?;
    // The scheduler spawns the planner session for goals in planning.
    state.notify_scheduler_goal(&goal.id);
    Ok((StatusCode::CREATED, to_dto(&state.store, goal).await?))
}

/// List goals.
#[utoipa::path(get, path = "/v1/goals", tag = "goals",
    params(GoalListQuery),
    responses((status = 200, body = [GoalDto])))]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<GoalListQuery>,
) -> ApiResult<Json<Vec<GoalDto>>> {
    let goals = state.store.list_goals(&q.statuses()?).await?;
    let mut out = Vec::with_capacity(goals.len());
    for goal in goals {
        let repos = state.store.list_goal_repositories(&goal.id).await?;
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
    to_dto(&state.store, state.store.get_goal(&id).await?).await
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
    state.notify_scheduler_goal(&goal.id);
    to_dto(&state.store, goal).await
}

/// Delete a finished goal and everything under it.
#[utoipa::path(delete, path = "/v1/goals/{id}", tag = "goals",
    params(("id" = String, Path, description = "goal id")),
    responses(
        (status = 204),
        (status = 404),
        (status = 409, description = "the goal is not finished yet; cancel it first")
    ))]
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let goal = state.store.get_goal(&id).await?;
    // Terminal goals only: an active one still owns tmux sessions and git
    // worktrees that only the cancel path tears down, and a hard delete here
    // would orphan them.
    if !goal.status().is_terminal() {
        return Err(ApiError::conflict(format!(
            "goal is {}, cancel it before deleting it",
            goal.status
        )));
    }
    // A terminal goal is *supposed* to own nothing live, but the delete is
    // what makes a mistake permanent: the rows cascade away and a pane that
    // outlived them is no longer anything the daemon can name, let alone
    // reap. So whatever is still standing is taken down first, and only a
    // clean teardown gets to delete.
    for session in state
        .store
        .list_sessions(SessionFilter {
            goal_id: Some(goal.id.clone()),
            live_only: true,
            ..Default::default()
        })
        .await?
    {
        tracing::info!(goal = %goal.id, session = %session.id, "deleting goal: killing a session that outlived it");
        state
            .launcher
            .kill_session(&session.id)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
    }
    state.store.delete_goal(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The tasks a plan holds, or a 409 saying it holds none: what neither
/// submitting nor approving a plan is worth doing without.
async fn planned_tasks(state: &AppState, id: &str, verb: &str) -> ApiResult<Vec<Task>> {
    let tasks = state
        .store
        .list_tasks(TaskFilter {
            goal_id: Some(id.to_string()),
            status: None,
        })
        .await?;
    if tasks.is_empty() {
        return Err(ApiError::conflict(format!(
            "cannot {verb} a plan with no tasks"
        )));
    }
    Ok(tasks)
}

/// Submit the plan for the user's approval: goal moves planning -> plan_ready
/// (planner or user). Nothing starts; only `finalize` does that.
#[utoipa::path(post, path = "/v1/goals/{id}/submit", tag = "goals",
    request_body = SubmitPlanRequest,
    params(("id" = String, Path, description = "goal id")),
    responses((status = 200, body = GoalDto), (status = 403), (status = 409)))]
pub async fn submit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<SubmitPlanRequest>,
) -> ApiResult<Json<GoalDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if !matches!(
        ctx.author_role,
        ariadne_core::AuthorRole::Planner | ariadne_core::AuthorRole::User
    ) {
        return Err(ApiError::forbidden(
            "only the planner or the user may submit the plan",
        ));
    }
    let goal = state.store.get_goal(&id).await?;
    // `plan_ready` as well as `planning`: a plan the user sent back for
    // changes is submitted again from where it already is, and the goal never
    // falls back to planning in between.
    if !matches!(goal.status(), GoalStatus::Planning | GoalStatus::PlanReady) {
        return Err(ApiError::conflict(format!(
            "goal is {}, expected planning or plan_ready",
            goal.status
        )));
    }
    planned_tasks(&state, &id, "submit").await?;
    let goal = state
        .store
        .set_goal_status(&id, GoalStatus::PlanReady)
        .await?;
    // Addressed to the user, through the one path an addressed message takes:
    // being told the plan is theirs to read is the whole point of submitting
    // it. After the status change, so that whatever the delivery wakes finds
    // the goal already waiting rather than still being planned.
    let _ = recipients::post(
        &state,
        ctx,
        Thread::Goal(goal.clone()),
        CreateMessageRequest {
            body: format!("Plan submitted for approval: {}", req.summary),
            to: Some(recipients::USER.to_string()),
        },
    )
    .await?;
    // The planner is left alone in `plan_ready`; the wake is what tells the
    // scheduler the goal has moved at all.
    state.notify_scheduler_goal(&goal.id);
    to_dto(&state.store, goal).await
}

/// Approve the plan: goal moves plan_ready (or planning) -> active, and its
/// tasks start. The user's call alone — the planner submits, it does not
/// approve its own plan.
#[utoipa::path(post, path = "/v1/goals/{id}/finalize", tag = "goals",
    request_body = FinalizePlanRequest,
    params(("id" = String, Path, description = "goal id")),
    responses((status = 200, body = GoalDto), (status = 403), (status = 409)))]
pub async fn finalize(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<FinalizePlanRequest>,
) -> ApiResult<Json<GoalDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if ctx.author_role != ariadne_core::AuthorRole::User {
        return Err(ApiError::forbidden("only the user may finalize the plan"));
    }
    let goal = state.store.get_goal(&id).await?;
    // Straight from `planning` too: a user who has read the plan need not
    // wait for the planner to hand it over before approving it.
    if !matches!(goal.status(), GoalStatus::Planning | GoalStatus::PlanReady) {
        return Err(ApiError::conflict(format!(
            "goal is {}, expected planning or plan_ready",
            goal.status
        )));
    }
    let tasks = planned_tasks(&state, &id, "finalize").await?;
    // Written straight rather than through `recipients::post`: it addresses
    // nobody, and the scheduler is woken below by every task the plan holds.
    state
        .store
        .create_message(NewMessage {
            goal_id: id.clone(),
            task_id: None,
            author_role: ctx.author_role,
            author_session_id: ctx.session.map(|s| s.id),
            recipient: None,
            body: format!("Plan finalized: {}", req.summary),
        })
        .await?;
    let goal = state.store.set_goal_status(&id, GoalStatus::Active).await?;
    // Wake the scheduler: pending tasks with no deps become ready now.
    for task in tasks {
        state.notify_scheduler(&task.id);
    }
    to_dto(&state.store, goal).await
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
    Ok(Json(message_dtos(&state.store, msgs).await?))
}

/// Post to the goal-level thread.
#[utoipa::path(post, path = "/v1/goals/{id}/messages", tag = "goals",
    request_body = CreateMessageRequest,
    params(("id" = String, Path, description = "goal id")),
    responses(
        (status = 201, body = MessageDto),
        (status = 400, description = "unknown addressee, or one taking no part in the goal")
    ))]
pub async fn post_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateMessageRequest>,
) -> ApiResult<(StatusCode, Json<MessageDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let goal = state.store.get_goal(&id).await?;
    recipients::post(&state, ctx, Thread::Goal(goal), req).await
}

#[cfg(test)]
mod tests {
    use super::GoalListQuery;

    use ariadne_core::GoalStatus;
    use axum::http::StatusCode;

    fn statuses(status: Option<&str>) -> Result<Vec<GoalStatus>, super::ApiError> {
        GoalListQuery {
            status: status.map(str::to_string),
        }
        .statuses()
    }

    /// One status or several, in the order they were asked for; no `status`
    /// at all means every goal.
    #[test]
    fn the_status_filter_takes_a_list() {
        use GoalStatus::*;
        for (raw, expected) in [
            (None, vec![]),
            (Some("active"), vec![Active]),
            (Some("plan_ready"), vec![PlanReady]),
            (Some("active,completed"), vec![Active, Completed]),
        ] {
            assert_eq!(statuses(raw).unwrap(), expected, "{raw:?}");
        }
    }

    /// An unknown value is a 400, alone or in a list — never a filter that
    /// quietly matches something else.
    #[test]
    fn an_unknown_status_is_a_bad_request() {
        for raw in ["nope", "active,nope", ""] {
            assert_eq!(
                statuses(Some(raw)).unwrap_err().status,
                StatusCode::BAD_REQUEST,
                "{raw:?}"
            );
        }
    }
}
