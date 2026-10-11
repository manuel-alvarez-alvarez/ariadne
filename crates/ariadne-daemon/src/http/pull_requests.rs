//! Pull requests, read live off the forge (026), and the requests Ariadne
//! works on: what the forge holds is never stored, so every route here reads
//! it, and joins what Ariadne keeps of a request it works on.
use super::{
    AppState,
    caller::call_ctx,
    convert::{forge_pull_dto, pull_request_comment_dto, pull_request_dto_of},
    error::{ApiError, ApiResult, Json},
};
use crate::forge::live::{self, Details, Live};
use crate::forge::pulls::{CONVERSATION, DraftComment, ReviewDraft};
use crate::forge::{ForgeClient, PullRequestRef, pulls};
use ariadne_api::pull_requests::{
    AskReviewRequest, PullRequestCommentDto, PullRequestCommentQuery, PullRequestDiffQuery,
    PullRequestDto, PullRequestListQuery, PullRequestMatchDto, ReplyCommentRequest,
    ReportPullRequestRequest, SubmitReviewRequest,
};
use ariadne_store::{
    AgentSession, ForgeIntegration, PullRequest, PullRequestComment, PullRequestFilter,
    PullRequestRow,
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
fn client(state: &AppState, forge: &ForgeIntegration) -> ForgeClient {
    ForgeClient::for_repository(&state.launcher.cfg, forge)
}

/// Read a request Ariadne works on off the forge now, and leave the read
/// where the fetch leaves its own (026): what a session asks for is never an
/// earlier read.
async fn read_now(state: &AppState, row: PullRequestRow) -> ApiResult<PullRequest> {
    let forge = integration(state, &row.repository_id).await?;
    let details = client(state, &forge)
        .details(
            &slug(&forge),
            row.number,
            crate::timeouts::Timeouts::default().forge_details,
        )
        .await
        .map_err(forge_error)?;
    if details.pull.number != row.number {
        return Err(forge_error(
            "the forge returned another request number".into(),
        ));
    }
    // A detail read says nothing of whether the request still asks for my
    // review: the last fetch's word stands, and with none yet a request I
    // review still asks until a fetch says otherwise (029).
    let review_requested = state
        .launcher
        .live
        .get(&row.id)
        .map_or(row.role == "reviewer", |l| l.review_requested);
    let head_sha = details.pull.head_sha.clone();
    state.launcher.live.set(
        &row.id,
        Live {
            pull: details.pull,
            review_requested,
            details: Some(Details {
                comments: details.comments,
                failed_checks: details.failed_checks,
                behind_base: details.behind_base,
                head_sha,
            }),
        },
    );
    live::of_row(&state.store, &state.launcher.live, row)
        .await?
        .ok_or_else(|| ApiError::conflict("the request could not be read off the forge"))
}

/// A request Ariadne works on, as the last read found it, or read now where
/// nothing has read it yet.
async fn read_held(state: &AppState, row: PullRequestRow) -> ApiResult<PullRequest> {
    match live::of_row(&state.store, &state.launcher.live, row.clone()).await? {
        Some(pull) => Ok(pull),
        None => read_now(state, row).await,
    }
}

/// The open requests of the enabled repositories, read live off the forge
/// (026): every one, or with `role` and `requested` the user's own or the
/// ones that ask for their review, each with what Ariadne keeps of it where
/// it works on it. With `task`, the request a task opened, which its author
/// keeps (005).
#[utoipa::path(get, path = "/v1/pull-requests", tag = "pull-requests", params(PullRequestListQuery), responses((status = 200, body = [PullRequestDto]), (status = 502)))]
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
    if let Some(task) = q.task {
        let rows = state
            .store
            .list_pull_requests(PullRequestFilter {
                origin_task_id: Some(task),
                ..Default::default()
            })
            .await?;
        let mut dtos = Vec::with_capacity(rows.len());
        for row in rows {
            let pull = read_held(&state, row).await?;
            dtos.push(pull_request_dto_of(&state.store, pull).await?);
        }
        return Ok(Json(dtos));
    }
    let forges = match &q.repo {
        Some(id) => vec![integration(&state, id).await?],
        None => state.store.enabled_forge_integrations().await?,
    };
    let mut dtos = Vec::new();
    for forge in forges {
        let login = forge.login.clone().unwrap_or_default();
        let pulls::Listed { open, requested } = client(&state, &forge)
            .list_open_pull_requests(&slug(&forge), &login)
            .await
            .map_err(forge_error)?;
        for pull in open {
            let role = pulls::role(&pull.author_login, &forge);
            let asks = requested.contains(&pull.number);
            if q.role.as_deref().is_some_and(|wanted| wanted != role)
                || q.requested.is_some_and(|wanted| wanted != asks)
            {
                continue;
            }
            let row = state
                .store
                .pull_request_by_number(&forge.repository_id, pull.number)
                .await?;
            dtos.push(match row {
                // Ariadne works on it: the list's read beside the details the
                // last detail read found.
                Some(row) => {
                    let details = state.launcher.live.get(&row.id).and_then(|l| l.details);
                    let read = Live {
                        pull,
                        review_requested: asks,
                        details,
                    };
                    let marks = state.store.pull_request_comment_marks(&row.id).await?;
                    let view = live::view(row, &read, &marks, &login);
                    pull_request_dto_of(&state.store, view).await?
                }
                None => forge_pull_dto(&forge.repository_id, pull, role, asks),
            });
        }
    }
    dtos.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(Json(dtos))
}

