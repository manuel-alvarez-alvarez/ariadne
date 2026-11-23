//! Worker draft creation and explicit source authorization.
use super::{
    AppState,
    convert::task_dto_of,
    error::{ApiError, ApiResult, Json},
};
use ariadne_api::tasks::{
    AuthorizeRemoteTaskRequest, CreateRemoteTaskRequest, RemoteTaskProtocol, TaskDto,
};
use ariadne_store::{GoalStep, NewRemoteTask};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};

fn source_only(headers: &HeaderMap) -> ApiResult<()> {
    if headers.contains_key(ariadne_api::SESSION_HEADER) {
        return Err(ApiError::forbidden(
            "an agent session cannot create or authorize remote drafts",
        ));
    }
    Ok(())
}

#[utoipa::path(get, path = "/v1/remote-tasks/protocol", tag = "tasks",
    responses((status = 200, body = RemoteTaskProtocol)))]
pub(super) async fn protocol(State(state): State<AppState>) -> ApiResult<Json<RemoteTaskProtocol>> {
    Ok(Json(RemoteTaskProtocol {
        protocol: "remote-tasks-v1".into(),
        daemon_id: state.store.daemon_id().await?,
    }))
}

#[utoipa::path(put, path = "/v1/remote-tasks/{id}", tag = "tasks",
    request_body = CreateRemoteTaskRequest, params(("id" = String, Path)),
    responses((status = 201, body = TaskDto), (status = 200, body = TaskDto), (status = 400), (status = 409)))]
pub(super) async fn create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateRemoteTaskRequest>,
) -> ApiResult<(StatusCode, Json<TaskDto>)> {
    source_only(&headers)?;
    if id.is_empty()
        || !id.bytes().all(|b| b.is_ascii_alphanumeric())
        || id.len() > 128
        || req.title.trim().is_empty()
        || req.source_goal.daemon_id.trim().is_empty()
        || req.source_goal.goal_id.trim().is_empty()
        || req.workflow.name.trim().is_empty()
        || req.workflow.steps.is_empty()
    {
        return Err(ApiError::bad_request(
            "provide task identity, source identity, title, and workflow columns",
        ));
    }
    if req.source_goal.daemon_id == state.store.daemon_id().await? {
        return Err(ApiError::bad_request(
            "a remote task must name another source daemon",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for step in &req.workflow.steps {
        if step.id.is_empty() || !seen.insert(&step.id) {
            return Err(ApiError::bad_request(
                "workflow columns need unique nonempty IDs",
            ));
        }
    }
    let request = serde_json::to_string(&req).map_err(|e| ApiError::bad_request(e.to_string()))?;
    // Repeat delivery remains valid after catalogs change or the task ends.
    if let Some(prior) = state.store.remote_task_context(&id).await? {
        if prior.request != request {
            return Err(ApiError::conflict(
                "the task ID already has another creation request; edit the task instead",
            ));
        }
        return Ok((
            StatusCode::OK,
            Json(task_dto_of(&state.store, state.store.get_task(&id).await?).await?),
        ));
    }
    let agents = super::tasks::resolve_agents(&state, &req.agents, false).await?;
    let steps = req
        .workflow
        .steps
        .iter()
        .enumerate()
        .map(|(ordinal, step)| GoalStep {
            goal_id: req.source_goal.goal_id.clone(),
            ordinal: ordinal as i64,
            id: step.id.clone(),
            title: step.title.clone(),
            description: step.description.clone(),
            skills: serde_json::to_string(&step.skills).expect("skill names serialize"),
            ranks: serde_json::to_string(&step.ranks).expect("ranks serialize"),
            gate: step.gate.map(|gate| gate.as_str().to_string()),
        })
        .collect();
    let (task, created) = state
        .store
        .create_remote_task(NewRemoteTask {
            id,
            repo_id: req.repo_id,
            title: req.title,
            description: req.description,
            source_daemon_id: req.source_goal.daemon_id,
            source_goal_id: req.source_goal.goal_id,
            source_title: req.source_goal.title,
            source_description: req.source_goal.description,
            workflow: req.workflow.name,
            steps,
            agents,
            request,
        })
        .await?;
    state.notify_scheduler(&task.id);
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(task_dto_of(&state.store, task).await?),
    ))
}

#[utoipa::path(post, path = "/v1/remote-tasks/{id}/authorize", tag = "tasks",
    request_body = AuthorizeRemoteTaskRequest, params(("id" = String, Path)),
    responses((status = 200, body = TaskDto), (status = 400), (status = 409)))]
pub(super) async fn authorize(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<AuthorizeRemoteTaskRequest>,
) -> ApiResult<Json<TaskDto>> {
    source_only(&headers)?;
    let task = state.store.authorize_remote_task(&id, req.revision).await?;
    state.notify_scheduler(&task.id);
    Ok(Json(task_dto_of(&state.store, task).await?))
}
