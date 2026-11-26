//! Source coordination for tasks owned by a persistent worker.
use ariadne_api::tasks::{
    CreateRemoteTaskRequest, SourceGoalReference, TaskDto, TaskWorkflowContext,
};
use ariadne_core::id::new_id;
use ariadne_store::{Goal, SourceTaskReference, Store, StoreError, Task};
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{
    AppState,
    error::{ApiError, ApiResult, Json},
};

pub(super) async fn create(
    state: &AppState,
    goal: Option<&Goal>,
    workflow: TaskWorkflowContext,
    repo_id: &str,
    host: &str,
    request: ariadne_api::tasks::CreateTaskRequest,
) -> ApiResult<(StatusCode, Json<TaskDto>)> {
    let ariadne_api::tasks::CreateTaskRequest {
        title,
        description,
        agents,
        depends_on: dependencies,
        ..
    } = request;
    let mut connection = Connection::new(state, host);
    let client = super::tasks::far_client(state, host)?;
    let protocol: ariadne_api::tasks::RemoteTaskProtocol = client
        .get_json("/v1/remote-tasks/protocol")
        .await
        .map_err(|error| match &error {
            ariadne_client::ClientError::Api { status, .. }
                if *status == StatusCode::NOT_FOUND
                    || *status == StatusCode::METHOD_NOT_ALLOWED =>
            {
                ApiError::conflict(format!(
                    "upgrade Ariadne on host {host}: remote-tasks-v1 is required"
                ))
            }
            ariadne_client::ClientError::Decode(_) => ApiError::conflict(format!(
                "upgrade Ariadne on host {host}: remote-tasks-v1 is required"
            )),
            _ => super::tasks::far_error(host, error),
        })?;
    if !protocol
        .capabilities
        .iter()
        .any(|c| c == ariadne_api::tasks::SOURCE_DEPENDENCY_CONTROLS)
        || protocol.protocol != "remote-tasks-v1"
        || protocol.daemon_id.is_empty()
        || protocol.daemon_id == state.store.daemon_id().await?
    {
        return Err(ApiError::conflict(format!(
            "upgrade or correct host {host}: a distinct remote-tasks-v1 worker with source-dependency-controls-v1 is required"
        )));
    }
    if goal.is_none()
        && !protocol
            .capabilities
            .iter()
            .any(|c| c == "goal-less-tasks-v1")
    {
        return Err(ApiError::conflict(format!(
            "upgrade Ariadne on host {host}: goal-less-tasks-v1 is required"
        )));
    }
    let local = state
        .store
        .list_repositories()
        .await?
        .into_iter()
        .map(super::convert::repository_dto)
        .collect();
    let paired = crate::hosts::host_repositories(local, &client)
        .await
        .map_err(|e| super::tasks::far_error(host, e))?;
    let worker_repo = paired
        .get(repo_id)
        .ok_or_else(|| ApiError::conflict("worker repository is unavailable"))?;
    if let Some(goal) = goal {
        check_active_staffing(&state.store, goal, &agents).await?;
    }
    let source_id = state.store.daemon_id().await?;
    let request = CreateRemoteTaskRequest {
        repo_id: worker_repo.clone(),
        title: title.clone(),
        description: description.clone(),
        agents,
        source_daemon_id: goal.is_none().then_some(source_id.clone()),
        source_goal: goal.map(|goal| SourceGoalReference {
            daemon_id: source_id.clone(),
            goal_id: goal.id.clone(),
            title: goal.title.clone(),
            description: goal.description.clone(),
        }),
        workflow,
    };
    let id = new_id();
    let timestamp = chrono::Utc::now().to_rfc3339();
    let cache = json!({"id":id,"goal_id":goal.map(|g| &g.id),"repo_id":repo_id,"title":title,"description":description,
        "workflow": (!request.workflow.name.is_empty()).then_some(&request.workflow.name), "steps": request.workflow.steps,
        "status":"pending","step":null,"branch":"","agents":[],"depends_on":dependencies,
        "worktree_path":null,"stalled":false,"merge_commit":null,"pr_url":null,"reason":null,
        "usage":{"total":{"input_tokens":0,"cached_input_tokens":0,"output_tokens":0},"agents":[]},
        "created_at":timestamp,"updated_at":timestamp});
    state
        .store
        .reserve_source_task(
            SourceTaskReference {
                id: id.clone(),
                goal_id: goal.map(|g| g.id.clone()).unwrap_or_default(),
                repo_id: repo_id.into(),
                host: host.into(),
                worker_id: protocol.daemon_id.clone(),
                request: Some(serde_json::to_string(&request).unwrap()),
                cache: cache.to_string(),
                stale: 1,
                cancel_requested: 0,
                retry_requested: 0,
                dependency_failure: None,
                failure_operation_id: None,
                failure_revision: None,
                authorization_revision: None,
                progress_id: None,
                progress_at: None,
                created_at: timestamp.clone(),
                updated_at: timestamp,
            },
            &dependencies,
        )
        .await?;
    // A lost response leaves the reservation for the host follower to replay.
    let response: Result<TaskDto, _> = client
        .put_json(&format!("/v1/remote-tasks/{id}"), &request)
        .await;
    if let Err(ariadne_client::ClientError::Api { status, .. }) = &response
        && status.is_client_error()
    {
        state.store.remove_source_task_reference(&id).await?;
        return Err(super::tasks::far_error(host, response.unwrap_err()));
    }
    if let Ok(task) = response
        && connection.current(state)
    {
        state
            .store
            .cache_source_task(
                &id,
                &protocol.daemon_id,
                &serde_json::to_value(task).unwrap(),
            )
            .await?;
    }
    state.notify_scheduler(&id);
    Ok((
        StatusCode::CREATED,
        Json(project(&state.store, &state.store.get_task(&id).await?).await?),
    ))
}