/// A request Ariadne works on, read off the forge now.
#[utoipa::path(get, path = "/v1/pull-requests/{id}", tag = "pull-requests", params(("id" = String, Path)), responses((status = 200, body = PullRequestDto), (status = 404), (status = 502)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<PullRequestDto>> {
    let row = state.store.get_pull_request(&id).await?;
    refuse_other_sessions(&state, &headers, &row).await?;
    let pull = read_now(&state, row).await?;
    Ok(Json(pull_request_dto_of(&state.store, pull).await?))
}

/// One open request of a repository, read off the forge now (026): its
/// own read, its checks and its comments, with what Ariadne keeps of it
/// where it works on it. What the desktop's panel shows.
#[utoipa::path(get, path = "/v1/repositories/{id}/pull-requests/{number}", tag = "pull-requests", params(("id" = String, Path), ("number" = i64, Path)), responses((status = 200, body = PullRequestDto), (status = 409), (status = 502)))]
pub(super) async fn get_by_number(
    State(state): State<AppState>,
    Path((id, number)): Path<(String, i64)>,
) -> ApiResult<Json<PullRequestDto>> {
    if let Some(row) = state.store.pull_request_by_number(&id, number).await? {
        let pull = read_now(&state, row).await?;
        return Ok(Json(pull_request_dto_of(&state.store, pull).await?));
    }
    let forge = integration(&state, &id).await?;
    let login = forge.login.clone().unwrap_or_default();
    let details = client(&state, &forge)
        .details(
            &slug(&forge),
            number,
            crate::timeouts::Timeouts::default().forge_details,
        )
        .await
        .map_err(forge_error)?;
    let role = pulls::role(&details.pull.author_login, &forge);
    let comments = live::answered(live::comments("", &details.comments, &[]), role, &login);
    let unanswered = live::waiting_threads(&comments, role, &login).len() as i64;
    let mut dto = forge_pull_dto(&id, details.pull, role, false);
    dto.unanswered_comments = unanswered;
    dto.behind_base = details.behind_base;
    dto.failed_checks = details
        .failed_checks
        .into_iter()
        .map(|c| ariadne_api::pull_requests::FailedCheckDto {
            name: c.name,
            url: c.url,
            conclusion: c.conclusion,
        })
        .collect();
    Ok(Json(dto))
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
    let matches = client(&state, &forge)
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
/// (029), or, for a request a task opened, by the agent of that task's
/// current column, which keeps it (030).
async fn own_session(
    state: &AppState,
    headers: &HeaderMap,
    row: &PullRequestRow,
) -> ApiResult<AgentSession> {
    let ctx = call_ctx(&state.store, headers).await?;
    if row.role == "author"
        && let Some(task_id) = row.origin_task_id.as_deref()
        && let Some(session) = ctx.session.as_ref()
        && session.task_id.as_deref() == Some(task_id)
    {
        let task = state.store.get_task(task_id).await?;
        super::steps::current_agent(state, &ctx, &task).await?;
        return Ok(session.clone());
    }
    match ctx.session {
        Some(session) if session.pull_request_id.as_deref() == Some(row.id.as_str()) => Ok(session),
        Some(session) => Err(ApiError::forbidden(format!(
            "session {} does not watch pull request {}",
            session.id, row.id
        ))),
        None => Err(ApiError::forbidden(format!(
            "only the session of pull request {} answers for it",
            row.id
        ))),
    }
}

/// A session that names a request other than its own is refused; the user
/// reads every request.
async fn refuse_other_sessions(
    state: &AppState,
    headers: &HeaderMap,
    row: &PullRequestRow,
) -> ApiResult<()> {
    match call_ctx(&state.store, headers).await?.session {
        Some(_) => own_session(state, headers, row).await.map(drop),
        None => Ok(()),
    }
}

/// The comments of a request Ariadne works on, read off the forge now, with
/// the marks beside them; with `unanswered_only`, the threads that wait on
/// the integration login.
#[utoipa::path(get, path = "/v1/pull-requests/{id}/comments", tag = "pull-requests",
    params(("id" = String, Path), PullRequestCommentQuery),
    responses((status = 200, body = [PullRequestCommentDto]), (status = 403), (status = 404), (status = 502)))]
