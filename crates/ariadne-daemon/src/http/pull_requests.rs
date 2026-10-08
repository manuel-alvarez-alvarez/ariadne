//! The ledger and live forge search.
use super::{
    AppState,
    caller::call_ctx,
    convert::{pull_request_comment_dto, pull_request_dto_of},
    error::{ApiError, ApiResult, Json},
};
use crate::forge::pulls::{DraftComment, ReviewDraft};
use crate::forge::{ForgeClient, PullRequestRef, pulls};
use ariadne_api::pull_requests::{
    AddPullRequestRequest, AskReviewRequest, PullRequestCommentDto, PullRequestCommentQuery,
    PullRequestDiffQuery, PullRequestDto, PullRequestListQuery, PullRequestMatchDto,
    ReplyCommentRequest, ReportPullRequestRequest, SubmitReviewRequest,
};
use ariadne_core::{AttentionReason, Seat};
use ariadne_store::{
    AgentSession, ForgeIntegration, NewPullRequestComment, PullRequest, PullRequestFilter,
};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use utoipa::IntoParams;

fn forge_error(error: String) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, "forge_error", error)
}
async fn integration(state: &AppState, id: &str) -> ApiResult<ForgeIntegration> {
    state
        .store
        .get_repository(id)
        .await?
        .forge
        .filter(|f| f.enabled)
        .ok_or_else(|| ApiError::conflict("the repository's forge integration is off"))
}
fn slug(forge: &ForgeIntegration) -> String {
    format!("{}/{}/{}", forge.host, forge.owner, forge.name)
}

#[utoipa::path(get, path = "/v1/pull-requests", tag = "pull-requests", params(PullRequestListQuery), responses((status = 200, body = [PullRequestDto])))]
pub(super) async fn list(
    State(state): State<AppState>,
    Query(q): Query<PullRequestListQuery>,
) -> ApiResult<Json<Vec<PullRequestDto>>> {
    if q.role
        .as_deref()
        .is_some_and(|r| !matches!(r, "author" | "reviewer"))
    {
        return Err(ApiError::bad_request("role must be author or reviewer"));
    }
    let selected = q.state.as_deref().unwrap_or("open");
    if !matches!(selected, "open" | "merged" | "closed" | "all") {
        return Err(ApiError::bad_request(
            "state must be open, merged, closed or all",
        ));
    }
    let rows = state
        .store
        .list_pull_requests(PullRequestFilter {
            repository_id: q.repo,
            role: q.role,
            state: (selected != "all").then(|| selected.to_owned()),
            origin_task_id: q.task,
            review_requested: q.requested,
        })
        .await?;
    let mut dtos = Vec::with_capacity(rows.len());
    for row in rows {
        dtos.push(pull_request_dto_of(&state.store, row).await?);
    }
    Ok(Json(dtos))
}
#[utoipa::path(get, path = "/v1/pull-requests/{id}", tag = "pull-requests", params(("id" = String, Path)), responses((status = 200, body = PullRequestDto), (status = 404)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<PullRequestDto>> {
    let row = state.store.get_pull_request(&id).await?;
    Ok(Json(pull_request_dto_of(&state.store, row).await?))
}
#[utoipa::path(post, path = "/v1/pull-requests", tag = "pull-requests", request_body = AddPullRequestRequest, responses((status = 201, body = PullRequestDto), (status = 200, body = PullRequestDto), (status = 404), (status = 409)))]
pub(super) async fn add(
    State(state): State<AppState>,
    Json(req): Json<AddPullRequestRequest>,
) -> ApiResult<(StatusCode, Json<PullRequestDto>)> {
    let (forge, number) = match (req.repository_id, req.number, req.url) {
        (Some(id), Some(number), None) if number > 0 => (integration(&state, &id).await?, number),
        (None, None, Some(url)) => state
            .store
            .enabled_forge_integrations()
            .await?
            .into_iter()
            .find_map(|forge| {
                PullRequestRef::parse(&url, &forge).map(|reference| (forge, reference.number))
            })
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "pull_request_not_found",
                    "no enabled repository matches this URL",
                )
            })?,
        _ => {
            return Err(ApiError::bad_request(
                "provide a URL or a repository_id and positive number",
            ));
        }
    };
    let pull = ForgeClient::for_repository(&state.launcher.cfg, &forge)
        .pull_request(&slug(&forge), number)
        .await
        .map_err(forge_error)?;
    if pull.number != number {
        return Err(forge_error(
            "the forge returned another request number".into(),
        ));
    }
    // A request of the user's own is the task's that opened it, and its
    // author keeps it (005). By hand, Ariadne takes only a request it is to
    // review.
    if pulls::role(&pull.author_login, &forge) == "author" {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "pull_request_is_yours",
            "this request is yours: Ariadne tracks by hand only a request that asks for your              review, and the task that opened a request of yours keeps it",
        ));
    }
    let (row, created) = pulls::record(&state.store, &forge, pull, "user", None, None)
        .await
        .map_err(forge_error)?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(pull_request_dto_of(&state.store, row).await?),
    ))
}
#[utoipa::path(delete, path = "/v1/pull-requests/{id}", tag = "pull-requests", params(("id" = String, Path)), responses((status = 204), (status = 404), (status = 409)))]
pub(super) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    // The request's sessions go with its row: their agents are taken down
    // first, so none is left running under a row that is gone.
    let row = state.store.get_pull_request(&id).await?;
    if row.tracked_by == "user" {
        state
            .launcher
            .end_pull_request_sessions(&row)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
    }
    state.store.delete_pull_request(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Debug, Default, Deserialize, IntoParams)]
