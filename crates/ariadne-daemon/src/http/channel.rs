//! What a task's agents say to each other, the diff they say it about, and
//! the request the `pr` column opens.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde_json::json;
use tracing::warn;

use ariadne_api::messages::{MessageDto, MessageListQuery, SendMessageRequest};
use ariadne_api::tasks::{OpenPullRequestRequest, TaskDto};
use ariadne_core::{Actor, MessageKind, Seat, TaskStatus};
use ariadne_store::{MessageFilter, NewMessage, Task};

use super::AppState;
use super::caller::{CallCtx, call_ctx, ensure_task_scope};
use super::convert::{message_dto, task_dto_of};
use super::error::{ApiError, ApiResult, Json};
use crate::forge::ForgeClient;
use crate::stats::session_fact;

/// Git could not answer about the task's branch: a conflict, since what the
/// caller asked for cannot be established rather than being wrong.
fn unresolved(e: impl std::fmt::Display) -> ApiError {
    ApiError::conflict(e.to_string())
}

/// Open the pull or merge request a task lands by, through the repository's
/// own forge CLI, and answer its URL.
///
/// The daemon runs the forge call, never the agent: `gh` or `glab` only ever
/// run here, with the authentication the user already set up for them
/// ([`crate::forge`]). The current column's agent supplies only what the
/// daemon cannot read off the task or the repository — the title and the
/// body — and gets the URL back to show the user.
///
/// One request per task: a task that already has a `pr_url` answers it again
/// and opens nothing, so a retried call never opens a second request.
#[utoipa::path(post, path = "/v1/tasks/{id}/pull-request", tag = "tasks",
    request_body = OpenPullRequestRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 200, body = TaskDto),
        (status = 403, description = "not the current column's agent"),
        (status = 409, description = "the task is not in progress, has no forge or \
                                       its integration is off, is not authenticated, \
                                       or is not pushed")
    ))]
pub(super) async fn open_pull_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<OpenPullRequestRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let task = state.store.get_task(&id).await?;
    super::steps::current_agent(&state, &ctx, &task).await?;
    if task.status() != TaskStatus::InProgress {
        return Err(ApiError::conflict("the task must be in progress"));
    }
    // One request per task: a second call answers the same URL.
    if let Some(url) = task.pr_url.as_deref() {
        record_opened_pull_request(&state, &task, url).await?;
        return Ok(Json(task_dto_of(&state.store, task).await?));
    }

    let repo = state.store.get_repository(&task.repo_id).await?;
    let Some(forge) = repo.forge.as_ref() else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "forge_unavailable",
            "no forge was detected for this repository",
        ));
    };
    // The `pr` column keeps the request it opens, and the integration is
    // what reads the forge and tells it the request's news (026, 030): with
    // it off, nobody would.
    if !state
        .store
        .forge_integration(&task.repo_id)
        .await?
        .is_some_and(|integration| integration.enabled)
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "forge_disabled",
            "the forge integration of this repository is off: a person turns it on, so Ariadne \
             can tell you the news of the request you open",
        ));
    }
    let client = ForgeClient::for_repository(&state.launcher.cfg, forge);
    let refused =
        |message: String| ApiError::new(StatusCode::CONFLICT, "forge_unauthenticated", message);
    client.auth_status(&forge.host).await.map_err(refused)?;

    let repo_path = std::path::PathBuf::from(&repo.path);
    let pushed = state
        .launcher
        .git
        .remote_has_branch_tip(&repo_path, &forge.remote, &task.branch)
        .await
        .map_err(unresolved)?;
    if !pushed {
        return Err(ApiError::conflict(format!(
            "push {} to {} first",
            task.branch, forge.remote
        )));
    }

    // The host rides along: a repository on an enterprise host is opened
    // there, not on the CLI's default one.
    let slug = format!("{}/{}/{}", forge.host, forge.owner, forge.name);
    let url = client
        .open(
            &slug,
            &task.branch,
            &repo.base_branch,
            &req.title,
            &req.body,
            req.draft,
        )
        .await
        .map_err(unresolved)?;

    state.store.set_task_pull_request(&id, &url).await?;
    record_opened_pull_request(&state, &task, &url).await?;
    let task = state.store.get_task(&id).await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

async fn record_opened_pull_request(state: &AppState, task: &Task, url: &str) -> ApiResult<()> {
    let Some(forge) = state
        .store
        .forge_integration(&task.repo_id)
        .await?
        .filter(|f| f.enabled)
    else {
        return Ok(());
    };
    let reference = crate::forge::PullRequestRef::parse(url, &forge)
        .ok_or_else(|| ApiError::conflict("the pull request URL does not match the repository"))?;
    let slug = format!("{}/{}/{}", forge.host, forge.owner, forge.name);
    let pull = ForgeClient::for_repository(&state.launcher.cfg, &forge)
        .pull_request(&slug, reference.number)
        .await
        .map_err(unresolved)?;
    if pull.number != reference.number {
        return Err(ApiError::conflict(
            "the forge returned another request number",
        ));
    }
    let (row, _) =
        crate::forge::pulls::start_work(&state.store, &forge, &pull, Some(task.id.clone()))
            .await
            .map_err(unresolved)?;
    state.launcher.live.set_pull(&row.id, pull, false);
    state.notify_scheduler_pull_request(&row.id);
    Ok(())
}

