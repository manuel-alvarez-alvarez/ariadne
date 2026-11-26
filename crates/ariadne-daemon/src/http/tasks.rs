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
use ariadne_store::{Goal, NewTask, NewTaskAgent, Task, TaskFilter, TaskUpdate};

use super::AppState;
use super::caller::{CallCtx, call_ctx, ensure_task_scope};
use super::convert::{task_dto_of, transition_dto};
use super::error::{ApiError, ApiResult, Json};
use super::pins;

fn check_dependencies(goal: &Goal, depends_on: &[String]) -> ApiResult<()> {
    if goal.origin_goal_id.is_some() && !depends_on.is_empty() {
        return Err(ApiError::bad_request(
            "a linked goal receives tasks after their dependencies merge",
        ));
    }
    Ok(())
}

/// Refuse a far task that its host cannot run (037): a host that is unknown
/// or not `ready`, a repository that the host lacks, or a model that the
/// host's `GET /v1/models` does not list. Where `staffed`, the orchestrator
/// staffs the task, and a model that the host turned off is refused too.
pub(super) async fn check_host(
    state: &AppState,
    host: &str,
    repo: &ariadne_store::Repository,
    agents: &[AgentAssignment],
    staffed: bool,
) -> ApiResult<()> {
    let Some(dto) = state.hosts.list().into_iter().find(|h| h.name == host) else {
        return Err(ApiError::bad_request(format!(
            "unknown host {host}; `GET /v1/hosts` lists the hosts"
        )));
    };
    let endpoint = match (dto.status, crate::hosts::ready_endpoint(&dto)) {
        (ariadne_api::hosts::HostStatus::Ready, Some(endpoint)) => endpoint,
        (status, _) => {
            return Err(ApiError::conflict(format!(
                "host {host} is {}; a far task needs a ready host",
                format!("{status:?}").to_lowercase()
            )));
        }
    };
    let client = crate::hosts::client_for(&endpoint);
    let unanswered = |error: ariadne_client::ClientError| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "host_unreachable",
            format!("host {host} did not answer: {error}"),
        )
    };
    let local = state
        .store
        .list_repositories()
        .await?
        .into_iter()
        .map(super::convert::repository_dto)
        .collect();
    let paired = crate::hosts::host_repositories(local, &client)
        .await
        .map_err(unanswered)?;
    if !paired.contains_key(&repo.id) {
        return Err(ApiError::conflict(crate::far::lacks_repository(host, repo)));
    }
    let models: Vec<ariadne_api::models::ModelDto> =
        client.get_json("/v1/models").await.map_err(unanswered)?;
    for agent in agents {
        match models.iter().find(|model| model.id == agent.model) {
            None => {
                return Err(ApiError::bad_request(format!(
                    "host {host} does not list the model {}; staff the task on a model of \
                     `GET /v1/models` on that host",
                    agent.model
                )));
            }
            // A model the host turned off is the user's choice alone (011
            // rule 19): automatic staffing applies the host's switch.
            Some(model) if staffed && !model.enabled => {
                return Err(ApiError::bad_request(format!(
                    "host {host} has the model {} turned off; staff the task on a model that \
                     is on there, or turn it back on first",
                    agent.model
                )));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// The agents of a far task, pinned as the request names them. The host's
/// catalog, not this daemon's, decides which models a far task can run on
/// ([`check_host`]), and the host checks each pin again when it gets the task.
fn far_agents(assignments: &[AgentAssignment]) -> ApiResult<Vec<NewTaskAgent>> {
    assignments
        .iter()
        .map(|assignment| {
            let model: ariadne_core::models::ModelRef =
                assignment.model.parse().map_err(ApiError::bad_request)?;
            Ok(NewTaskAgent {
                step: assignment.step.clone(),
                skills: assignment.skills.clone(),
                pin: ariadne_store::AgentPin {
                    model: model.to_string(),
                    effort: assignment
                        .effort
                        .clone()
                        .filter(|effort| !["", "default"].contains(&effort.as_str())),
                },
                brief: assignment.brief.clone(),
            })
        })
        .collect()
}

/// The agents an assignment list asks for, in the order it names them.
///
/// An agent has nothing behind it to fall back to: every assignment names its
/// CLI and its model, and one that names no model is refused.
pub(super) async fn resolve_agents(
    state: &AppState,
    assignments: &[AgentAssignment],
    manual: bool,
) -> ApiResult<Vec<NewTaskAgent>> {
    let mut agents = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        agents.push(NewTaskAgent {
            step: assignment.step.clone(),
            skills: assignment.skills.clone(),
            pin: if manual {
                pins::chosen(
                    &state.store,
                    &state.agent_registry,
                    Some(&assignment.model),
                    assignment.effort.as_deref(),
                )
                .await?
            } else {
                pins::staffed(
                    &state.store,
                    &state.agent_registry,
                    Some(&assignment.model),
                    assignment.effort.as_deref(),
                )
                .await?
            },
            brief: assignment.brief.clone(),
        });
    }
    Ok(agents)
}

/// Create a task in a goal (orchestrator via MCP, or the user).
#[utoipa::path(post, path = "/v1/goals/{goal_id}/tasks", tag = "tasks",
    request_body = CreateTaskRequest,
    params(("goal_id" = String, Path, description = "goal id")),
    responses((status = 201, body = TaskDto),
        (status = 200, body = TaskDto,
            description = "the task the linked goal already holds for `origin_task_id`"),
        (status = 400), (status = 409),
        (status = 502, description = "the host of a far task did not answer")))]
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
    check_dependencies(&goal, &req.depends_on)?;
    if req.origin_task_id.is_some() && goal.origin_goal_id.is_none() {
        return Err(ApiError::bad_request(
            "origin_task_id names the task of a linked goal; this goal is not linked",
        ));
    }
    // A far task of a linked goal would lose its replay key (037).
    if req.origin_task_id.is_some()
        && req
            .host
            .as_deref()
            .is_some_and(|host| host != ariadne_api::tasks::LOCAL_HOST)
    {
        return Err(ApiError::bad_request(
            "origin_task_id and a host other than local cannot go together: the task of a \
             linked goal runs on this daemon",
        ));
    }
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

    // `local` names the goal's own daemon, as an omitted host does.
    let host = req
        .host
        .clone()
        .filter(|host| host != ariadne_api::tasks::LOCAL_HOST);
    let agents = match &host {
        Some(host) => {
            let repo = state.store.get_repository(&repo_id).await?;
            check_host(&state, host, &repo, &req.agents, ctx.actor != Actor::User).await?;
            far_agents(&req.agents)?
        }
        None => resolve_agents(&state, &req.agents, ctx.actor == Actor::User).await?,
    };

    if let Some(host) = &host {
        return super::source_tasks::create(&state, &goal, &repo_id, host, req).await;
    }
    let new = NewTask {
        goal_id: goal.id,
        repo_id,
        title: req.title,
        description: req.description,
        agents,
        depends_on: req.depends_on,
    };
    let (task, created) = match (&host, &req.origin_task_id) {
        (Some(host), _) => (state.store.create_far_task(new, host).await?, true),
        // The task the goal holds for the origin task answers a repeated
        // create, so a lost answer makes no second task (037).
        (None, Some(origin)) => state.store.create_linked_task(new, origin).await?,
        (None, None) => (state.store.create_task(new).await?, true),
    };
    if created && goal.origin_goal_id.is_some() {
        state.notify_scheduler(&task.id);
    }
    let dto = task_dto_of(&state.store, task).await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(dto)))
}

