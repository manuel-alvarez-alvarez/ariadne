//! What happens to a task once its author says it is done: the verdicts, the
//! diff they are about, the request it was published as, and the proof that it
//! landed.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::DateTime;
use serde_json::json;
use tracing::warn;

use ariadne_api::messages::{MessageDto, MessageListQuery, SendMessageRequest};
use ariadne_api::tasks::{OpenPullRequestRequest, PickWinnerRequest, TaskDto};
use ariadne_core::{Actor, Landing, Seat, TaskStatus};
use ariadne_store::{MessageFilter, NewMessage, Repository, SessionFilter, Task};

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

/// Prove the task really was landed, in the primary checkout and with git
/// alone.
///
/// What "landed" leaves behind depends on how the task ends, so the check
/// does too — and a task that lands nothing leaves nothing to check, which is
/// the whole of what makes a release or a filed report finishable.
///
/// `direct` rebases, squashes and fast-forwards, which leaves the base tip *as*
/// the branch tip: the branch being an ancestor of the base is the whole of it,
/// and no sha has to be taken on trust.
///
/// `pull_request` ends once the request is open: the task has a `pr_url`,
/// and the branch it was opened from is still what the remote has for it.
/// What happens to the request from there — its comments, its checks, its
/// merge — is not this task's to wait on any more.
pub(super) async fn verify_merged(
    state: &AppState,
    task: &Task,
    repo: &Repository,
    reported: Option<&str>,
) -> ApiResult<()> {
    let repo_path = std::path::PathBuf::from(&repo.path);
    let base_branch = state.store.task_landing_branch(task, repo).await?;
    let on_the_base = async |rev: &str| {
        state
            .launcher
            .git
            .is_ancestor(&repo_path, rev, &base_branch)
            .await
            .map_err(unresolved)
    };
    // The final task of a feature branch goal is published by its request,
    // the same as a published task is.
    let landing = match task.landing() {
        Landing::FeatureBranch if state.store.works_on_goal_branch(task).await? => {
            Landing::PullRequest
        }
        landing => landing,
    };
    match landing {
        // Nothing was landed, so there is nothing git can be asked about.
        // What the task produced is the task's own to have put where it
        // belongs, and no check here can see it.
        Landing::None => {}
        // Both endings verify the task against its landing branch.
        Landing::Merge | Landing::FeatureBranch => {
            // The branch that lands is the picked winner's, on a task
            // staffed with several authors; the task's own everywhere else.
            let branch = state
                .launcher
                .review_branch(task, task.picked_agent_id.as_deref())
                .await
                .map_err(unresolved)?
                .unwrap_or_else(|| task.branch.clone());
            if !on_the_base(&branch).await? {
                return Err(ApiError::conflict(format!(
                    "merge not verified: {branch} is not an ancestor of {} in {}",
                    base_branch, repo.path
                )));
            }
            if task.landing() == Landing::FeatureBranch
                && let Some(sha) = reported
                && !on_the_base(sha).await?
            {
                return Err(ApiError::conflict(format!(
                    "merge not verified: {sha} is not on {base_branch} in {}",
                    repo.path
                )));
            }
        }
        Landing::PullRequest => {
            let Some(_) = &task.pr_url else {
                return Err(ApiError::conflict(
                    "merge not verified: no pull request is open for this task — call \
                     `open_pull_request` first",
                ));
            };
            let remote = repo
                .forge
                .as_ref()
                .map_or("origin", |forge| forge.remote.as_str());
            let branch = state
                .launcher
                .review_branch(task, task.picked_agent_id.as_deref())
                .await
                .map_err(unresolved)?
                .unwrap_or_else(|| task.branch.clone());
            if !state
                .launcher
                .git
                .remote_has_branch_tip(&repo_path, remote, &branch)
                .await
                .map_err(unresolved)?
            {
                return Err(ApiError::conflict(format!(
                    "merge not verified: {branch} is not on {remote} in {}",
                    repo.path
                )));
            }
        }
    }
    Ok(())
}