pub(super) struct RefreshQuery {
    repo: Option<String>,
}
#[utoipa::path(post, path = "/v1/pull-requests/refresh", tag = "pull-requests", params(RefreshQuery), responses((status = 202), (status = 409)))]
pub(super) async fn refresh(
    State(state): State<AppState>,
    Query(q): Query<RefreshQuery>,
) -> ApiResult<StatusCode> {
    match q.repo {
        Some(id) => {
            integration(&state, &id).await?;
            state.forge_poll.wake(&id);
        }
        None => {
            for forge in state.store.enabled_forge_integrations().await? {
                state.forge_poll.wake(&forge.repository_id);
            }
        }
    }
    Ok(StatusCode::ACCEPTED)
}
#[derive(Debug, Default, Deserialize, IntoParams)]
pub(super) struct SearchQuery {
    #[serde(default)]
    q: String,
}
#[utoipa::path(get, path = "/v1/repositories/{id}/pull-requests/search", tag = "pull-requests", params(("id" = String, Path), SearchQuery), responses((status = 200, body = [PullRequestMatchDto]), (status = 409)))]
pub(super) async fn search(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Vec<PullRequestMatchDto>>> {
    let forge = integration(&state, &id).await?;
    let rows = state
        .store
        .list_pull_requests(PullRequestFilter {
            repository_id: Some(id),
            ..Default::default()
        })
        .await?;
    let matches = ForgeClient::for_repository(&state.launcher.cfg, &forge)
        .search_pull_requests(&slug(&forge), &q.q)
        .await
        .map_err(forge_error)?;
    let mut result = Vec::new();
    for pull in matches {
        let Some(reference) =
            PullRequestRef::parse(&pull.url, &forge).filter(|r| r.number == pull.number)
        else {
            return Err(forge_error(
                "the forge returned a request outside this repository".into(),
            ));
        };
        // A request of the user's own is no request to add by hand (005).
        if pulls::role(&pull.author_login, &forge) == "author" {
            continue;
        }
        result.push(PullRequestMatchDto {
            number: reference.number,
            url: pull.url,
            title: pull.title,
            author_login: pull.author_login,
            tracked: rows.iter().any(|row| row.number == reference.number),
        });
    }
    Ok(Json(result))
}

/// The session a call came from, refused unless it is the request's own: a
/// request is answered for by the review session the daemon started on it
/// (029), or, for a request a task opened, by that task's author, which
/// keeps it (005).
async fn own_session(
    state: &AppState,
    headers: &HeaderMap,
    pull: &PullRequest,
) -> ApiResult<AgentSession> {
    let ctx = call_ctx(&state.store, headers).await?;
    let keeps = |session: &AgentSession| {
        pull.role == "author"
            && session.seat() == Some(Seat::Author)
            && session.task_id.is_some()
            && session.task_id == pull.origin_task_id
    };
    match ctx.session {
        Some(session) if session.pull_request_id.as_deref() == Some(pull.id.as_str()) => {
            Ok(session)
        }
        Some(session) if keeps(&session) => Ok(session),
        Some(session) => Err(ApiError::forbidden(format!(
            "session {} does not watch pull request {}",
            session.id, pull.id
        ))),
        None => Err(ApiError::forbidden(format!(
            "only the session of pull request {} answers for it",
            pull.id
        ))),
    }
}

/// A session that names a request other than its own is refused; the user
/// reads every request.
async fn refuse_other_sessions(
    state: &AppState,
    headers: &HeaderMap,
    pull: &PullRequest,
) -> ApiResult<()> {
    match call_ctx(&state.store, headers).await?.session {
        Some(_) => own_session(state, headers, pull).await.map(drop),
        None => Ok(()),
    }
}

#[utoipa::path(get, path = "/v1/pull-requests/{id}/comments", tag = "pull-requests",
    params(("id" = String, Path), PullRequestCommentQuery),
    responses((status = 200, body = [PullRequestCommentDto]), (status = 403), (status = 404)))]
