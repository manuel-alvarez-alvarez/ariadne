//! What happens to a task once its engineer says it is done: the verdicts, the
//! diff they are about, the request it was published as, and the proof that it
//! landed.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::reviews::{CreateReviewRequest, ReviewDto};
use ariadne_api::tasks::{RecordPullRequestRequest, TaskDto};
use ariadne_core::{AttentionReason, MergeStrategy, Role, TaskStatus};
use ariadne_store::{NewReview, Repository, Task};

use super::AppState;
use super::convert::{review_dto, task_dto_of};
use super::error::{ApiError, ApiResult, Json};
use super::caller::{call_ctx, ensure_task_scope};

/// Git could not answer about the task's branch: a conflict, since what the
/// caller asked for cannot be established rather than being wrong.
fn unresolved(e: impl std::fmt::Display) -> ApiError {
    ApiError::conflict(e.to_string())
}

/// Prove the task really was landed, in the primary checkout and with git
/// alone.
///
/// What "landed" leaves behind depends on the strategy, so the check does too.
///
/// `direct` rebases, squashes and fast-forwards, which leaves the base tip *as*
/// the branch tip: the branch being an ancestor of the base is the whole of it,
/// and no sha has to be taken on trust.
///
/// `pull_request` is squashed by the forge, which writes a commit no branch
/// points at — the task branch is deliberately *not* an ancestor of the base
/// afterwards. What can be checked here is the other half of the engineer's
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
    match repo.merge_strategy() {
        MergeStrategy::Direct => {
            if !on_the_base(&task.branch).await? {
                return Err(ApiError::conflict(format!(
                    "merge not verified: {} is not an ancestor of {} in {}",
                    task.branch, repo.base_branch, repo.path
                )));
            }
        }
        MergeStrategy::PullRequest => {
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

/// Record the pull or merge request the engineer opened for a task.
///
/// The URL travels as a tool call, so a published task is either one the UI
/// and the CLI can point at or one that was never reported.
///
/// And this is the moment the task becomes the user's: a request nobody can
/// merge but a human is exactly what `waiting_user` says, so it goes up here,
/// on the session that opened it — the pane they answer in, and the one place
/// the request can be traced back to. It used to be raised by the message the
/// landing briefing told the engineer to write, and a published task with
/// nothing on the strip is one nobody knows to go and merge.
///
/// It stays up until the user acts: an agent's own events never take
/// `waiting_user` down (`clear_agent_attention`), and the engineer polling its
/// request is exactly such an agent. `Scheduler::keep_waiting_user` puts it
/// back on whatever comes up when that engineer is restarted, which is what
/// makes the two halves one flag rather than two.
#[utoipa::path(post, path = "/v1/tasks/{id}/pull-request", tag = "tasks",
    request_body = RecordPullRequestRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 200, body = TaskDto),
        (status = 400, description = "empty URL"),
        (status = 403, description = "not an engineer session"),
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
    let Some(engineer) = ctx.session.filter(|s| s.role() == Role::Engineer) else {
        return Err(ApiError::forbidden(
            "only the engineer of a task may record its pull request",
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
        .set_session_attention(&engineer.id, AttentionReason::WaitingUser)
        .await?;
    state.notify_scheduler(&id);
    let task = state.store.get_task(&id).await?;
    Ok(Json(task_dto_of(&state.store, task).await?))
}

/// Reviews of a task.
#[utoipa::path(get, path = "/v1/tasks/{id}/reviews", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = [ReviewDto])))]
pub async fn list_reviews(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ReviewDto>>> {
    state.store.get_task(&id).await?;
    let rows = state.store.list_reviews(&id, None).await?;
    Ok(Json(rows.into_iter().map(review_dto).collect()))
}

/// Submit a review verdict for the current round.
#[utoipa::path(post, path = "/v1/tasks/{id}/reviews", tag = "tasks",
    request_body = CreateReviewRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 201, body = ReviewDto), (status = 409)))]
pub async fn post_review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateReviewRequest>,
) -> ApiResult<(StatusCode, Json<ReviewDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let task = state.store.get_task(&id).await?;
    if task.status() != TaskStatus::UnderReview {
        return Err(ApiError::conflict(format!(
            "task is {}, reviews are only accepted under_review",
            task.status
        )));
    }

    // Reviewer identity: from the session, or explicit for user-submitted reviews.
    let reviewer_profile_id = match (&ctx.session, &req.reviewer_profile) {
        (Some(session), _) => {
            if session.role() != Role::Reviewer {
                return Err(ApiError::forbidden(
                    "only reviewer sessions may submit reviews",
                ));
            }
            session.profile_id.clone()
        }
        (None, Some(spec)) => state.store.resolve_profile(spec).await?.id,
        (None, None) => {
            return Err(ApiError::bad_request(
                "reviewer_profile is required for user-submitted reviews",
            ));
        }
    };
    let assigned = state.store.list_task_reviewers(&id).await?;
    if !assigned.contains(&reviewer_profile_id) {
        return Err(ApiError::forbidden(format!(
            "profile {reviewer_profile_id} is not an assigned reviewer of task {id}"
        )));
    }

    let review = state
        .store
        .create_review(NewReview {
            task_id: id.clone(),
            round: task.review_round,
            reviewer_profile_id,
            session_id: ctx.session.map(|s| s.id),
            verdict: req.verdict,
            body: req.body,
        })
        .await?;
    state.notify_scheduler(&id);
    Ok((StatusCode::CREATED, Json(review_dto(review))))
}

/// Diff of the task branch against its base (`git diff base...branch`), or,
/// once the task is merged, the diff its merge commit brought into the base —
/// after the merge the branch is contained in the base, so the three-dot diff
/// would be forever empty.
#[utoipa::path(get, path = "/v1/tasks/{id}/diff", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, content_type = "text/plain", body = String), (status = 404), (status = 409)))]
pub async fn diff(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<String> {
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