/// Open the pull or merge request a task lands by, through the repository's
/// own forge CLI, and answer its URL.
///
/// The daemon runs the forge call, never the agent: `gh` or `glab` only ever
/// run here, with the authentication the user already set up for them
/// ([`crate::forge`]). The author supplies only what the daemon cannot read
/// off the task or the repository — the title and the body — and gets the
/// URL back to show the user.
///
/// One request per task: a task that already has a `pr_url` answers it again
/// and opens nothing, so a retried call never opens a second request.
#[utoipa::path(post, path = "/v1/tasks/{id}/pull-request", tag = "tasks",
    request_body = OpenPullRequestRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 200, body = TaskDto),
        (status = 403, description = "not an author session"),
        (status = 409, description = "the task is not approved, has no forge, \
                                       is not authenticated, or is not pushed")
    ))]
pub(super) async fn open_pull_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<OpenPullRequestRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    if ctx.session.as_ref().and_then(|s| s.seat()) != Some(Seat::Author) {
        return Err(ApiError::forbidden(
            "only the author of a task may open its pull request",
        ));
    }
    let task = state.store.get_task(&id).await?;
    if task.status() != TaskStatus::Approved {
        return Err(ApiError::conflict(format!(
            "task is {}, a pull request belongs to an approved task being landed",
            task.status
        )));
    }
    // One request per task: a second call answers the same URL.
    if task.pr_url.is_some() {
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
    let client = ForgeClient::for_repository(&state.launcher.cfg, forge);
    let refused =
        |message: String| ApiError::new(StatusCode::CONFLICT, "forge_unauthenticated", message);
    client.auth_status(&forge.host).await.map_err(refused)?;

    // The branch that lands is the picked winner's, on a task staffed with
    // several authors; the task's own everywhere else.
    let branch = state
        .launcher
        .review_branch(&task, task.picked_agent_id.as_deref())
        .await
        .map_err(unresolved)?
        .unwrap_or_else(|| task.branch.clone());
    let repo_path = std::path::PathBuf::from(&repo.path);
    let pushed = state
        .launcher
        .git
        .remote_has_branch_tip(&repo_path, &forge.remote, &branch)
        .await
        .map_err(unresolved)?;
    if !pushed {
        return Err(ApiError::conflict(format!(
            "push {branch} to {} first",
            forge.remote
        )));
    }

    let base = state.store.task_landing_branch(&task, &repo).await?;
    let slug = format!("{}/{}", forge.owner, forge.name);
    let url = client
        .open(&slug, &branch, &base, &req.title, &req.body, req.draft)
        .await
        .map_err(unresolved)?;

    state.store.set_task_pull_request(&id, &url).await?;
    let task = state.store.get_task(&id).await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

/// The messages of a task: what its agents have said to each other.
#[utoipa::path(get, path = "/v1/tasks/{id}/messages", tag = "tasks",
    params(("id" = String, Path, description = "task id"), MessageListQuery),
    responses((status = 200, body = [MessageDto])))]