pub(super) async fn comments(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PullRequestCommentQuery>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<PullRequestCommentDto>>> {
    let row = state.store.get_pull_request(&id).await?;
    refuse_other_sessions(&state, &headers, &row).await?;
    let pull = read_now(&state, row).await?;
    let comments = live::comments_of(&state.store, &state.launcher.live, &pull).await?;
    let comments = match q.unanswered_only.unwrap_or(false) {
        true => {
            let login = live::login_of(&state.store, &pull.repository_id).await?;
            live::waiting(&comments, &pull.role, &login)
        }
        false => comments,
    };
    Ok(Json(
        comments.into_iter().map(pull_request_comment_dto).collect(),
    ))
}

/// One comment of a request Ariadne works on, read off the forge now.
#[utoipa::path(get, path = "/v1/pull-requests/{id}/comments/{comment_id}", tag = "pull-requests",
    params(("id" = String, Path), ("comment_id" = String, Path)),
    responses((status = 200, body = PullRequestCommentDto), (status = 403), (status = 404), (status = 502)))]
pub(super) async fn comment(
    State(state): State<AppState>,
    Path((id, comment_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<PullRequestCommentDto>> {
    let row = state.store.get_pull_request(&id).await?;
    refuse_other_sessions(&state, &headers, &row).await?;
    let pull = read_now(&state, row).await?;
    let comment = find_comment(&state, &pull, &comment_id).await?;
    Ok(Json(pull_request_comment_dto(comment)))
}

/// A comment of `pull` by its forge id, as the last read found it, or as a
/// read now finds it where the last one does not hold it yet.
async fn find_comment(
    state: &AppState,
    pull: &PullRequest,
    comment_id: &str,
) -> ApiResult<PullRequestComment> {
    let held = live::comments_of(&state.store, &state.launcher.live, pull).await?;
    if let Some(comment) = held.into_iter().find(|c| c.id == comment_id) {
        return Ok(comment);
    }
    let row = state.store.get_pull_request(&pull.id).await?;
    let pull = read_now(state, row).await?;
    live::comments_of(&state.store, &state.launcher.live, &pull)
        .await?
        .into_iter()
        .find(|c| c.id == comment_id)
        .ok_or_else(|| {
            ariadne_store::StoreError::NotFound {
                entity: "pull_request_comment",
                id: comment_id.to_string(),
            }
            .into()
        })
}

/// Reply to one comment. The daemon posts the reply through the forge CLI,
/// marks it the review's where a review session posts it (029), and reads
/// the request again.
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
    let row = state.store.get_pull_request(&id).await?;
    let session = own_session(&state, &headers, &row).await?;
    // A review session's reply is the review's, as its findings are.
    let from_review = session.pull_request_id.as_deref() == Some(row.id.as_str());
    let body = req.body.trim();
    if body.is_empty() {
        return Err(ApiError::bad_request("a reply needs a body"));
    }
    let pull = read_held(&state, row).await?;
    let comment = find_comment(&state, &pull, &comment_id).await?;
    let forge = integration(&state, &pull.repository_id).await?;
    let login = forge.login.clone().unwrap_or_default();
    // A review's reply is signed as the review's (029).
    let sent = match from_review {
        true => pulls::signed(body, false),
        false => body.to_string(),
    };
    let forge_id = client(&state, &forge)
        .reply(&slug(&forge), pull.number, &comment, &sent)
        .await
        .map_err(forge_error)?;
    if from_review {
        state
            .store
            .mark_review_comments(&pull.id, std::slice::from_ref(&forge_id))
            .await?;
    }
    let in_reply_to = match comment.kind.as_str() {
        "review_comment" => comment
            .in_reply_to
            .clone()
            .or(Some(comment.forge_id.clone())),
        _ => None,
    };
    let posted = PullRequestComment {
        id: forge_id.clone(),
        pull_request_id: pull.id.clone(),
        forge_id,
        thread_id: comment.thread_id.clone(),
        kind: match comment.kind.as_str() {
            "review_comment" => "review_comment",
            _ => "issue_comment",
        }
        .into(),
        author_login: login,
        author_is_bot: false,
        body: body.to_string(),
        path: comment.path.clone(),
        line: comment.line,
        in_reply_to,
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        answered: false,
        resolved: comment.resolved,
        told_at: None,
        from_review,
    };
    state.launcher.live.add_comments(
        &pull.id,
        &[ariadne_store::NewPullRequestComment {
            forge_id: posted.forge_id.clone(),
            thread_id: posted.thread_id.clone(),
            kind: posted.kind.clone(),
            author_login: posted.author_login.clone(),
            author_is_bot: false,
            body: posted.body.clone(),
            path: posted.path.clone(),
            line: posted.line,
            in_reply_to: posted.in_reply_to.clone(),
            created_at: posted.created_at.clone(),
            resolved: posted.resolved,
            from_review,
        }],
    );
    state.forge_poll.wake(&pull.repository_id);
    state.notify_scheduler_pull_request(&pull.id);
    Ok((StatusCode::CREATED, Json(pull_request_comment_dto(posted))))
}

/// Resolve the thread of one comment, once a push fixed what it found
/// (029). Only a review session resolves, and only a thread it opened under
/// the integration login: the threads of anyone else stay open for the
/// person who wrote them.
#[utoipa::path(post, path = "/v1/pull-requests/{id}/comments/{comment_id}/resolve", tag = "pull-requests",
    params(("id" = String, Path), ("comment_id" = String, Path)),
    responses((status = 200, body = PullRequestCommentDto), (status = 403), (status = 404), (status = 409), (status = 502)))]
