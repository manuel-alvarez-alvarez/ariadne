//! What happens to a task once its author says it is done: the verdicts, the
//! diff they are about, the request it was published as, and the proof that it
//! landed.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::messages::{MessageDto, MessageListQuery, SendMessageRequest};
use ariadne_api::tasks::{PickWinnerRequest, RecordPullRequestRequest, TaskDto};
use ariadne_core::{Actor, AttentionReason, Landing, Seat, TaskStatus};
use ariadne_store::{MessageFilter, NewMessage, Repository, Task};

use super::AppState;
use super::caller::{CallCtx, call_ctx, ensure_task_scope};
use super::convert::{message_dto, task_dto_of};
use super::error::{ApiError, ApiResult, Json};

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
/// `pull_request` is squashed by the forge, which writes a commit no branch
/// points at — the task branch is deliberately *not* an ancestor of the base
/// afterwards. What can be checked here is the other half of the author's
/// last step: it fetches and fast-forwards the local base onto the merge, and
/// the sha it reports has to be on that base branch. Until it is, the change is
/// not on this machine and the task is not finished. Asking the forge instead
/// would be the daemon watching a request again, which is what this release
/// stopped doing.
pub(super) async fn verify_merged(
    state: &AppState,
    task: &Task,
    repo: &Repository,
    reported: Option<&str>,
) -> ApiResult<()> {
    let repo_path = std::path::PathBuf::from(&repo.path);
    let on_the_base = async |rev: &str| {
        state
            .launcher
            .git
            .is_ancestor(&repo_path, rev, &repo.base_branch)
            .await
            .map_err(unresolved)
    };
    match task.landing() {
        // Nothing was landed, so there is nothing git can be asked about.
        // What the task produced is the task's own to have put where it
        // belongs, and no check here can see it.
        Landing::None => {}
        Landing::Merge => {
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
                    repo.base_branch, repo.path
                )));
            }
        }
        Landing::PullRequest => {
            let Some(sha) = reported else {
                return Err(ApiError::conflict(
                    "merge not verified: a published request is squashed by the forge, \
                     so report the sha it landed as (`git rev-parse <base>`)",
                ));
            };
            if !on_the_base(sha).await? {
                return Err(ApiError::conflict(format!(
                    "merge not verified: {sha} is not on {} in {} — fetch the remote \
                     and fast-forward the base branch first",
                    repo.base_branch, repo.path
                )));
            }
        }
    }
    Ok(())
}

/// Record the pull or merge request the author opened for a task.
///
/// The URL travels as a tool call, so a published task is either one the UI
/// and the CLI can point at or one that was never reported.
///
/// And this is the moment the task becomes the user's: a request nobody can
/// merge but a human is exactly what `waiting_user` says, so it goes up here,
/// on the session that opened it — the pane they answer in, and the one place
/// the request can be traced back to. It used to be raised by the message the
/// landing briefing told the author to write, and a published task with
/// nothing on the strip is one nobody knows to go and merge.
///
/// It stays up until the user acts: an agent's own events never take
/// `waiting_user` down (`clear_agent_attention`), and the author polling its
/// request is exactly such an agent. `Scheduler::keep_waiting_user` puts it
/// back on whatever comes up when that author is restarted, which is what
/// makes the two halves one flag rather than two.
#[utoipa::path(post, path = "/v1/tasks/{id}/pull-request", tag = "tasks",
    request_body = RecordPullRequestRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 200, body = TaskDto),
        (status = 400, description = "empty URL"),
        (status = 403, description = "not an author session"),
        (status = 409, description = "the task is not approved")
    ))]
pub async fn record_pull_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<RecordPullRequestRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let Some(author) = ctx.session.filter(|s| s.seat() == Seat::Author) else {
        return Err(ApiError::forbidden(
            "only the author of a task may record its pull request",
        ));
    };
    let task = state.store.get_task(&id).await?;
    if task.status() != TaskStatus::Approved {
        return Err(ApiError::conflict(format!(
            "task is {}, a pull request belongs to an approved task being landed",
            task.status
        )));
    }
    let url = req.url.trim();
    if url.is_empty() {
        return Err(ApiError::bad_request(
            "pass the URL `gh pr create` or `glab mr create` printed, e.g. \
             https://github.com/owner/repo/pull/12",
        ));
    }
    state.store.set_task_pull_request(&id, url).await?;
    state
        .store
        .set_session_attention(&author.id, AttentionReason::WaitingUser)
        .await?;
    state.notify_scheduler(&id);
    let task = state.store.get_task(&id).await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

/// The messages of a task: what its agents have said to each other.
#[utoipa::path(get, path = "/v1/tasks/{id}/messages", tag = "tasks",
    params(("id" = String, Path, description = "task id"), MessageListQuery),
    responses((status = 200, body = [MessageDto])))]
pub async fn list_task_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MessageListQuery>,
) -> ApiResult<Json<Vec<MessageDto>>> {
    state.store.get_task(&id).await?;
    let rows = state
        .store
        .list_messages(MessageFilter {
            task_id: Some(id),
            to_agent_id: q.to_agent_id,
            undelivered_only: q.undelivered,
            ..Default::default()
        })
        .await?;
    Ok(Json(rows.into_iter().map(message_dto).collect()))
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
pub async fn post_task_message(
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

    Ok(state
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
        .await?)
}

/// Which branch of a task a diff is asked about: one author's of several, or
/// — left out — the task's own, which after the pick is the winner's.
#[derive(Debug, Default, serde::Deserialize, utoipa::IntoParams)]
pub struct DiffQuery {
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
pub async fn diff(
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
    state
        .launcher
        .git
        .diff(&repo_path, &repo.base_branch, &branch)
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
pub async fn pick_winner(
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
        .filter(|s| s.seat() == Seat::Reviewer)
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