pub(super) async fn comments(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PullRequestCommentQuery>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<PullRequestCommentDto>>> {
    let pull = state.store.get_pull_request(&id).await?;
    refuse_other_sessions(&state, &headers, &pull).await?;
    let login = integration(&state, &pull.repository_id)
        .await
        .map(|forge| forge.login.unwrap_or_default())
        .unwrap_or_default();
    let rows = state
        .store
        .list_pull_request_comments(&pull.id, q.unanswered_only.unwrap_or(false), &login)
        .await?;
    Ok(Json(
        rows.into_iter().map(pull_request_comment_dto).collect(),
    ))
}

/// Reply to one stored comment. The daemon posts the reply through the
/// forge CLI, stores it as a comment of the integration login, and counts
/// the threads that wait again.
#[utoipa::path(post, path = "/v1/pull-requests/{id}/comments/{comment_id}/reply", tag = "pull-requests",
    params(("id" = String, Path), ("comment_id" = String, Path)),
    request_body = ReplyCommentRequest,
    responses((status = 201, body = PullRequestCommentDto), (status = 403), (status = 404), (status = 409), (status = 502)))]
pub(super) async fn reply(
    State(state): State<AppState>,
    Path((id, comment_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<ReplyCommentRequest>,
) -> ApiResult<(StatusCode, Json<PullRequestCommentDto>)> {
    let pull = state.store.get_pull_request(&id).await?;
    let session = own_session(&state, &headers, &pull).await?;
    // A review session's reply is the review's, as its findings are.
    let from_review = session.pull_request_id.as_deref() == Some(pull.id.as_str());
    let body = req.body.trim();
    if body.is_empty() {
        return Err(ApiError::bad_request("a reply needs a body"));
    }
    let comment = state
        .store
        .get_pull_request_comment(&pull.id, &comment_id)
        .await?;
    let forge = integration(&state, &pull.repository_id).await?;
    let login = forge.login.clone().unwrap_or_default();
    let forge_id = ForgeClient::for_repository(&state.launcher.cfg, &forge)
        .reply(&slug(&forge), pull.number, &comment, body)
        .await
        .map_err(forge_error)?;
    let in_reply_to = match comment.kind.as_str() {
        "review_comment" => comment
            .in_reply_to
            .clone()
            .or(Some(comment.forge_id.clone())),
        _ => None,
    };
    let kind = match comment.kind.as_str() {
        "review_comment" => "review_comment",
        _ => "issue_comment",
    };
    state
        .store
        .upsert_pull_request_comments(
            &pull.id,
            &[NewPullRequestComment {
                forge_id: forge_id.clone(),
                thread_id: comment.thread_id.clone(),
                kind: kind.into(),
                author_login: login.clone(),
                author_is_bot: false,
                body: body.to_string(),
                path: comment.path.clone(),
                line: comment.line,
                in_reply_to,
                created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                resolved: comment.resolved,
                from_review,
            }],
            &login,
        )
        .await?;
    let stored = state
        .store
        .list_pull_request_comments(&pull.id, false, &login)
        .await?
        .into_iter()
        .find(|c| c.forge_id == forge_id)
        .ok_or_else(|| ApiError::conflict("the reply was posted but not stored"))?;
    state.notify_scheduler_pull_request(&pull.id);
    Ok((StatusCode::CREATED, Json(pull_request_comment_dto(stored))))
}

/// Resolve the thread of one stored comment, once a push fixed what it
/// found (029). Only a review session resolves, and only a thread it opened
/// under the integration login: the threads of anyone else stay open for
/// the person who wrote them.
#[utoipa::path(post, path = "/v1/pull-requests/{id}/comments/{comment_id}/resolve", tag = "pull-requests",
    params(("id" = String, Path), ("comment_id" = String, Path)),
    responses((status = 200, body = PullRequestCommentDto), (status = 403), (status = 404), (status = 409), (status = 502)))]