pub(super) async fn resolve(
    State(state): State<AppState>,
    Path((id, comment_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Json<PullRequestCommentDto>> {
    let row = state.store.get_pull_request(&id).await?;
    let session = own_session(&state, &headers, &row).await?;
    if session.pull_request_id.as_deref() != Some(row.id.as_str()) {
        return Err(ApiError::forbidden(format!(
            "only the review session of pull request {} resolves a thread",
            row.id
        )));
    }
    let pull = read_held(&state, row).await?;
    let mut comment = find_comment(&state, &pull, &comment_id).await?;
    let forge = integration(&state, &pull.repository_id).await?;
    let login = forge.login.clone().unwrap_or_default();
    let opened_by_me = live::comments_of(&state.store, &state.launcher.live, &pull)
        .await?
        .into_iter()
        .find(|c| c.thread_id == comment.thread_id)
        .is_some_and(|first| first.author_login.eq_ignore_ascii_case(&login));
    if !opened_by_me {
        return Err(ApiError::forbidden(format!(
            "thread {} was not opened by {login}: its author resolves it",
            comment.thread_id
        )));
    }
    if !comment.resolved {
        client(&state, &forge)
            .resolve(&slug(&forge), pull.number, &comment)
            .await
            .map_err(forge_error)?;
        comment.resolved = true;
    }
    state.forge_poll.wake(&pull.repository_id);
    state.notify_scheduler_pull_request(&pull.id);
    Ok(Json(pull_request_comment_dto(comment)))
}

/// What the request's session says of it: `ready` once the babysitting task
/// believes every required approval and check reads green, and the head a
/// review session posted its review on. Neither raises `waiting_user` on
/// the session by itself (029): the claim alone is not confirmed evidence,
/// and a pr-reviewer session finishing its own review must notify nobody —
/// the `pull_request` attention producer reads the forge's own evidence
/// against this claim and raises its own item once it actually backs the
/// claim up.
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
    let mut row = state.store.get_pull_request(&id).await?;
    let session = own_session(&state, &headers, &row).await?;
    if let Some(ready) = req.ready {
        // `ready` is the babysitting task's own claim (031): a row it
        // keeps answers through its own column agent alone
        // (`Seat::Agent`), never through a reviewer session that also
        // happens to answer for the same row — a request of mine a task
        // keeps can also carry an Ariadne self-review asked on it, and
        // that reviewer seat's own claim must never pass as the
        // babysitter's.
        if row.origin_task_id.is_some() && session.seat() != Some(ariadne_core::Seat::Agent) {
            return Err(ApiError::forbidden(
                "only the task that keeps this request may report it ready",
            ));
        }
        // The head this report answers for is the live read's own, at the
        // moment of the report: a readiness item answers for exactly this
        // revision (031), never for whichever one a later fetch happens to
        // find.
        let head_sha = state
            .launcher
            .live
            .get(&row.id)
            .map(|live| live.pull.head_sha);
        let (next, _moved) = state
            .store
            .set_pull_request_ready(&row.id, ready, head_sha.as_deref())
            .await?;
        row = next;
    }
    // A review posted on a new head is kept for the record (029); the
    // user's approval is theirs to give in their own time, and this alone
    // asks nothing of them.
    if let Some(sha) = req.reviewed_sha.as_deref() {
        if row.role != "reviewer" && !row.review_asked {
            return Err(ApiError::bad_request(
                "only a request Ariadne reviews takes a reviewed_sha",
            ));
        }
        if !is_sha(sha) {
            return Err(ApiError::bad_request(format!("{sha} is no commit sha")));
        }
        let (next, _moved) = state.store.set_pull_request_reviewed(&row.id, sha).await?;
        row = next;
    }
    state.notify_scheduler_pull_request(&row.id);
    let pull = read_held(&state, row).await?;
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
    let row = state.store.get_pull_request(&id).await?;
    let session = own_session(&state, &headers, &row).await?;
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
    let pull = read_held(&state, row).await?;
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
/// approval above all: the user gives every approval. What it posted is
/// marked as the review's.
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
    let row = state.store.get_pull_request(&id).await?;
    own_session(&state, &headers, &row).await?;
    let asked = match req.event.as_str() {
        "request_changes" => true,
        "comment" => false,
        other => {
            return Err(ApiError::bad_request(format!(
                "a review is request_changes or comment, not {other}: the user gives every approval"
            )));
        }
    };
    if row.role != "reviewer" && !row.review_asked {
        return Err(ApiError::conflict(
            "only a request Ariadne reviews takes a review",
        ));
    }
    // A forge takes no change request on a request of its own author's: the
    // review of a request of mine is a comment, whatever it found (029).
    let request_changes = asked && row.role == "reviewer";
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
            body: pulls::signed(
                &format!(
                    "**[{}] {}**\n\n{}",
                    comment.priority,
                    comment.title.trim(),
                    comment.body.trim()
                ),
                false,
            ),
        });
    }
    let pull = read_held(&state, row).await?;
    let forge = integration(&state, &pull.repository_id).await?;
    let login = forge.login.clone().unwrap_or_default();
    // A change request is its P0 findings, each on its own line of code: a
    // new one in this round, or an earlier one still open. One with neither
    // is a summary that asks for changes it never shows.
    let open_p0 = live::comments_of(&state.store, &state.launcher.live, &pull)
        .await?
        .iter()
        .any(|c| c.from_review && !c.resolved && c.body.starts_with("**[P0]"));
    if asked && !req_has_p0 && !open_p0 {
        return Err(ApiError::bad_request(
            "a change request carries each P0 finding as an inline comment on its line",
        ));
    }
    let forge_client = client(&state, &forge);
    // A round with no new finding posts no review: its summary says where
    // the review stands.
    let mut posted = match comments.is_empty() {
        true => Vec::new(),
        false => forge_client
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
    // The findings are the review's the moment the forge holds them: marked
    // and held before the summary is written, so a summary that fails loses
    // none of them, and the round is finished by a call with no comments.
    hold_review_comments(&state, &pull.id, &mut posted).await?;
    // The summary the row names, else the one the forge holds under its
    // mark: a review stopped and asked again finds its own summary.
    let existing = pull.summary_comment_id.clone().or_else(|| {
        state
            .launcher
            .live
            .get(&pull.id)
            .and_then(|l| l.details)
            .and_then(|d| {
                d.comments
                    .into_iter()
                    .filter(|c| {
                        c.thread_id == CONVERSATION
                            && c.author_login.eq_ignore_ascii_case(&login)
                            && pulls::signature(&c.body).summary
                    })
                    .map(|c| c.forge_id)
                    .next_back()
            })
    });
    let written = forge_client
        .write_summary(
            &slug(&forge),
            pull.number,
            existing.as_deref(),
            &pulls::signed(&body, true),
        )
        .await;
    let (summary_id, written_at) = match written {
        Ok(written) => written,
        Err(error) if !posted.is_empty() => {
            return Err(forge_error(format!(
                "the {} new findings are posted; the summary was not written: {error}. \
                 Call submit_review again with the summary and no comments",
                posted.len()
            )));
        }
        Err(error) => return Err(forge_error(error)),
    };
    if pull.summary_comment_id.as_deref() != Some(summary_id.as_str()) {
        state
            .store
            .set_pull_request_summary(&pull.id, &summary_id)
            .await?;
    }
    let mut summary = vec![ariadne_store::NewPullRequestComment {
        forge_id: summary_id,
        thread_id: CONVERSATION.into(),
        kind: "issue_comment".into(),
        author_login: login,
        author_is_bot: false,
        body,
        path: None,
        line: None,
        in_reply_to: None,
        created_at: written_at,
        resolved: false,
        from_review: true,
    }];
    hold_review_comments(&state, &pull.id, &mut summary).await?;
    posted.extend(summary);
    let ids: Vec<String> = posted.iter().map(|c| c.forge_id.clone()).collect();
    // In the order they were posted: the findings, then the summary.
    let views = live::comments(&pull.id, &posted, &[]);
    let answered: Vec<PullRequestCommentDto> = ids
        .iter()
        .filter_map(|id| views.iter().find(|c| &c.id == id).cloned())
        .map(pull_request_comment_dto)
        .collect();
    state.forge_poll.wake(&pull.repository_id);
    state.notify_scheduler_pull_request(&pull.id);
    Ok((StatusCode::CREATED, Json(answered)))
}

