//! Drafts a repository that supervises its posts holds for the user (026):
//! a reply, a review or a resolve an agent wrote, listed, edited, discarded
//! or published here — the forge sees none of it until the user publishes.
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use ariadne_api::pull_requests::{
    DiscardDraftRequest, DraftDto, DraftListQuery, ReplyDraftPayload, UpdateDraftRequest,
};
use ariadne_store::PullRequestDraft;

use super::{
    AppState,
    convert::draft_dto,
    error::{ApiError, ApiResult, Json},
    pull_requests,
};
use crate::forge::pulls;

/// The drafts of one request; with `state`, only those in it.
#[utoipa::path(get, path = "/v1/pull-requests/{id}/drafts", tag = "pull-requests",
    params(("id" = String, Path), DraftListQuery),
    responses((status = 200, body = [DraftDto])))]
pub(super) async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<DraftListQuery>,
) -> ApiResult<Json<Vec<DraftDto>>> {
    let drafts = state
        .store
        .list_pull_request_drafts(&id, q.state.as_deref())
        .await?;
    Ok(Json(drafts.into_iter().map(draft_dto).collect()))
}

/// One draft by id.
#[utoipa::path(get, path = "/v1/drafts/{id}", tag = "pull-requests",
    params(("id" = String, Path)),
    responses((status = 200, body = DraftDto), (status = 404)))]
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<DraftDto>> {
    Ok(Json(draft_dto(
        state.store.get_pull_request_draft(&id).await?,
    )))
}

/// Replace the payload of a pending draft; a draft that is not pending
/// answers 409. The payload must fit the draft's own kind — for `reply`,
/// `{comment_id, body}` — else 400, rather than a shape publish only finds
/// broken later.
#[utoipa::path(put, path = "/v1/drafts/{id}", tag = "pull-requests",
    params(("id" = String, Path)), request_body = UpdateDraftRequest,
    responses((status = 200, body = DraftDto), (status = 400), (status = 404), (status = 409)))]
pub(super) async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateDraftRequest>,
) -> ApiResult<Json<DraftDto>> {
    let kind = state.store.get_pull_request_draft(&id).await?.kind;
    validate_payload(&kind, &req.payload)?;
    let payload =
        serde_json::to_string(&req.payload).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let draft = state
        .store
        .update_pull_request_draft_payload(&id, &payload)
        .await?;
    Ok(Json(draft_dto(draft)))
}

/// Refuse a payload that does not fit `kind`'s own shape — for `reply`,
/// [`ReplyDraftPayload`]. `review` and `resolve` have no shape of their own
/// yet (rule: publish answers them 501), so nothing here refuses one.
fn validate_payload(kind: &str, payload: &serde_json::Value) -> ApiResult<()> {
    if kind == "reply" {
        serde_json::from_value::<ReplyDraftPayload>(payload.clone())
            .map_err(|e| ApiError::bad_request(format!("invalid reply payload: {e}")))?;
    }
    Ok(())
}

/// Discard a pending draft: the forge never sees it, and the comment it
/// answered reads unanswered again. `feedback`, where given, says why; the
/// body is optional, since a plain `DELETE` with none carries no
/// `Content-Type` for the usual `Json` extractor to read.
#[utoipa::path(delete, path = "/v1/drafts/{id}", tag = "pull-requests",
    params(("id" = String, Path)), request_body = DiscardDraftRequest,
    responses((status = 200, body = DraftDto), (status = 404), (status = 409)))]