/// List tasks.
#[utoipa::path(get, path = "/v1/tasks", tag = "tasks",
    params(TaskListQuery),
    responses((status = 200, body = [TaskDto])))]
pub(super) async fn list(
    State(state): State<AppState>,
    Query(q): Query<TaskListQuery>,
) -> ApiResult<Json<Vec<TaskDto>>> {
    let mut tasks = match &q.origin_task_id {
        Some(origin) => state
            .store
            .linked_task_of(origin)
            .await?
            .into_iter()
            .collect(),
        None => {
            state
                .store
                .list_tasks(TaskFilter {
                    goal_id: q.goal.clone(),
                    status: q.status,
                })
                .await?
        }
    };
    tasks.retain(|task| {
        q.goal
            .as_ref()
            .is_none_or(|goal| task.goal_id.as_ref() == Some(goal))
            && (!q.no_goal || task.goal_id.is_none())
            && q.status.is_none_or(|status| task.status() == status)
    });
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
    if state.store.source_task_reference(&id).await?.is_some() {
        return Ok(Json(task_dto_of(&state.store, task).await?));
    }
    let mut dto = task_dto_of(&state.store, task.clone()).await?;
    if let (Some(host), Some(far_id)) = (&task.host, &task.far_task_id)
        && let Ok(client) = far_client(&state, host)
        && let Ok(far) = client
            .get_json::<TaskDto>(&format!("/v1/tasks/{far_id}"))
            .await
    {
        for agent in &mut dto.agents {
            if let Some(other) = far.agents.iter().find(|other| other.step == agent.step) {
                agent.model.clone_from(&other.model);
                agent.effort.clone_from(&other.effort);
                agent.session_id = other
                    .session_id
                    .as_ref()
                    .map(|session| format!("far~{id}~{session}"));
            }
        }
    }
    Ok(Json(dto))
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
    if let Some(deps) = &req.depends_on
        && !deps.is_empty()
    {
        let task = state.store.get_task(&id).await?;
        let goal = state.store.task_goal(&task).await?;
        check_dependencies(&goal, deps)?;
    }
    let current = state.store.get_task(&id).await?;
    check_unsent_host(&state, &current).await?;
    if current.goal_id.is_none() && req.depends_on.as_ref().is_some_and(|d| !d.is_empty()) {
        return Err(ApiError::bad_request(
            "a goal-less task takes no dependencies",
        ));
    }
    let agents = match &req.agents {
        Some(a) if current.host.is_some() => Some(far_agents(a)?),
        Some(a) => Some(resolve_agents(&state, a, ctx.actor == Actor::User).await?),
        None => None,
    };
    if let (Some(host), Some(far_id)) = (&current.host, &current.far_task_id) {
        if let Some(assignments) = &req.agents {
            let repo = state.store.get_repository(&current.repo_id).await?;
            check_host(&state, host, &repo, assignments, ctx.actor != Actor::User).await?;
        }
        let client = far_client(&state, host)?;
        let mut remote = req.clone();
        remote.depends_on = None;
        let _: TaskDto = client
            .patch_json(&format!("/v1/tasks/{far_id}"), &remote)
            .await
            .map_err(|error| far_error(host, error))?;
    }
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

pub(super) fn far_client(state: &AppState, host: &str) -> ApiResult<ariadne_client::Client> {
    let dto = state
        .hosts
        .list()
        .into_iter()
        .find(|item| item.name == host)
        .ok_or_else(|| ApiError::conflict(format!("host {host} is unknown")))?;
    let endpoint = crate::hosts::ready_endpoint(&dto).ok_or_else(|| {
        ApiError::conflict(format!(
            "host {host} is {}",
            format!("{:?}", dto.status).to_lowercase()
        ))
    })?;
    Ok(crate::hosts::client_for(&endpoint))
}

async fn check_unsent_host(state: &AppState, task: &Task) -> ApiResult<()> {
    if let Some(host) = &task.host
        && task.far_task_id.is_none()
        && state.store.task_dependencies_merged(&task.id).await?
    {
        far_client(state, host)?;
    }
    Ok(())
}

pub(super) fn far_error(host: &str, error: ariadne_client::ClientError) -> ApiError {
    match error {
        ariadne_client::ClientError::Api {
            status,
            code,
            message,
            ..
        } => ApiError::new(status, &code, message),
        error => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "host_unreachable",
            format!("host {host} did not answer: {error}"),
        ),
    }
}