/// Mark what a review posted as the review's, and hold it beside the last
/// read until the next fetch reads it back (029): posted under the user's
/// login, yet on a request of their own the task's author answers it.
async fn hold_review_comments(
    state: &AppState,
    pull_request_id: &str,
    posted: &mut [ariadne_store::NewPullRequestComment],
) -> ApiResult<()> {
    if posted.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = posted.iter().map(|c| c.forge_id.clone()).collect();
    state
        .store
        .mark_review_comments(pull_request_id, &ids)
        .await?;
    for comment in posted.iter_mut() {
        comment.from_review = true;
    }
    state.launcher.live.add_comments(pull_request_id, posted);
    Ok(())
}

/// Ask Ariadne to review a request on the model the user picks, or stop
/// asking (029): a review session runs on that pin while the request is
/// open and out of draft. A request of the user's own posts each round as
/// a comment in the user's name; one that asks for the user's review on a
/// repository with no review pin of its own gets the manual start the
/// `pull_request` attention producer offers when nobody is assigned to it,
/// and posts as any other review Ariadne runs on it would (029). Asking
/// starts Ariadne's work on the request; stopping ends it, where no task
/// keeps the request. A request that asks for the user's review on a
/// repository that already pins one has a review already, and takes no
/// asking.
#[utoipa::path(put, path = "/v1/repositories/{id}/pull-requests/{number}/ariadne-review", tag = "pull-requests",
    params(("id" = String, Path), ("number" = i64, Path)),
    request_body = AskReviewRequest,
    responses((status = 200, body = PullRequestDto), (status = 400), (status = 404), (status = 409), (status = 502)))]
