//! The diff of a task's work, and the request the `pr` column opens.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::tasks::{OpenPullRequestRequest, TaskDto};
use ariadne_core::TaskStatus;
use ariadne_store::Task;

use super::AppState;
use super::caller::{call_ctx, ensure_task_scope};
use super::convert::task_dto_of;
use super::error::{ApiError, ApiResult, Json};
use crate::forge::ForgeClient;
use crate::forge::pulls;

/// Git could not answer about the task's branch: a conflict, since what the
/// caller asked for cannot be established rather than being wrong.
fn unresolved(e: impl std::fmt::Display) -> ApiError {
    ApiError::conflict(e.to_string())
}

/// Open the pull request a task lands by, through the repository's own
/// forge CLI, and answer its URL.
///
/// The daemon runs the forge call, never the agent: `gh` only ever runs
/// here, with the authentication the user already set up for it
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
    // The forge never sees a trailer an agent's skill appends: a
    // `Generated with` footer or a `Co-Authored-By` is noise on a request a
    // human reads.
    let body = pulls::strip_trailers(&req.body);
    let url = client
        .open(
            &slug,
            &task.branch,
            &repo.base_branch,
            &req.title,
            &body,
            true,
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

/// The ref a task's work is compared against. A linked task starts at the
/// fetched remote base, which can be ahead of the local base branch: compare
/// against that tracking ref, so work that merged on other hosts is neither a
/// task commit nor part of the task's diff. Any other task, and a linked task
/// whose tracking ref is missing, uses the local base branch.
pub(super) async fn task_base(
    state: &AppState,
    repo: &ariadne_store::Repository,
    task: &Task,
) -> ApiResult<String> {
    let goal = state.store.task_goal(task).await?;
    if goal.origin_goal_id.is_some() {
        let remote = repo.forge.as_ref().map_or("origin", |f| f.remote.as_str());
        let tracking = format!("refs/remotes/{remote}/{}", repo.base_branch);
        if state
            .launcher
            .git
            .has_commit(std::path::Path::new(&repo.path), &tracking)
            .await
            .unwrap_or(false)
        {
            return Ok(tracking);
        }
    }
    Ok(repo.base_branch.clone())
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
    let base = task_base(&state, &repo, &task).await?;
    state
        .launcher
        .git
        .diff(&repo_path, &base, &task.branch)
        .await
        .map_err(unresolved)
}
