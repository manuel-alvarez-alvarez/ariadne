//! Text from one agent put straight into the session of another (spec 039).
//!
//! A post is a prompt in a live session and nothing else: it writes no row,
//! answers no pending permission or question, and clears no attention.
//!
//! A post crosses to a worker-protocol task on another host (037 rule 101).
//! The source orchestrator's post goes to the worker's
//! `POST /v1/remote-tasks/{id}/posts`, and the worker applies the rules of a
//! local post. A worker agent's post to `orchestrator` goes out as a
//! `source_post` frame on the event stream that the source holds open on the
//! worker ([`crate::source_posts`]). No daemon holds a post for later.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::messages::{PostToSessionRequest, RemotePostRequest, SourcePostDto};
use ariadne_core::Seat;
use ariadne_store::{AgentSession, SessionFilter, SourceTaskReference, Task};

use super::AppState;
use super::caller::{call_ctx, ensure_task_scope};
use super::error::{ApiError, ApiResult, Json};
use crate::agents::prompts;

/// Put the body into the live session of the orchestrator or of the agent
/// of one column, on one task of this host or a worker-protocol task on
/// another.
///
/// Each refusal is a bad request with the sentence the caller reads.
#[utoipa::path(post, path = "/v1/tasks/{id}/posts", tag = "tasks",
    request_body = PostToSessionRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 204), (status = 400), (status = 403)))]
pub(super) async fn post_to_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<PostToSessionRequest>,
) -> ApiResult<StatusCode> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let Some(sender) = &ctx.session else {
        return Err(ApiError::bad_request(
            "a post is from an agent session, and this call has none",
        ));
    };
    if !matches!(sender.seat(), Some(Seat::Orchestrator | Seat::Agent)) {
        return Err(ApiError::bad_request(
            "only the orchestrator and the agent of a column post to a session",
        ));
    }
    let body = req.body.trim();
    if body.is_empty() {
        return Err(ApiError::bad_request("a post needs a body"));
    }
    let task = state.store.get_task(&id).await?;
    if sender.goal_id.as_deref() != task.goal_id.as_deref() {
        return Err(ApiError::bad_request(format!(
            "task {id} is a task of another goal, so you cannot post to it"
        )));
    }
    ensure_task_scope(&ctx, &id)
        .map_err(|_| ApiError::bad_request(format!("task {id} is not your own task")))?;
    if let Some(reference) = state.store.source_task_reference(&id).await? {
        return to_worker(&state, sender, &reference, req.to.trim(), body).await;
    }
    if task.is_far() {
        return Err(ApiError::bad_request(format!(
            "task {id} is a legacy mirror of a task on host {}, and a post does not cross to a mirror.",
            task.host.as_deref().unwrap_or_default()
        )));
    }
    deliver(&state, &task, Sender::Session(sender), req.to.trim(), body).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Put a post of the source orchestrator into the live session of the agent
/// of one column of a worker-protocol task on this worker. The worker applies
/// the rules of a local post.
#[utoipa::path(post, path = "/v1/remote-tasks/{id}/posts", tag = "tasks",
    request_body = RemotePostRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 204), (status = 400), (status = 403)))]