pub(super) async fn discard(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<DraftDto>> {
    let req: DiscardDraftRequest = match body.is_empty() {
        true => DiscardDraftRequest::default(),
        false => serde_json::from_slice(&body).map_err(|e| ApiError::bad_request(e.to_string()))?,
    };
    let draft = state.store.get_pull_request_draft(&id).await?;
    if draft.state != "pending" {
        return Err(ApiError::conflict(format!(
            "draft {id} is {}, not pending",
            draft.state
        )));
    }
    if let Some(feedback) = req.feedback.as_deref().filter(|f| !f.is_empty()) {
        state
            .store
            .set_pull_request_draft_feedback(&id, feedback)
            .await?;
    }
    // The comment a reply draft answered reads unanswered again: clear the
    // told mark alone, nothing else of the row (026 rule 26), so the next
    // news tells it again.
    if draft.kind == "reply"
        && let Ok(payload) = serde_json::from_str::<ReplyDraftPayload>(&draft.payload)
    {
        state
            .store
            .clear_pull_request_comment_told(&draft.pull_request_id, &payload.comment_id)
            .await?;
    }
    let draft = state
        .store
        .set_pull_request_draft_state(&id, "discarded")
        .await?;
    // Poke the scheduler to hand the news to the keeping session, with no
    // forge call needed to see that it moved.
    state.notify_scheduler_pull_request(&draft.pull_request_id);
    Ok(Json(draft_dto(draft)))
}

/// Publish a pending draft: run its forge call, dispatched by kind, and
/// mark it published on success. A forge error leaves it pending.
#[utoipa::path(post, path = "/v1/drafts/{id}/publish", tag = "pull-requests",
    params(("id" = String, Path)),
    responses((status = 200, body = DraftDto), (status = 404), (status = 409), (status = 501), (status = 502)))]
pub(super) async fn publish(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<DraftDto>> {
    let draft = state.store.get_pull_request_draft(&id).await?;
    if draft.state != "pending" {
        return Err(ApiError::conflict(format!(
            "draft {id} is {}, not pending",
            draft.state
        )));
    }
    let draft = match draft.kind.as_str() {
        "reply" => publish_reply(&state, draft).await?,
        kind => {
            return Err(ApiError::new(
                StatusCode::NOT_IMPLEMENTED,
                "not_implemented",
                format!("publishing a {kind} draft is not yet supported"),
            ));
        }
    };
    Ok(Json(draft_dto(draft)))
}

/// Publish a `reply` draft: replay the same forge call and live-view update
/// the direct reply path runs (026), from whatever session wrote the draft
/// rather than the one publishing it.
async fn publish_reply(state: &AppState, draft: PullRequestDraft) -> ApiResult<PullRequestDraft> {
    let payload: ReplyDraftPayload = serde_json::from_str(&draft.payload).map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            e.to_string(),
        )
    })?;
    let row = state.store.get_pull_request(&draft.pull_request_id).await?;
    // Held the same way the direct path holds it: a draft's publish and a
    // direct reply racing it for the same comment cannot both post. Two
    // publish calls on the same draft race the same way, which is why its
    // own state is read again below, under this lock, rather than trusted
    // from before it: the call that waited here must not still act on a
    // draft the one that ran first already moved on from.
    let _reply_lock = state.reply_locks.hold(&row.id).await;
    let draft = state.store.get_pull_request_draft(&draft.id).await?;
    if draft.state != "pending" {
        return Err(ApiError::conflict(format!(
            "draft {} is {}, not pending",
            draft.id, draft.state
        )));
    }
    let pull = pull_requests::read_held(state, row).await?;
    let comment = pull_requests::find_comment(state, &pull, &payload.comment_id).await?;
    let forge = pull_requests::integration(state, &pull.repository_id).await?;
    let login = forge.login.clone().unwrap_or_default();
    let session = state.store.get_session(&draft.session_id).await?;
    let from_review = session.seat() == Some(ariadne_core::Seat::Reviewer);
    let sent = match from_review {
        true => pulls::signed(&payload.body, false),
        false => payload.body.clone(),
    };
    pull_requests::post_reply(
        state,
        &pull,
        &comment,
        &payload.body,
        &sent,
        from_review,
        &login,
        &forge,
    )
    .await?;
    Ok(state
        .store
        .set_pull_request_draft_state(&draft.id, "published")
        .await?)
}
