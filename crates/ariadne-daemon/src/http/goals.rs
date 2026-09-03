//! Goal endpoints.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use utoipa::IntoParams;

use ariadne_api::goals::{CreateGoalRequest, FinalizePlanRequest, GoalDto};
use ariadne_core::{GoalStatus, Role};
use ariadne_store::{Goal, NewGoal, SessionFilter, Store, TaskFilter};

use super::AppState;
use super::convert::goal_dto_of;
use super::error::{ApiError, ApiResult, Json};
use super::pins::{self, Standing};
use super::caller::call_ctx;

#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct GoalListQuery {
    /// Filter by status: one status, or several comma-separated
    /// (`status=active,completed`), matching goals in any of them.
    #[param(value_type = Option<String>, example = "active,completed")]
    pub status: Option<String>,
}

/// A goal with the repositories it references and what its agents have
/// spent, which is how every one of these endpoints answers.
async fn to_dto(store: &Store, goal: Goal) -> ApiResult<Json<GoalDto>> {
    Ok(Json(goal_dto_of(store, goal).await?))
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
    // Refused before anything is looked up: a model that names no agent CLI is
    // a fact about the request, not about the profiles it names. The effort
    // beside it is checked below, since an effort written on its own is run at
    // whatever the planner profile is on.
    pins::readable(req.model.as_deref())?;

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
    let pin = pins::chosen(
        req.model.as_deref(),
        req.effort.as_deref(),
        Standing {
            agent_kind: planner.agent_kind(),
            model: planner.model.as_deref(),
        },
    )
    .await?;

    let goal = state
        .store
        .create_goal(NewGoal {
            title: req.title,
            description: req.description,
            planner_profile_id: planner.id,
            max_tasks: req.max_tasks,
            required_approvals: req.required_approvals.unwrap_or(1),
            repository_ids: req.repository_ids,
            pin,
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
        out.push(goal_dto_of(&state.store, goal).await?);
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

/// Finalize the plan: goal moves planning -> active and its tasks start. The
/// planner's call alone, and there is nothing left for the user to approve.
#[utoipa::path(post, path = "/v1/goals/{id}/finalize", tag = "goals",
    request_body = FinalizePlanRequest,
    params(("id" = String, Path, description = "goal id")),
    responses((status = 200, body = GoalDto), (status = 403), (status = 409)))]
pub async fn finalize(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(_req): Json<FinalizePlanRequest>,
) -> ApiResult<Json<GoalDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if ctx.actor != ariadne_core::Actor::Planner {
        return Err(ApiError::forbidden(
            "only the planner may finalize the plan",
        ));
    }
    let goal = state.store.get_goal(&id).await?;
    if goal.status() != GoalStatus::Planning {
        return Err(ApiError::conflict(format!(
            "goal is {}, expected planning",
            goal.status
        )));
    }
    let tasks = state
        .store
        .list_tasks(TaskFilter {
            goal_id: Some(id.clone()),
            status: None,
        })
        .await?;
    if tasks.is_empty() {
        return Err(ApiError::conflict("cannot finalize a plan with no tasks"));
    }
    let goal = state.store.set_goal_status(&id, GoalStatus::Active).await?;
    // Wake the scheduler: pending tasks with no deps become ready now.
    for task in tasks {
        state.notify_scheduler(&task.id);
    }
    to_dto(&state.store, goal).await
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