pub(crate) async fn project(store: &Store, task: &Task) -> Result<TaskDto, StoreError> {
    let reference = store
        .source_task_reference(&task.id)
        .await?
        .ok_or_else(|| StoreError::Invalid("missing source reference".into()))?;
    let mut dto: TaskDto =
        serde_json::from_str(&reference.cache).map_err(|e| StoreError::Invalid(e.to_string()))?;
    dto.host = reference.host;
    dto.repo_id = reference.repo_id;
    dto.host_unreachable = reference.stale != 0;
    dto.depends_on = store.list_task_dependencies(&task.id).await?;
    for agent in &mut dto.agents {
        if let Some(session) = &mut agent.session_id {
            *session = format!("far~{}~{session}", task.id);
        }
    }
    Ok(dto)
}

pub(super) async fn forward(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match route(&state, request, next).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn route(
    state: &AppState,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let path = request.uri().path().to_string();
    let target = request
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(&path)
        .to_string();
    if let Some(rest) = path.strip_prefix("/v1/sessions/far~")
        && let Some((task_id, rest)) = rest.split_once('~')
    {
        let (session_id, suffix) = rest.split_once('/').unwrap_or((rest, ""));
        if let Some(reference) = state.store.source_task_reference(task_id).await? {
            if suffix == "switch" {
                return Ok(next.run(request).await);
            }
            return super::source_sessions::route(state, request, reference, session_id, suffix)
                .await;
        }
    }
    let Some(rest) = path.strip_prefix("/v1/tasks/") else {
        return Ok(next.run(request).await);
    };
    let (id, suffix) = rest
        .split_once('/')
        .map_or((rest, ""), |(id, tail)| (id, tail));
    let Some(reference) = state.store.source_task_reference(id).await? else {
        return Ok(next.run(request).await);
    };
    // A post keeps the rules of 039, and its route sends it to the worker.
    if suffix == "messages"
        || suffix == "posts"
        || suffix.is_empty() && request.method() == axum::http::Method::GET
    {
        return Ok(next.run(request).await);
    }
    let ctx = super::caller::call_ctx(&state.store, request.headers()).await?;
    super::caller::ensure_task_scope(&ctx, id)?;
    if ctx
        .session
        .as_ref()
        .is_some_and(|s| s.goal_id.as_deref() != Some(&reference.goal_id))
    {
        return Err(ApiError::forbidden("task belongs to another goal"));
    }
    if !matches!(
        ctx.actor,
        ariadne_core::Actor::User | ariadne_core::Actor::Orchestrator
    ) && request.method() != axum::http::Method::GET
    {
        return Err(ApiError::forbidden(
            "only the source orchestrator or user may control remote tasks",
        ));
    }
    if suffix == "cancel" && request.method() == axum::http::Method::POST {
        let task = state.store.request_source_cancel(id).await?;
        state.notify_scheduler(id);
        return Ok(Json(project(&state.store, &task).await?).into_response());
    }
    if suffix == "retry" && request.method() == axum::http::Method::POST {
        let task = state.store.request_source_retry(id).await?;
        state.notify_scheduler(id);
        return Ok(Json(project(&state.store, &task).await?).into_response());
    }
    if suffix == "transitions" && request.method() == axum::http::Method::POST {
        let bytes = axum::body::to_bytes(request.into_body(), 2 * 1024 * 1024)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        let transition: ariadne_api::tasks::TransitionRequest =
            serde_json::from_slice(&bytes).map_err(|e| ApiError::bad_request(e.to_string()))?;
        let task = match transition.to {
            ariadne_core::TaskStatus::Cancelled => state.store.request_source_cancel(id).await?,
            ariadne_core::TaskStatus::Ready => state.store.request_source_retry(id).await?,
            _ => {
                return Err(ApiError::forbidden(
                    "use worker agent step operations to advance execution",
                ));
            }
        };
        state.notify_scheduler(id);
        return Ok(Json(project(&state.store, &task).await?).into_response());
    }
    let request = if suffix.is_empty() && request.method() == axum::http::Method::PATCH {
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, 2 * 1024 * 1024)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        let edit: ariadne_api::tasks::UpdateTaskRequest =
            serde_json::from_slice(&bytes).map_err(|e| ApiError::bad_request(e.to_string()))?;
        if edit.agents.is_none()
            && edit.title.is_none()
            && edit.description.is_none()
            && let Some(dependencies) = edit.depends_on
        {
            state.store.set_task_dependencies(id, &dependencies).await?;
            state.notify_scheduler(id);
            return Ok(
                Json(project(&state.store, &state.store.get_task(id).await?).await?)
                    .into_response(),
            );
        }
        axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes))
    } else {
        request
    };
    let mut connection = Connection::new(state, &reference.host);
    let client = super::tasks::far_client(state, &reference.host)?;
    let protocol: ariadne_api::tasks::RemoteTaskProtocol = client
        .get_json("/v1/remote-tasks/protocol")
        .await
        .map_err(|e| super::tasks::far_error(&reference.host, e))?;
    if protocol.daemon_id != reference.worker_id {
        return Err(ApiError::conflict(
            "host now names a different worker; restore the original worker",
        ));
    }
    let method = request.method().clone();
    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut bytes = axum::body::to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?
        .to_vec();
    if suffix.is_empty() && method == axum::http::Method::PATCH {
        let update: ariadne_api::tasks::UpdateTaskRequest =
            serde_json::from_slice(&bytes).map_err(|e| ApiError::bad_request(e.to_string()))?;
        if let Some(assignments) = &update.agents {
            if !reference.goal_id.is_empty() {
                let goal = state.store.get_goal(&reference.goal_id).await?;
                check_active_staffing(&state.store, &goal, assignments).await?;
            }
            let repo = state.store.get_repository(&reference.repo_id).await?;
            super::tasks::check_host(
                state,
                &reference.host,
                &repo,
                assignments,
                ctx.actor != ariadne_core::Actor::User,
            )
            .await?;
        }
        let mut edit: Value =
            serde_json::from_slice(&bytes).map_err(|e| ApiError::bad_request(e.to_string()))?;
        if reference.authorization_revision.is_some()
            && !serde_json::from_str::<TaskDto>(&reference.cache)
                .map_err(|e| ApiError::conflict(e.to_string()))?
                .execution_context
                .is_some_and(|c| c.authorized)
        {
            return Err(ApiError::conflict(
                "worker authorization is pending; read the task again before editing",
            ));
        }
        if let Some(dependencies) = edit.as_object_mut().and_then(|o| o.remove("depends_on")) {
            if reference.authorization_revision.is_some() {
                return Err(ApiError::conflict(
                    "dependencies cannot change after execution authorization",
                ));
            }
            let dependencies: Vec<String> = serde_json::from_value(dependencies)
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            state.store.set_task_dependencies(id, &dependencies).await?;
        }
        if edit.as_object().is_some_and(|o| o.is_empty()) {
            state.notify_scheduler(id);
            return Ok(
                Json(project(&state.store, &state.store.get_task(id).await?).await?)
                    .into_response(),
            );
        }
        bytes = serde_json::to_vec(&edit).unwrap();
    }
    let response = client
        .forward(method, &target, content_type.as_deref(), bytes)
        .await
        .map_err(|e| super::tasks::far_error(&reference.host, e))?;
    if (200..300).contains(&response.status)
        && connection.current(state)
        && let Ok(task) = serde_json::from_slice::<TaskDto>(&response.body)
    {
        state
            .store
            .cache_source_task(
                id,
                &reference.worker_id,
                &serde_json::to_value(task).unwrap(),
            )
            .await?;
        state.notify_scheduler(id);
        return Ok(
            Json(project(&state.store, &state.store.get_task(id).await?).await?).into_response(),
        );
    }
    let body = response.body;
    let mut answer = (StatusCode::from_u16(response.status).unwrap(), body).into_response();
    if let Some(content_type) = response.content_type {
        answer
            .headers_mut()
            .insert("content-type", content_type.parse().unwrap());
    }
    Ok(answer)
}