async fn far_transition(state: &AppState, task: &Task, action: &str) -> ApiResult<Task> {
    let host = task.host.as_deref().expect("far task has a host");
    let far_id = task.far_task_id.as_deref().expect("sent task has an id");
    let far: TaskDto = far_client(state, host)?
        .post_empty(&format!("/v1/tasks/{far_id}/{action}"))
        .await
        .map_err(|error| far_error(host, error))?;
    state
        .store
        .mirror_far_task(
            &task.id,
            &ariadne_store::FarTaskState {
                status: far.status,
                step: far.step,
                branch: far.branch,
                pr_url: far.pr_url,
                merge_commit: far.merge_commit,
                reason: far.reason,
            },
        )
        .await?;
    state.notify_scheduler(&task.id);
    Ok(state.store.get_task(&task.id).await?)
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

/// Whether `sha` could name a commit: hex digits only, 7 to 64 characters —
/// a short sha up through a full one. Checked before a `merge_commit` ever
/// reaches a git command as data: a value git could read as an option, or
/// anything that is not a sha, is refused here instead.
pub(super) fn valid_merge_commit(sha: &str) -> bool {
    (7..=64).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(super) fn invalid_merge_commit(sha: &str) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request",
        format!("merge_commit must be a hex sha of 7 to 64 characters (got `{sha}`)"),
    )
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
    if let Some(sha) = req.merge_commit.as_deref()
        && !valid_merge_commit(sha)
    {
        return Err(invalid_merge_commit(sha));
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
    if matches!(ctx.actor, Actor::Orchestrator | Actor::User) {
        let task = state.store.get_task(&id).await?;
        check_unsent_host(&state, &task).await?;
        if task.far_task_id.is_some() {
            let task = far_transition(&state, &task, "cancel").await?;
            return Ok(Json(task_dto_of(&state.store, task).await?));
        }
    }
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
    if matches!(ctx.actor, Actor::Orchestrator | Actor::User) {
        let task = state.store.get_task(&id).await?;
        check_unsent_host(&state, &task).await?;
        if task.far_task_id.is_some() {
            let task = far_transition(&state, &task, "retry").await?;
            return Ok(Json(task_dto_of(&state.store, task).await?));
        }
    }
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

/// Create a local task without a goal. Only the user can create one.
#[utoipa::path(post, path = "/v1/tasks", tag = "tasks",
    request_body = ariadne_api::tasks::CreateStandaloneTaskRequest,
    responses((status = 201, body = TaskDto), (status = 400), (status = 403), (status = 404)))]
pub(super) async fn create_standalone(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ariadne_api::tasks::CreateStandaloneTaskRequest>,
) -> ApiResult<(StatusCode, Json<TaskDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if ctx.actor != Actor::User {
        return Err(ApiError::forbidden(
            "only the user may create a goal-less task",
        ));
    }
    if req.host.as_deref().is_some_and(|host| host != "local") {
        return Err(ApiError::bad_request(
            "a goal-less task requires the local host",
        ));
    }
    let agents = resolve_agents(&state, &req.agents, true).await?;
    let task = state
        .store
        .create_standalone_task(
            &req.repo_id,
            &req.title,
            &req.description,
            &req.workflow,
            agents,
        )
        .await?;
    state.notify_scheduler(&task.id);
    Ok((
        StatusCode::CREATED,
        Json(task_dto_of(&state.store, task).await?),
    ))
}