/// The request a task opened at `url`, read off the forge now (030): its
/// state (`open`, `merged` or `closed`), and the merge and head commits a
/// gate that confirms the merge records against the task.
pub(super) async fn request(
    state: &AppState,
    task: &Task,
    url: &str,
) -> ApiResult<crate::forge::pulls::ForgePullRequest> {
    let forge = state
        .store
        .forge_integration(&task.repo_id)
        .await?
        .ok_or_else(|| ApiError::conflict("the repository has no forge integration"))?;
    let reference = crate::forge::PullRequestRef::parse(url, &forge)
        .ok_or_else(|| ApiError::conflict("the pull request URL does not match the repository"))?;
    let slug = format!("{}/{}/{}", forge.host, forge.owner, forge.name);
    ForgeClient::for_repository(&state.launcher.cfg, &forge)
        .pull_request(&slug, reference.number)
        .await
        .map_err(unresolved)
}

/// The messages of a task: what its agents have said to each other.
#[utoipa::path(get, path = "/v1/tasks/{id}/messages", tag = "tasks",
    params(("id" = String, Path, description = "task id"), MessageListQuery),
    responses((status = 200, body = [MessageDto])))]
pub(super) async fn list_task_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<MessageListQuery>,
) -> ApiResult<Json<Vec<MessageDto>>> {
    let task = state.store.get_task(&id).await?;
    read_channel(
        &state,
        &headers,
        q,
        &task.goal_id,
        MessageFilter {
            task_id: Some(id),
            ..Default::default()
        },
    )
    .await
}

/// One read of a channel, and the one place where a read is also a delivery.
///
/// A plain read answers the filter it was given and changes nothing. A read
/// with `deliver` is the second way a message reaches the agent it is for —
/// the first is the prompt the scheduler hands over — so it obeys the one
/// rule both of them obey: it takes only what is addressed to the calling
/// session's own agent and still undelivered, and it returns only the rows
/// it claimed. A message handed over here is handed over once.
///
/// `goal_id` is the goal the channel belongs to, and a delivering session
/// must be of that goal. A stamp is spent once and cannot be given back, so
/// the seat that is narrowed by nothing but its seat — the orchestrator,
/// which every goal has one of — would otherwise take delivery of another
/// goal's orchestrator's messages by naming one of that goal's tasks.
/// [`ensure_task_scope`] cannot say so: it exempts the orchestrator, which
/// reads and moves every task of its own goal.
pub(super) async fn read_channel(
    state: &AppState,
    headers: &HeaderMap,
    q: MessageListQuery,
    goal_id: &str,
    mut filter: MessageFilter,
) -> ApiResult<Json<Vec<MessageDto>>> {
    if !q.deliver {
        filter.to_agent_id = q.to_agent_id;
        filter.undelivered_only = q.undelivered;
        let rows = state.store.list_messages(filter).await?;
        return Ok(Json(rows.into_iter().map(message_dto).collect()));
    }
    let ctx = call_ctx(&state.store, headers).await?;
    let Some(session) = &ctx.session else {
        return Err(ApiError::bad_request(
            "`deliver` hands the caller its own messages, so it needs an agent session",
        ));
    };
    if session.goal_id.as_deref() != Some(goal_id) {
        return Err(ApiError::forbidden(format!(
            "session {} does not belong to goal {goal_id}",
            session.id
        )));
    }
    // Narrowed to the caller itself, never to what the caller asked for: a
    // delivery stamps what it returns, and no agent may spend another's.
    filter.undelivered_only = true;
    match session.seat() {
        Some(Seat::Orchestrator) => filter.to_actor = Some(Actor::Orchestrator),
        _ => {
            let Some(agent_id) = session.task_agent_id.clone() else {
                return Err(ApiError::bad_request(format!(
                    "session {} is staffed on no agent, so no message is addressed to it",
                    session.id
                )));
            };
            filter.to_agent_id = Some(agent_id);
        }
    }
    let waiting = state.store.list_messages(filter).await?;
    let mut handed = Vec::with_capacity(waiting.len());
    for message in waiting {
        // A claim, not a plain stamp: the runtime may claim the same row in
        // the same instant to send it as a prompt, and only the winner hands
        // it over.
        if !state.store.mark_message_delivered(&message.id).await? {
            continue;
        }
        // Read back rather than answered as it was read: the row the caller
        // is given carries the stamp this call put on it.
        handed.push(state.store.get_message(&message.id).await?);
    }
    Ok(Json(handed.into_iter().map(message_dto).collect()))
}