pub(super) async fn resolve(
    State(state): State<AppState>,
    Path((id, comment_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<PullRequestCommentDto>> {
    let pull = state.store.get_pull_request(&id).await?;
    let session = own_session(&state, &headers, &pull).await?;
    if session.pull_request_id.as_deref() != Some(pull.id.as_str()) {
        return Err(ApiError::forbidden(format!(
            "only the review session of pull request {} resolves a thread",
            pull.id
        )));
    }
    let comment = state
        .store
        .get_pull_request_comment(&pull.id, &comment_id)
        .await?;
    let forge = integration(&state, &pull.repository_id).await?;
    let login = forge.login.clone().unwrap_or_default();
    let thread = state
        .store
        .list_pull_request_comments(&pull.id, false, &login)
        .await?
        .into_iter()
        .filter(|c| c.thread_id == comment.thread_id)
        .collect::<Vec<_>>();
    let opened_by_me = thread
        .first()
        .is_some_and(|first| first.author_login.eq_ignore_ascii_case(&login));
    if !opened_by_me {
        return Err(ApiError::forbidden(format!(
            "thread {} was not opened by {login}: its author resolves it",
            comment.thread_id
        )));
    }
    if !comment.resolved {
        ForgeClient::for_repository(&state.launcher.cfg, &forge)
            .resolve(&slug(&forge), pull.number, &comment)
            .await
            .map_err(forge_error)?;
        state
            .store
            .resolve_pull_request_thread(&pull.id, &comment.thread_id, &login)
            .await?;
    }
    let stored = state
        .store
        .get_pull_request_comment(&pull.id, &comment.id)
        .await?;
    state.notify_scheduler_pull_request(&pull.id);
    Ok(Json(pull_request_comment_dto(stored)))
}

/// What the request's session says of it: `ready` once every required
/// approval and check reads green, which raises `waiting_user` on the
/// session, and a `state` a human moved it to.
#[utoipa::path(post, path = "/v1/pull-requests/{id}/report", tag = "pull-requests",
    params(("id" = String, Path)),
    request_body = ReportPullRequestRequest,
    responses((status = 200, body = PullRequestDto), (status = 400), (status = 403), (status = 404)))]
pub(super) async fn report(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ReportPullRequestRequest>,
) -> ApiResult<Json<PullRequestDto>> {
    let mut pull = state.store.get_pull_request(&id).await?;
    let session = own_session(&state, &headers, &pull).await?;
    if let Some(ready) = req.ready {
        let (row, moved) = state.store.set_pull_request_ready(&pull.id, ready).await?;
        pull = row;
        if moved && ready {
            state
                .store
                .set_session_attention(&session.id, AttentionReason::WaitingUser)
                .await?;
        } else if moved && session.attention_reason() == Some(AttentionReason::WaitingUser) {
            state.store.clear_session_attention(&session.id).await?;
        }
    }
    if let Some(next) = req.state.as_deref() {
        pull = state.store.set_pull_request_state(&pull.id, next).await?;
    }
    // A review posted on a new head is the user's to act on: the approval
    // is theirs to give (029). The same sha again raises nothing.
    if let Some(sha) = req.reviewed_sha.as_deref() {
        if pull.role != "reviewer" && !pull.review_asked {
            return Err(ApiError::bad_request(
                "only a request Ariadne reviews takes a reviewed_sha",
            ));
        }
        if !is_sha(sha) {
            return Err(ApiError::bad_request(format!("{sha} is no commit sha")));
        }
        let (row, moved) = state.store.set_pull_request_reviewed(&pull.id, sha).await?;
        pull = row;
        if moved {
            if session.attention_reason() == Some(AttentionReason::WaitingUser) {
                state.store.clear_session_attention(&session.id).await?;
            }
            state
                .store
                .set_session_attention(&session.id, AttentionReason::WaitingUser)
                .await?;
        }
    }
    state.notify_scheduler_pull_request(&pull.id);
    Ok(Json(pull_request_dto_of(&state.store, pull).await?))
}

/// Whether `text` reads as a commit sha, whole or abbreviated: what a
/// reviewer session names a head by, and nothing git would read as an
/// option.
fn is_sha(text: &str) -> bool {
    (4..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The change under review, read in the reviewer session's worktree (029):
/// `git diff <base>...HEAD`, or `git diff <since>..HEAD` where `since` is
/// given. The base is the remote's copy of the base branch where the
/// checkout holds one, else the local branch.
#[utoipa::path(get, path = "/v1/pull-requests/{id}/diff", tag = "pull-requests",
    params(("id" = String, Path), PullRequestDiffQuery),
    responses((status = 200, content_type = "text/plain", body = String), (status = 400), (status = 403), (status = 404), (status = 409)))]
pub(super) async fn diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PullRequestDiffQuery>,
    headers: HeaderMap,
) -> ApiResult<String> {
    let pull = state.store.get_pull_request(&id).await?;
    let session = own_session(&state, &headers, &pull).await?;
    let worktree = session
        .worktree_path
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_dir())
        .ok_or_else(|| ApiError::conflict("the session has no worktree"))?;
    let git = &state.launcher.git;
    let failed = |e: anyhow::Error| ApiError::conflict(format!("{e:#}"));
    if let Some(since) = q.since.as_deref() {
        if !is_sha(since) || !git.has_commit(&worktree, since).await.map_err(failed)? {
            return Err(ApiError::bad_request(format!(
                "{since} names no commit of the worktree"
            )));
        }
        return git.diff_since(&worktree, since).await.map_err(failed);
    }
    let repo = state.store.get_repository(&pull.repository_id).await?;
    let remote = repo
        .forge
        .as_ref()
        .map_or_else(|| "origin".to_string(), |forge| forge.remote.clone());
    let tracking = format!("{remote}/{}", pull.base_branch);
    let base = match git
        .has_commit(&worktree, &format!("refs/remotes/{tracking}"))
        .await
        .map_err(failed)?
    {
        true => tracking,
        false => pull.base_branch.clone(),
    };
    git.diff(&worktree, &base, "HEAD").await.map_err(failed)
}