/// An HTTP response from a previous host connection cannot refresh source progress.
struct Connection {
    host: String,
    endpoint: Option<String>,
    events: tokio::sync::broadcast::Receiver<crate::bus::BusEvent>,
}

impl Connection {
    fn new(state: &AppState, host: &str) -> Self {
        let events = state.events.subscribe();
        let endpoint = state
            .hosts
            .list()
            .iter()
            .find(|h| h.name == host)
            .and_then(crate::hosts::ready_endpoint);
        Self {
            host: host.into(),
            endpoint,
            events,
        }
    }

    fn current(&mut self, state: &AppState) -> bool {
        use ariadne_api::stream::DomainEvent;
        loop {
            match self.events.try_recv() {
                Ok(event) => match event.event {
                    DomainEvent::HostUpdated(host) if host.name == self.host => {
                        if crate::hosts::ready_endpoint(&host) != self.endpoint {
                            return false;
                        }
                    }
                    DomainEvent::HostDeleted(host) if host.name == self.host => return false,
                    _ => {}
                },
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(_) => return false,
            }
        }
        self.endpoint.is_some()
            && state
                .hosts
                .list()
                .iter()
                .find(|h| h.name == self.host)
                .and_then(crate::hosts::ready_endpoint)
                == self.endpoint
    }
}

async fn check_active_staffing(
    store: &Store,
    goal: &Goal,
    agents: &[ariadne_api::tasks::AgentAssignment],
) -> ApiResult<()> {
    if goal.status() != ariadne_core::GoalStatus::Active {
        return Ok(());
    }
    let steps = store.goal_steps(&goal.id).await?;
    let staffed: Vec<String> = agents.iter().filter_map(|a| a.step.clone()).collect();
    let missing = ariadne_store::unstaffed_columns(&steps, &staffed);
    if !missing.is_empty() {
        return Err(ApiError::conflict(format!(
            "staff every column before adding active work: {}",
            missing.join(", ")
        )));
    }
    Ok(())
}