pub(super) async fn list_task_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<MessageListQuery>,
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
/// Three things are checked here, and they are the three an agent can get
/// wrong: a recipient that is not on this task, a verdict from an agent that
/// is not one of its reviewers, and a verdict on a task nobody asked to have
/// reviewed. Everything else the channel carries as it stands — it is a
/// conversation, and the daemon is not in it.
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
        Actor::Author | Actor::Reviewer => {
            let Some(agent_id) = &to_agent_id else {
                return Err(ApiError::bad_request(format!(
                    "a message to the {} needs the to_agent_id `get_task` lists",
                    to_actor.as_str()
                )));
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
    if req.kind.is_verdict() {
        let task = task.ok_or_else(|| ApiError::bad_request("a verdict needs a task"))?;
        if task.status() != TaskStatus::UnderReview {
            return Err(ApiError::conflict(format!(
                "task is {}, a verdict is only taken under_review",
                task.status
            )));
        }
        let Some(agent_id) = &from_agent_id else {
            return Err(ApiError::forbidden(
                "a verdict comes from a reviewer staffed on the task",
            ));
        };
        let reviewers = state.store.list_task_reviewers(&task.id).await?;
        if !reviewers.iter().any(|a| a.id == *agent_id) {
            return Err(ApiError::forbidden(format!(
                "agent {agent_id} is not a reviewer of task {}",
                task.id
            )));
        }
        // One verdict per reviewer per review asked for. This used to be a
        // unique index over the round the row carried; a review is bounded by
        // its own request now, which is a row rather than a column, so the
        // rule is read here. On a task staffed with several authors the
        // reviews run side by side, so the review a verdict belongs to is the
        // one its address names: the author it judges.
        let authors = state.store.list_task_authors(&task.id).await?;
        if authors.len() > 1 {
            let Some(author_id) = to_agent_id.as_deref() else {
                return Err(ApiError::bad_request(
                    "a verdict goes to the author whose change it judges",
                ));
            };
            if !authors.iter().any(|a| a.id == author_id) {
                let ids: Vec<&str> = authors.iter().map(|a| a.id.as_str()).collect();
                return Err(ApiError::bad_request(format!(
                    "a verdict goes to the author whose change it judges; \
                     the authors of task {} are: {}",
                    task.id,
                    ids.join(", ")
                )));
            }
            if state
                .store
                .open_review_request_of(&task.id, author_id)
                .await?
                .is_none()
            {
                return Err(ApiError::conflict(format!(
                    "author {author_id} has not asked for a review of task {}",
                    task.id
                )));
            }
            if state
                .store
                .open_verdicts_of(&task.id, author_id)
                .await?
                .iter()
                .any(|m| m.from_agent_id.as_deref() == Some(agent_id.as_str()))
            {
                return Err(ApiError::conflict(format!(
                    "agent {agent_id} has already given its verdict on this review of \
                     author {author_id} on task {}",
                    task.id
                )));
            }
        } else if state
            .store
            .open_verdicts(&task.id)
            .await?
            .iter()
            .any(|m| m.from_agent_id.as_deref() == Some(agent_id.as_str()))
        {
            return Err(ApiError::conflict(format!(
                "agent {agent_id} has already given its verdict on this review of task {}",
                task.id
            )));
        }
    }

    let message = state
        .store
        .send_message(NewMessage {
            goal_id: goal_id.to_string(),
            task_id: task.map(|t| t.id.clone()),
            kind: req.kind,
            from_actor: ctx.actor,
            from_agent_id,
            from_session: ctx.session.as_ref().map(|s| s.id.clone()),
            to_actor,
            to_agent_id,
            body: body.to_string(),
        })
        .await?;
    record_message_fact(state, ctx, task, &message).await;
    if req.kind.is_verdict() {
        record_verdict_fact(state, ctx, task.expect("a verdict needs a task"), &message).await;
    }
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

/// A verdict describes both the reviewer that sent it and the author change it
/// judged. The channel is the round counter: each request from that author is
/// one review boundary.
async fn record_verdict_fact(
    state: &AppState,
    ctx: &CallCtx,
    task: &Task,
    message: &ariadne_store::Message,
) {
    let Some(reviewer) = ctx.session.as_ref() else {
        return;
    };
    let Some(author_id) = message.to_agent_id.as_deref() else {
        return;
    };
    let messages = match state
        .store
        .list_messages(MessageFilter {
            task_id: Some(task.id.clone()),
            ..Default::default()
        })
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            warn!(error = %error, "could not read review messages for a verdict fact");
            return;
        }
    };
    let requests: Vec<_> = messages
        .iter()
        .filter(|m| {
            m.kind.as_str() == "review_request" && m.from_agent_id.as_deref() == Some(author_id)
        })
        .collect();
    let Some(request) = requests.last() else {
        return;
    };
    let author = match state
        .store
        .list_sessions(SessionFilter {
            task_id: Some(task.id.clone()),
            ..Default::default()
        })
        .await
    {
        Ok(sessions) => sessions
            .into_iter()
            .filter(|s| s.task_agent_id.as_deref() == Some(author_id))
            .max_by(|left, right| left.id.cmp(&right.id)),
        Err(error) => {
            warn!(error = %error, "could not find the author session for a verdict fact");
            return;
        }
    };
    let latency_secs = match (
        DateTime::parse_from_rfc3339(&request.created_at),
        DateTime::parse_from_rfc3339(&message.created_at),
    ) {
        (Ok(start), Ok(end)) => (end - start).num_seconds().max(0),
        _ => 0,
    };
    let data = json!({ "verdict": message.kind, "author_model": author.as_ref().map(|s| &s.model), "author_session_id": author.as_ref().map(|s| &s.id), "round": requests.len(), "latency_secs": latency_secs });
    match session_fact(&state.store, &reviewer.id, "verdict", data).await {
        Ok(fact) => match state.store.record_fact(fact).await {
            Ok(()) => {}
            Err(error) => warn!(error = %error, "could not record a verdict fact"),
        },
        Err(error) => warn!(error = %error, "could not record a verdict fact"),
    }
}

