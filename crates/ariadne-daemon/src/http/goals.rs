//! Goal endpoints.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use utoipa::IntoParams;

use ariadne_api::goals::{CompleteGoalRequest, CreateGoalRequest, FinalizePlanRequest, GoalDto};
use ariadne_api::messages::{MessageDto, MessageListQuery, SendMessageRequest};
use ariadne_core::{GoalStatus, TaskStatus};
use ariadne_store::{Goal, MessageFilter, NewGoal, SessionFilter, TaskFilter};

use super::AppState;
use super::caller::call_ctx;
use super::convert::{goal_dto_of, message_dto};
use super::error::{ApiError, ApiResult, Json};
use super::pins;

#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct GoalListQuery {
    /// Filter by status: one status, or several comma-separated
    /// (`status=active,completed`), matching goals in any of them.
    #[param(value_type = Option<String>, example = "active,completed")]
    pub status: Option<String>,
}

/// A goal with the repositories it references and what its agents have
/// spent, which is how every one of these endpoints answers.
async fn to_dto(state: &AppState, goal: Goal) -> ApiResult<Json<GoalDto>> {
    Ok(Json(
        goal_dto_of(&state.store, &state.agent_registry, goal).await?,
    ))
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

/// Create a goal on registered repositories; the orchestrator session is
/// spawned by the scheduler once agent execution lands.
///
/// The repos are referenced, not copied: whatever `POST /v1/repositories`
/// validated about a checkout holds for every goal that names it, and an edit
/// there moves this goal too.
#[utoipa::path(post, path = "/v1/goals", tag = "goals",
    request_body = CreateGoalRequest,
    responses(
        (status = 201, body = GoalDto),
        (status = 400),
        (status = 404, description = "no such repository or orchestrator profile")
    ))]
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateGoalRequest>,
) -> ApiResult<(StatusCode, Json<GoalDto>)> {
    if req.repository_ids.is_empty() {
        return Err(ApiError::bad_request("a goal needs at least one repo"));
    }
    // Refused before anything is looked up: a missing model, or one that
    // names no agent CLI, is a fact about the request rather than about
    // anything it refers to.
    pins::readable(Some(&req.model), &state.agent_registry)?;

    // Resolved here as well as in the store, so an unknown id is a 404 about
    // the repository rather than a goal that half-exists.
    for id in &req.repository_ids {
        state.store.get_repository(id).await?;
    }
    let pin = pins::chosen(
        &state.store,
        &state.agent_registry,
        Some(&req.model),
        req.effort.as_deref(),
    )
    .await?;

    let goal = state
        .store
        .create_goal(NewGoal {
            title: req.title,
            description: req.description,
            repository_ids: req.repository_ids,
            pin,
        })
        .await?;
    // The scheduler spawns the orchestrator session for goals in planning.
    state.notify_scheduler_goal(&goal.id);
    Ok((StatusCode::CREATED, to_dto(&state, goal).await?))
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
        out.push(goal_dto_of(&state.store, &state.agent_registry, goal).await?);
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
    to_dto(&state, state.store.get_goal(&id).await?).await
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
    to_dto(&state, goal).await
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
/// orchestrator's call alone, and there is nothing left for the user to
/// approve.
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
    if ctx.actor != ariadne_core::Actor::Orchestrator {
        return Err(ApiError::forbidden(
            "only the orchestrator may finalize the plan",
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
    to_dto(&state, goal).await
}

/// Complete the goal: it moves active -> completed and every session of it
/// is torn down.
///
/// The orchestrator's call, because it is the only agent that knows whether
/// the plan did what the goal asked for — the daemon can see that every task
/// ended, and not whether the goal is met. The user's too: it is their goal,
/// and a goal whose orchestrator will not start is otherwise one nothing can
/// close.
///
/// What the daemon checks is the part it can see. A goal with a task still
/// going is not one anybody may declare finished, however sure they are.
#[utoipa::path(post, path = "/v1/goals/{id}/complete", tag = "goals",
    request_body = CompleteGoalRequest,
    params(("id" = String, Path, description = "goal id")),
    responses((status = 200, body = GoalDto), (status = 403), (status = 409)))]
pub async fn complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(_req): Json<CompleteGoalRequest>,
) -> ApiResult<Json<GoalDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if !matches!(
        ctx.actor,
        ariadne_core::Actor::Orchestrator | ariadne_core::Actor::User
    ) {
        return Err(ApiError::forbidden(
            "only the orchestrator or the user may complete the goal",
        ));
    }
    let goal = state.store.get_goal(&id).await?;
    if goal.status() != GoalStatus::Active {
        return Err(ApiError::conflict(format!(
            "goal is {}, expected active",
            goal.status
        )));
    }
    let unfinished: Vec<String> = state
        .store
        .list_tasks(TaskFilter {
            goal_id: Some(id.clone()),
            status: None,
        })
        .await?
        .into_iter()
        .filter(|t| !matches!(t.status(), TaskStatus::Finished | TaskStatus::Cancelled))
        .map(|t| format!("{} ({})", t.title, t.status))
        .collect();
    if !unfinished.is_empty() {
        return Err(ApiError::conflict(format!(
            "the goal still has unfinished tasks: {}",
            unfinished.join(", ")
        )));
    }
    let goal = state
        .store
        .set_goal_status(&id, GoalStatus::Completed)
        .await?;
    state.notify_scheduler_goal(&id);
    to_dto(&state, goal).await
}

/// The messages of a goal: what its agents said that was not about one task.
///
/// A goal's channel is the orchestrator's inbox. Everything an agent says to
/// it about a task is on that task instead, and this is where the rest goes.
#[utoipa::path(get, path = "/v1/goals/{id}/messages", tag = "goals",
    params(("id" = String, Path, description = "goal id"), MessageListQuery),
    responses((status = 200, body = [MessageDto])))]
pub async fn list_goal_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MessageListQuery>,
) -> ApiResult<Json<Vec<MessageDto>>> {
    state.store.get_goal(&id).await?;
    let rows = state
        .store
        .list_messages(MessageFilter {
            goal_id: Some(id),
            to_agent_id: q.to_agent_id,
            undelivered_only: q.undelivered,
            ..Default::default()
        })
        .await?;
    Ok(Json(rows.into_iter().map(message_dto).collect()))
}

/// Send a message about the goal itself.
#[utoipa::path(post, path = "/v1/goals/{id}/messages", tag = "goals",
    request_body = SendMessageRequest,
    params(("id" = String, Path, description = "goal id")),
    responses((status = 201, body = MessageDto), (status = 403), (status = 409)))]
pub async fn post_goal_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<SendMessageRequest>,
) -> ApiResult<(StatusCode, Json<MessageDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let goal = state.store.get_goal(&id).await?;
    let message = super::landing::send(&state, &ctx, &goal.id, None, req).await?;
    state.notify_scheduler_goal(&id);
    Ok((StatusCode::CREATED, Json(message_dto(message))))
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
