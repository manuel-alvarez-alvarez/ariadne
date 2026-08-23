//! Goal endpoints (incl. the goal-level message thread).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use utoipa::IntoParams;

use ariadne_api::Page;
use ariadne_api::goals::{CreateGoalRequest, FinalizePlanRequest, GoalDto};
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_core::{GoalStatus, Role};
use ariadne_store::{NewGoal, NewMessage, SessionFilter};

use super::AppState;
use super::auth::call_ctx;
use super::convert::{goal_dto, message_dto_of, message_dtos};
use super::error::{ApiError, ApiResult};
use super::recipients;

#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct GoalListQuery {
    /// Filter by status: one status, or several comma-separated
    /// (`status=active,completed`), matching goals in any of them.
    #[param(value_type = Option<String>, example = "active,completed")]
    pub status: Option<String>,
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
    state.notify_scheduler_goal(&goal.id).await;
    let repos = state.store.list_goal_repositories(&goal.id).await?;
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
    let goal = state.store.get_goal(&id).await?;
    let repos = state.store.list_goal_repositories(&goal.id).await?;
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
    let repos = state.store.list_goal_repositories(&goal.id).await?;
    Ok(Json(goal_dto(goal, repos)))
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
            recipient: None,
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
    let repos = state.store.list_goal_repositories(&goal.id).await?;
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
    let recipient = match &req.to {
        Some(to) => {
            let participants = recipients::goal_participants(&state.store, &goal).await?;
            Some(recipients::resolve(&state.store, to, &participants).await?)
        }
        None => None,
    };
    let msg = state
        .store
        .create_message(NewMessage {
            goal_id: id,
            task_id: None,
            author_role: ctx.author_role,
            author_session_id: ctx.session.map(|s| s.id),
            recipient,
            body: req.body,
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(message_dto_of(&state.store, msg).await?),
    ))
}

#[cfg(test)]
mod tests {
    use super::GoalListQuery;

    use ariadne_core::GoalStatus;
    use axum::http::StatusCode;

    fn query(status: Option<&str>) -> GoalListQuery {
        GoalListQuery {
            status: status.map(str::to_string),
        }
    }

    #[test]
    fn no_status_param_means_every_goal() {
        assert_eq!(query(None).statuses().unwrap(), vec![]);
    }

    #[test]
    fn a_single_status_still_parses() {
        assert_eq!(
            query(Some("active")).statuses().unwrap(),
            vec![GoalStatus::Active]
        );
    }

    #[test]
    fn a_comma_separated_list_parses_in_order() {
        assert_eq!(
            query(Some("active,completed")).statuses().unwrap(),
            vec![GoalStatus::Active, GoalStatus::Completed]
        );
    }

    #[test]
    fn an_unknown_status_is_a_bad_request() {
        for raw in ["nope", "active,nope", ""] {
            let err = query(Some(raw)).statuses().unwrap_err();
            assert_eq!(err.status, StatusCode::BAD_REQUEST);
        }
    }
}