/// Which branch of a task a diff is asked about: one author's of several, or
/// — left out — the task's own, which after the pick is the winner's.
#[derive(Debug, Default, serde::Deserialize, utoipa::IntoParams)]
pub(super) struct DiffQuery {
    /// Id of the author whose branch to read, on a task staffed with several.
    pub agent: Option<String>,
}

/// Diff of the task branch against its base (`git diff base...branch`), or,
/// once the task is merged, the diff its merge commit brought into the base —
/// after the merge the branch is contained in the base, so the three-dot diff
/// would be forever empty.
///
/// On a task staffed with several authors, `agent` names the author whose
/// branch to read; left out, the task's own branch is read — the first
/// author's until the pick settles, and the winner's after it.
#[utoipa::path(get, path = "/v1/tasks/{id}/diff", tag = "tasks",
    params(("id" = String, Path, description = "task id"), DiffQuery),
    responses((status = 200, content_type = "text/plain", body = String), (status = 404), (status = 409)))]
pub(super) async fn diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<DiffQuery>,
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

    let author = match &q.agent {
        Some(agent_id) => {
            let authors = state.store.list_task_authors(&task.id).await?;
            if !authors.iter().any(|a| a.id == *agent_id) {
                return Err(ApiError::bad_request(format!(
                    "agent {agent_id} is not an author of task {}",
                    task.id
                )));
            }
            Some(agent_id.as_str())
        }
        None => task.picked_agent_id.as_deref(),
    };
    let branch = state
        .launcher
        .review_branch(&task, author)
        .await
        .map_err(unresolved)?
        .unwrap_or_else(|| task.branch.clone());

    if !state
        .launcher
        .git
        .branch_exists(&repo_path, &branch)
        .await
        .map_err(unresolved)?
    {
        return Err(ApiError::conflict(format!(
            "branch {branch} does not exist yet (task not started?)"
        )));
    }
    let base_branch = state.store.task_landing_branch(&task, &repo).await?;
    state
        .launcher
        .git
        .diff(&repo_path, &base_branch, &branch)
        .await
        .map_err(unresolved)
}

/// One reviewer's pick of the winning author, on a task staffed with several.
///
/// The gate is here: the pick starts only once every author is approved, and
/// a pick before that is refused. One pick per reviewer per task — a second
/// is refused by the reviewer's name — and the daemon settles the winner once
/// every staffed reviewer has picked.
#[utoipa::path(post, path = "/v1/tasks/{id}/pick", tag = "tasks",
    request_body = PickWinnerRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 200, body = TaskDto),
        (status = 400, description = "not an author of the task"),
        (status = 403, description = "not a reviewer session"),
        (status = 409, description = "the pick has not started, or this reviewer has picked already")
    ))]
pub(super) async fn pick_winner(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<PickWinnerRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let Some(reviewer_id) = ctx
        .session
        .as_ref()
        .filter(|s| s.seat() == Some(Seat::Reviewer))
        .and_then(|s| s.task_agent_id.clone())
    else {
        return Err(ApiError::forbidden(
            "only a reviewer of the task may pick the winner",
        ));
    };
    let task = state.store.get_task(&id).await?;
    let authors = state.store.list_task_authors(&id).await?;
    if authors.len() < 2 {
        return Err(ApiError::conflict(format!(
            "task {} has one author; there is nothing to pick",
            task.id
        )));
    }
    if task.picked_agent_id.is_some() {
        return Err(ApiError::conflict(format!(
            "the pick on task {} has settled already",
            task.id
        )));
    }
    if task.status() != TaskStatus::UnderReview {
        return Err(ApiError::conflict(format!(
            "task is {}, a pick is only taken under_review",
            task.status
        )));
    }
    if !authors.iter().any(|a| a.id == req.author_agent_id) {
        let ids: Vec<&str> = authors.iter().map(|a| a.id.as_str()).collect();
        return Err(ApiError::bad_request(format!(
            "agent {} is not an author of task {}; the authors are: {}",
            req.author_agent_id,
            task.id,
            ids.join(", ")
        )));
    }
    // The gate the acceptance criteria name: no pick before every author is
    // approved.
    if !state.store.authors_all_approved(&id).await? {
        return Err(ApiError::conflict(format!(
            "the pick starts once every author of task {} is approved",
            task.id
        )));
    }
    state
        .store
        .record_pick(&id, &reviewer_id, &req.author_agent_id)
        .await?;
    state.notify_scheduler(&id);
    let task = state.store.get_task(&id).await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}