pub(super) async fn post_from_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<RemotePostRequest>,
) -> ApiResult<StatusCode> {
    super::remote_tasks::source_only(&headers)?;
    let body = req.body.trim();
    if body.is_empty() {
        return Err(ApiError::bad_request("a post needs a body"));
    }
    let task = state.store.get_task(&id).await?;
    let Some(context) = state.store.remote_task_context(&id).await? else {
        return Err(ApiError::bad_request(format!(
            "task {id} is no worker-protocol task of a source, so a source cannot post to it"
        )));
    };
    if context.source_daemon_id != req.source_daemon_id {
        return Err(ApiError::bad_request(format!(
            "task {id} is the task of another source, so you cannot post to it"
        )));
    }
    deliver(
        &state,
        &task,
        Sender::Source(req.dropped.as_deref()),
        req.to.trim(),
        body,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Who sends a post: a session of this daemon, or the source orchestrator
/// of a worker-protocol task.
#[derive(Clone, Copy)]
enum Sender<'a> {
    Session(&'a AgentSession),
    /// With the reason when it tells of a dropped post.
    Source(Option<&'a str>),
}

impl Sender<'_> {
    fn is(&self, session: &AgentSession) -> bool {
        matches!(self, Sender::Session(sender) if sender.id == session.id)
    }
}

/// Where the address `orchestrator` reaches for a task of this daemon.
enum Orchestrator {
    /// A live orchestrator session of the goal.
    Local(Box<AgentSession>),
    /// The source orchestrator of a worker-protocol task, by the daemon ID
    /// of the source.
    Source(String),
}

/// The local rules of a post (039), for a task that runs on this daemon.
async fn deliver(
    state: &AppState,
    task: &Task,
    sender: Sender<'_>,
    to: &str,
    body: &str,
) -> ApiResult<()> {
    let id = &task.id;
    let agents = state.store.list_task_agents(id).await?;
    let source = state.store.remote_task_context(id).await?;
    // The addresses that work now: the orchestrator and the agent of the
    // current column, each with a live session that is not the sender's own.
    let orchestrator = match (&source, sender) {
        _ if task.goal_id.is_none() => None,
        (_, Sender::Source(_)) => None,
        (Some(context), _) => state
            .source_posts
            .is_open(&context.source_daemon_id)
            .then(|| Orchestrator::Source(context.source_daemon_id.clone())),
        (None, _) => live_session(
            state,
            SessionFilter {
                goal_id: task.goal_id.clone(),
                live_only: true,
                ..Default::default()
            },
            |s| s.seat() == Some(Seat::Orchestrator) && !sender.is(s),
        )
        .await?
        .map(|session| Orchestrator::Local(Box::new(session))),
    };
    let current = match agents
        .iter()
        .find(|a| task.step.as_deref().unwrap_or_default() == a.step)
    {
        Some(agent) => live_session(
            state,
            SessionFilter {
                task_id: Some(id.clone()),
                live_only: true,
                ..Default::default()
            },
            |s| s.task_agent_id.as_deref() == Some(agent.id.as_str()) && !sender.is(s),
        )
        .await?
        .map(|session| (agent.step.clone(), session)),
        None => None,
    };
    let working = working_addresses(
        orchestrator.is_some(),
        current.as_ref().map(|c| c.0.as_str()),
    );
    let column_of = |session: &AgentSession| {
        session
            .task_agent_id
            .as_ref()
            .and_then(|agent_id| agents.iter().find(|a| &a.id == agent_id))
            .map(|a| a.step.clone())
            .ok_or_else(|| ApiError::bad_request("your session is staffed on no column"))
    };
    let recipient = if to.eq_ignore_ascii_case("orchestrator") {
        let Sender::Session(session) = sender else {
            return Err(ApiError::bad_request(format!(
                "you cannot post to your own session. {working}"
            )));
        };
        // A worker-protocol task names its source goal as its origin too:
        // only a linked goal without a source context is a legacy mirror.
        if source.is_none() && state.store.task_goal(task).await?.origin_goal_id.is_some() {
            return Err(ApiError::bad_request(format!(
                "task {id} is a legacy mirror of a task on another host, and a post does not reach its orchestrator."
            )));
        }
        match (orchestrator, &source) {
            (Some(Orchestrator::Local(session)), _) => *session,
            (Some(Orchestrator::Source(daemon_id)), _) => {
                let post = SourcePostDto {
                    id: ariadne_core::id::new_id(),
                    task_id: id.clone(),
                    column: column_of(session)?,
                    body: body.to_string(),
                };
                if !state.source_posts.send(&daemon_id, post) {
                    return Err(ApiError::bad_request(format!(
                        "the source of task {id} has no open event stream here, so the post cannot reach the orchestrator. {working}"
                    )));
                }
                return Ok(());
            }
            (None, Some(_)) => {
                return Err(ApiError::bad_request(format!(
                    "the source of task {id} has no open event stream here, so the post cannot reach the orchestrator. {working}"
                )));
            }
            (None, None) => {
                return Err(ApiError::bad_request(format!(
                    "the orchestrator has no live session. {working}"
                )));
            }
        }
    } else {
        let Some(agent) = agents.iter().find(|a| a.step.eq_ignore_ascii_case(to)) else {
            return Err(ApiError::bad_request(format!(
                "task {id} staffs no column {to}. {working}"
            )));
        };
        if task.step.as_deref() != Some(agent.step.as_str()) {
            return Err(ApiError::bad_request(format!(
                "the {} column is not the current column of task {id}. {working}",
                agent.step
            )));
        }
        if let Sender::Session(session) = sender
            && session.task_agent_id.as_deref() == Some(agent.id.as_str())
        {
            return Err(ApiError::bad_request(format!(
                "you cannot post to your own session. {working}"
            )));
        }
        current.map(|c| c.1).ok_or_else(|| {
            ApiError::bad_request(format!(
                "the {} agent has no live session. {working}",
                agent.step
            ))
        })?
    };
    let (from, answer) = match sender {
        Sender::Source(Some(reason)) => {
            let text = prompts::dropped_post_notice(reason, body);
            return state
                .launcher
                .acp
                .send_post(&recipient.id, text)
                .map_err(|_| {
                    ApiError::bad_request("the recipient has no live session to take the notice")
                });
        }
        Sender::Source(None) => (
            "your orchestrator".to_string(),
            "to `orchestrator`".to_string(),
        ),
        Sender::Session(session) if session.seat() == Some(Seat::Orchestrator) => (
            "your orchestrator".to_string(),
            "to `orchestrator`".to_string(),
        ),
        Sender::Session(session) => {
            let column = column_of(session)?;
            agent_sender(&column, id, &task.title)
        }
    };
    let text = prompts::post_briefing(&from, body, &answer);
    state
        .launcher
        .acp
        .send_post(&recipient.id, text)
        .map_err(|_| ApiError::bad_request("the recipient has no live session to take the post"))
}

/// Send the post of the source orchestrator to the worker of `reference`.
/// The source refuses it while the host is not `ready`. The worker applies
/// the local rules, and its refusal returns with its sentence.
async fn to_worker(
    state: &AppState,
    sender: &AgentSession,
    reference: &SourceTaskReference,
    to: &str,
    body: &str,
) -> ApiResult<StatusCode> {
    let id = &reference.id;
    let host = &reference.host;
    if sender.seat() != Some(Seat::Orchestrator) {
        return Err(ApiError::bad_request(format!(
            "task {id} runs on host {host}, and only the orchestrator posts to it"
        )));
    }
    if to.eq_ignore_ascii_case("orchestrator") {
        return Err(ApiError::bad_request(format!(
            "you cannot post to your own session. Say a column of task {id}"
        )));
    }
    let ready = state
        .hosts
        .list()
        .into_iter()
        .find(|h| &h.name == host)
        .map(|h| (crate::hosts::ready_endpoint(&h).is_some(), h.status));
    match ready {
        Some((true, _)) => {}
        Some((false, status)) => {
            return Err(ApiError::bad_request(format!(
                "host {host} is {}, not `ready`, so the post cannot reach task {id}",
                format!("{status:?}").to_lowercase()
            )));
        }
        None => {
            return Err(ApiError::bad_request(format!(
                "host {host} of task {id} is unknown, so the post cannot reach it"
            )));
        }
    }
    let client = super::tasks::far_client(state, host)?;
    let request = RemotePostRequest {
        source_daemon_id: state.store.daemon_id().await?,
        to: to.to_string(),
        body: body.to_string(),
        dropped: None,
    };
    client
        .send_no_content(
            axum::http::Method::POST,
            &format!("/v1/remote-tasks/{id}/posts"),
            Some(&request),
        )
        .await
        .map_err(|error| super::tasks::far_error(host, error))?;
    Ok(StatusCode::NO_CONTENT)
}

/// The sender and the answer address of a post from the agent of `column`
/// on the task `id` titled `title`.
pub(crate) fn agent_sender(column: &str, id: &str, title: &str) -> (String, String) {
    (
        format!(r#"the {column} agent of task {id} "{title}""#),
        format!("to `{column}` and task `{id}`"),
    )
}

/// The sentence that names each address that works now, or says none does.
fn working_addresses(orchestrator: bool, column: Option<&str>) -> String {
    let mut addresses = Vec::new();
    if orchestrator {
        addresses.push("`orchestrator`".to_string());
    }
    if let Some(column) = column {
        addresses.push(format!("`{column}`"));
    }
    match addresses.is_empty() {
        true => "No address works now: no other session of the task is live.".to_string(),
        false => format!("Say {}.", addresses.join(" or ")),
    }
}

async fn live_session(
    state: &AppState,
    filter: SessionFilter,
    pick: impl Fn(&AgentSession) -> bool,
) -> ApiResult<Option<AgentSession>> {
    Ok(state
        .store
        .list_sessions(filter)
        .await?
        .into_iter()
        .find(pick))
}