/// Post one round of a review of a request, in the user's name (029): its
/// new findings as one review of inline comments, each led by its priority,
/// and its summary, written into the one summary comment the review keeps
/// on the request — posted on the first round, edited on every later one. A
/// round with no new finding posts no review. `request_changes` stands only
/// on a P0 finding, new or still open. Any other event is refused, an
/// approval above all: the user gives every approval. The daemon stores what
/// it posted as comments of the integration login, marked as the review's.
#[utoipa::path(post, path = "/v1/pull-requests/{id}/reviews", tag = "pull-requests",
    params(("id" = String, Path)),
    request_body = SubmitReviewRequest,
    responses((status = 201, body = [PullRequestCommentDto]), (status = 400), (status = 403), (status = 404), (status = 409), (status = 502)))]
pub(super) async fn submit_review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<SubmitReviewRequest>,
) -> ApiResult<(StatusCode, Json<Vec<PullRequestCommentDto>>)> {
    let pull = state.store.get_pull_request(&id).await?;
    own_session(&state, &headers, &pull).await?;
    let asked = match req.event.as_str() {
        "request_changes" => true,
        "comment" => false,
        other => {
            return Err(ApiError::bad_request(format!(
                "a review is request_changes or comment, not {other}: the user gives every approval"
            )));
        }
    };
    if pull.role != "reviewer" && !pull.review_asked {
        return Err(ApiError::conflict(
            "only a request Ariadne reviews takes a review",
        ));
    }
    // A forge takes no change request on a request of its own author's: the
    // review of a request of mine is a comment, whatever it found (029).
    let request_changes = asked && pull.role == "reviewer";
    // The body is the review's one summary: where the review stands, kept
    // on the request in a comment of its own and edited every round.
    let body = req.body.trim().to_string();
    if body.is_empty() {
        return Err(ApiError::bad_request(
            "a review needs a body: the summary of where it stands",
        ));
    }
    let req_has_p0 = req.comments.iter().any(|c| c.priority == "P0");
    let mut comments = Vec::with_capacity(req.comments.len());
    for comment in req.comments {
        if !matches!(comment.priority.as_str(), "P0" | "P1" | "P2") {
            return Err(ApiError::bad_request(format!(
                "a priority is P0, P1 or P2, not {}",
                comment.priority
            )));
        }
        if comment.path.trim().is_empty()
            || comment.line < 1
            || comment.title.trim().is_empty()
            || comment.body.trim().is_empty()
        {
            return Err(ApiError::bad_request(
                "a comment needs a path, a line from 1, a title and a body",
            ));
        }
        // Each finding opens on its priority and its title, then says what
        // goes wrong and how to fix it.
        comments.push(DraftComment {
            path: comment.path,
            line: comment.line,
            body: format!(
                "**[{}] {}**\n\n{}",
                comment.priority,
                comment.title.trim(),
                comment.body.trim()
            ),
        });
    }
    let forge = integration(&state, &pull.repository_id).await?;
    let login = forge.login.clone().unwrap_or_default();
    // A change request is its P0 findings, each on its own line of code: a
    // new one in this round, or an earlier one still open. One with neither
    // is a summary that asks for changes it never shows.
    let open_p0 = state
        .store
        .list_pull_request_comments(&pull.id, false, &login)
        .await?
        .iter()
        .any(|c| c.from_review && !c.resolved && c.body.starts_with("**[P0]"));
    if asked && !req_has_p0 && !open_p0 {
        return Err(ApiError::bad_request(
            "a change request carries each P0 finding as an inline comment on its line",
        ));
    }
    let client = ForgeClient::for_repository(&state.launcher.cfg, &forge);
    // A round with no new finding posts no review: its summary says where
    // the review stands.
    let mut posted = match comments.is_empty() {
        true => Vec::new(),
        false => client
            .submit_review(
                &slug(&forge),
                pull.number,
                &ReviewDraft {
                    request_changes,
                    head_sha: pull.head_sha.clone(),
                    comments,
                },
                &login,
            )
            .await
            .map_err(forge_error)?,
    };
    let (summary_id, written_at) = client
        .write_summary(
            &slug(&forge),
            pull.number,
            pull.summary_comment_id.as_deref(),
            &body,
        )
        .await
        .map_err(forge_error)?;
    if pull.summary_comment_id.as_deref() != Some(summary_id.as_str()) {
        state
            .store
            .set_pull_request_summary(&pull.id, &summary_id)
            .await?;
    }
    posted.push(NewPullRequestComment {
        forge_id: summary_id,
        thread_id: crate::forge::pulls::CONVERSATION.into(),
        kind: "issue_comment".into(),
        author_login: login.clone(),
        author_is_bot: false,
        body,
        path: None,
        line: None,
        in_reply_to: None,
        created_at: written_at,
        resolved: false,
        from_review: true,
    });
    // Posted under the user's login, yet a review's: on a request of their
    // own the task's author answers it (029).
    for comment in &mut posted {
        comment.from_review = true;
    }
    state
        .store
        .upsert_pull_request_comments(&pull.id, &posted, &login)
        .await?;
    let stored = state
        .store
        .list_pull_request_comments(&pull.id, false, &login)
        .await?
        .into_iter()
        .filter(|c| posted.iter().any(|p| p.forge_id == c.forge_id))
        .map(pull_request_comment_dto)
        .collect();
    state.notify_scheduler_pull_request(&pull.id);
    Ok((StatusCode::CREATED, Json(stored)))
}

