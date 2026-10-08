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
    AddPullRequestRequest, PullRequestCommentDto, PullRequestCommentQuery, PullRequestDiffQuery,
    PullRequestDto, PullRequestListQuery, PullRequestMatchDto, ReplyCommentRequest,
    ReportPullRequestRequest, SubmitReviewRequest,
};
use ariadne_core::AttentionReason;
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
        result.push(PullRequestMatchDto {
            number: reference.number,
            role: pulls::role(&pull.author_login, &forge).into(),
            url: pull.url,
            title: pull.title,
            author_login: pull.author_login,
            tracked: rows.iter().any(|row| row.number == reference.number),
        });
    }
    Ok(Json(result))
}

/// The session a call came from, refused unless it is the request's own: a
/// request is answered for by the session the daemon started on it alone.
async fn own_session(
    state: &AppState,
    headers: &HeaderMap,
    pull: &PullRequest,
) -> ApiResult<AgentSession> {
    let ctx = call_ctx(&state.store, headers).await?;
    match ctx.session {
        Some(session) if session.pull_request_id.as_deref() == Some(pull.id.as_str()) => {
            Ok(session)
        }
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
/// the threads that wait again. There is no route that resolves a thread:
/// a human closes a thread.
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
    own_session(&state, &headers, &pull).await?;
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
        if pull.role != "reviewer" {
            return Err(ApiError::bad_request(
                "only a request you review takes a reviewed_sha",
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

/// Post one review of a request the user reviews, in the user's name
/// (029): `request_changes` or `comment`, with inline comments each led by
/// its priority. Any other event is refused, an approval above all: the
/// user gives every approval. The daemon stores what it posted as comments
/// of the integration login.
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
    let request_changes = match req.event.as_str() {
        "request_changes" => true,
        "comment" => false,
        other => {
            return Err(ApiError::bad_request(format!(
                "a review is request_changes or comment, not {other}: the user gives every approval"
            )));
        }
    };
    if pull.role != "reviewer" {
        return Err(ApiError::conflict(
            "only a request you review takes a review",
        ));
    }
    let body = req.body.trim().to_string();
    if body.is_empty() && req.comments.is_empty() {
        return Err(ApiError::bad_request("a review needs a body or a comment"));
    }
    let mut comments = Vec::with_capacity(req.comments.len());
    for comment in req.comments {
        if !matches!(comment.priority.as_str(), "P0" | "P1" | "P2") {
            return Err(ApiError::bad_request(format!(
                "a priority is P0, P1 or P2, not {}",
                comment.priority
            )));
        }
        if comment.path.trim().is_empty() || comment.line < 1 || comment.body.trim().is_empty() {
            return Err(ApiError::bad_request(
                "a comment needs a path, a line from 1 and a body",
            ));
        }
        comments.push(DraftComment {
            path: comment.path,
            line: comment.line,
            body: format!("{}: {}", comment.priority, comment.body.trim()),
        });
    }
    let forge = integration(&state, &pull.repository_id).await?;
    let login = forge.login.clone().unwrap_or_default();
    let draft = ReviewDraft {
        request_changes,
        body,
        head_sha: pull.head_sha.clone(),
        comments,
    };
    let posted = ForgeClient::for_repository(&state.launcher.cfg, &forge)
        .submit_review(&slug(&forge), pull.number, &draft, &login)
        .await
        .map_err(forge_error)?;
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