pub(super) async fn ask_review(
    State(state): State<AppState>,
    Path((id, number)): Path<(String, i64)>,
    Json(req): Json<AskReviewRequest>,
) -> ApiResult<Json<PullRequestDto>> {
    let forge = integration(&state, &id).await?;
    let pull = client(&state, &forge)
        .pull_request(&slug(&forge), number)
        .await
        .map_err(forge_error)?;
    if pull.number != number {
        return Err(forge_error(
            "the forge returned another request number".into(),
        ));
    }
    let role = pulls::role(&pull.author_login, &forge);
    let row = state.store.pull_request_by_number(&id, number).await?;
    let pin = match req.asked {
        false => None,
        true => {
            // A request of mine asks on any pin; a request that asks for
            // my review asks only where the repository names none of its
            // own (029): one that already does has a review of its own on
            // that pin, and an ask here would run a second session beside
            // it.
            if role == "reviewer" && forge.review_model.is_some() {
                return Err(ApiError::conflict(
                    "the request already has a review of its own, on the repository's review \
                     pin",
                ));
            }
            if pull.state != "open" {
                return Err(ApiError::conflict(format!(
                    "the request is {}: Ariadne reviews an open request",
                    pull.state
                )));
            }
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
    let row = match (row, &pin) {
        (Some(row), _) => row,
        // Asking starts the work on the request.
        (None, Some(_)) => {
            pulls::start_work(&state.store, &forge, &pull, None)
                .await
                .map_err(forge_error)?
                .0
        }
        // Nothing works on it, so there is no asking to stop.
        (None, None) => {
            return Ok(Json(forge_pull_dto(&id, pull, role, false)));
        }
    };
    let row = state
        .store
        .set_pull_request_review_asked(
            &row.id,
            pin.as_ref().map(|(pin, skills)| (pin, skills.as_slice())),
        )
        .await?;
    // A request of mine never asks for my own review, so `false` names it
    // correctly; a request that asks for mine is exactly this call's own
    // premise (rule 10: asking it takes the same ask a request of mine
    // does) — reading it any other way here would read as "no longer
    // asks" the moment this write lands, through either a stale `false`
    // this row never actually held or one a row fresh off this very call
    // never got the chance to hold yet. `review_pass` would then read
    // `wants_session` false and `end_review` would delete the row as
    // `done`, before the next fetch could correct it.
    let review_requested = role == "reviewer";
    state
        .launcher
        .live
        .set_pull(&row.id, pull.clone(), review_requested);
    // The session is the scheduler's to start, and the details its news is
    // read from the fetch's.
    state.forge_poll.wake(&row.repository_id);
    state.notify_scheduler_pull_request(&row.id);
    let view = read_held(&state, row).await?;
    Ok(Json(pull_request_dto_of(&state.store, view).await?))
}