/// Ask Ariadne to review a request of the user's own on the model the user
/// picks, or stop asking (029): a review session runs on that pin while the
/// request is open and out of draft, and posts one review as a comment in
/// the user's name. A request that asks for the user's review has one
/// already, on the repository's review pin, and takes no asking.
#[utoipa::path(put, path = "/v1/pull-requests/{id}/ariadne-review", tag = "pull-requests",
    params(("id" = String, Path)),
    request_body = AskReviewRequest,
    responses((status = 200, body = PullRequestDto), (status = 404), (status = 409)))]
pub(super) async fn ask_review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AskReviewRequest>,
) -> ApiResult<Json<PullRequestDto>> {
    let pull = state.store.get_pull_request(&id).await?;
    if pull.role != "author" {
        return Err(ApiError::conflict(
            "Ariadne reviews a request that asks for your review on its own: ask only on a \
             request of yours",
        ));
    }
    let pin = match req.asked {
        false => None,
        true => {
            if pull.state != "open" {
                return Err(ApiError::conflict(format!(
                    "the request is {}: Ariadne reviews an open request",
                    pull.state
                )));
            }
            integration(&state, &pull.repository_id).await?;
            let Some(model) = req.model.as_deref().filter(|m| !m.trim().is_empty()) else {
                return Err(ApiError::bad_request(
                    "a review runs on a model: pick one, as agent:model",
                ));
            };
            let pin = super::pins::chosen(
                &state.store,
                &state.agent_registry,
                Some(model),
                req.effort.as_deref(),
            )
            .await?;
            // Each a skill a task agent is staffed on (017); `pr-reviewer`
            // is loaded anyway, so it is not stored twice.
            let mut skills: Vec<String> = Vec::new();
            for name in &req.skills {
                if name == ariadne_store::defaults::PR_REVIEWER_SKILL || skills.contains(name) {
                    continue;
                }
                let skill = state.store.get_skill(name).await?;
                if skill.seat() != ariadne_store::SkillSeat::Task {
                    return Err(ApiError::bad_request(format!(
                        "skill {name} is no reviewer's to load"
                    )));
                }
                skills.push(name.clone());
            }
            Some((pin, skills))
        }
    };
    let row = state
        .store
        .set_pull_request_review_asked(
            &pull.id,
            pin.as_ref().map(|(pin, skills)| (pin, skills.as_slice())),
        )
        .await?;
    // The session is the scheduler's to start, and the details its news is
    // read from the fetch's.
    state.forge_poll.wake(&row.repository_id);
    state.notify_scheduler_pull_request(&row.id);
    Ok(Json(pull_request_dto_of(&state.store, row).await?))
}