/// Send a message about a task.
///
/// Who it is from is the session header's, never the body's: an agent cannot
/// write as somebody else, and a call with no session behind it is the user
/// speaking.
#[utoipa::path(post, path = "/v1/tasks/{id}/messages", tag = "tasks",
    request_body = SendMessageRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 201, body = MessageDto), (status = 403), (status = 409)))]
pub(super) async fn post_task_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<SendMessageRequest>,
) -> ApiResult<(StatusCode, Json<MessageDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let task = state.store.get_task(&id).await?;
    let message = send(&state, &ctx, &task.goal_id, Some(&task), req).await?;
    state.notify_scheduler(&id);
    Ok((StatusCode::CREATED, Json(message_dto(message))))
}

/// One message written, whoever wrote it and whatever it is about.
///
/// One thing is checked here, and it is the one an agent can get wrong: a
/// recipient that is not on this task. Everything else the channel carries
/// as it stands — it is a conversation, and the daemon is not in it.
pub(super) async fn send(
    state: &AppState,
    ctx: &CallCtx,
    goal_id: &str,
    task: Option<&Task>,
    req: SendMessageRequest,
) -> ApiResult<ariadne_store::Message> {
    let body = req.body.trim();
    if body.is_empty() {
        return Err(ApiError::bad_request("a message needs a body"));
    }
    let (to_actor, to_agent_id) = (req.to_actor, req.to_agent_id.clone());
    match to_actor {
        Actor::Orchestrator => {
            if to_agent_id.is_some() {
                return Err(ApiError::bad_request(
                    "the orchestrator is staffed on no task, so it has no agent id",
                ));
            }
            if !state.store.get_goal(goal_id).await?.orchestrated {
                return Err(ApiError::conflict(
                    "this goal has no orchestrator; the user answers in the console",
                ));
            }
        }
        Actor::Agent => {
            let Some(agent_id) = &to_agent_id else {
                return Err(ApiError::bad_request(
                    "a message to an agent needs the to_agent_id `get_task` lists",
                ));
            };
            let task = task
                .ok_or_else(|| ApiError::bad_request("a message to a task's agent needs a task"))?;
            let staffed = state.store.list_task_agents(&task.id).await?;
            if !staffed.iter().any(|a| a.id == *agent_id) {
                return Err(ApiError::bad_request(format!(
                    "agent {agent_id} is not staffed on task {}",
                    task.id
                )));
            }
        }
        Actor::Daemon | Actor::User => {
            return Err(ApiError::bad_request(
                "messages go to the orchestrator or to a task's agents",
            ));
        }
    }

    let from_agent_id = ctx.session.as_ref().and_then(|s| s.task_agent_id.clone());
    let message = state
        .store
        .send_message(NewMessage {
            goal_id: goal_id.to_string(),
            task_id: task.map(|t| t.id.clone()),
            kind: MessageKind::Message,
            from_actor: ctx.actor,
            from_agent_id,
            from_session: ctx.session.as_ref().map(|s| s.id.clone()),
            to_actor,
            to_agent_id,
            body: body.to_string(),
        })
        .await?;
    record_message_fact(state, ctx, task, &message).await;
    Ok(message)
}

/// Record every stored task message. A sender session supplies the model and
/// seat; messages from the user or daemon still keep their task and goal.
async fn record_message_fact(
    state: &AppState,
    ctx: &CallCtx,
    task: Option<&Task>,
    message: &ariadne_store::Message,
) {
    let data = json!({ "kind": message.kind, "from_actor": message.from_actor, "to_actor": message.to_actor });
    let fact = match ctx.session.as_ref() {
        Some(session) => session_fact(&state.store, &session.id, "message", data).await,
        None => Ok(ariadne_store::NewStatFact {
            kind: "message".into(),
            repo_id: task.map(|t| t.repo_id.clone()),
            goal_id: Some(message.goal_id.clone()),
            task_id: message.task_id.clone(),
            session_id: None,
            launch_id: None,
            seat: None,
            model: None,
            effort: None,
            skills: vec![],
            data,
        }),
    };
    match fact {
        Ok(fact) => match state.store.record_fact(fact).await {
            Ok(()) => {}
            Err(error) => warn!(error = %error, "could not record a message fact"),
        },
        Err(error) => warn!(error = %error, "could not record a message fact"),
    }
}

/// Diff of the task branch against its base (`git diff base...branch`), or,
/// once the task is merged, the diff its merge commit brought into the base —
/// after the merge the branch is contained in the base, so the three-dot diff
/// would be forever empty.
#[utoipa::path(get, path = "/v1/tasks/{id}/diff", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, content_type = "text/plain", body = String), (status = 404), (status = 409)))]
pub(super) async fn diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<String> {
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
            .map_err(unresolved);
    }

    if !state
        .launcher
        .git
        .branch_exists(&repo_path, &task.branch)
        .await
        .map_err(unresolved)?
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
        .map_err(unresolved)
}
