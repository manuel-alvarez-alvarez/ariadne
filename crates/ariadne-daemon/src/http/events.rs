//! Agent-event endpoints: public listing plus the internal ingestion sink for
//! hooks/plugins.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;

use ariadne_api::Page;
use ariadne_api::events::{AgentEventDto, EventListQuery, IngestEventRequest};
use ariadne_store::{EventFilter, NewAgentEvent};

use super::AppState;
use super::classify::{
    attention_for_event, extract_internal_id, status_for_event, usage_for_event,
};
use super::convert::event_dto;
use super::error::ApiResult;

/// List agent events (poll with `after` for tailing).
#[utoipa::path(get, path = "/v1/events", tag = "events",
    params(EventListQuery, Page),
    responses((status = 200, body = [AgentEventDto])))]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<EventListQuery>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<AgentEventDto>>> {
    let events = state
        .store
        .list_events(EventFilter {
            session_id: q.session,
            task_id: q.task,
            limit: page.limit(),
            after: page.after,
        })
        .await?;
    Ok(Json(events.into_iter().map(event_dto).collect()))
}

/// Ingest an event reported by an agent hook (`ariadne agent-event`).
/// Not part of the public OpenAPI surface.
pub async fn ingest(
    State(state): State<AppState>,
    Json(req): Json<IngestEventRequest>,
) -> ApiResult<StatusCode> {
    // The session must exist; its task link is copied onto the event.
    let session = state.store.get_session(&req.session_id).await?;
    state
        .store
        .create_event(NewAgentEvent {
            session_id: Some(session.id.clone()),
            task_id: session.task_id.clone(),
            agent_kind: Some(req.agent_kind),
            kind: req.kind.clone(),
            payload: req.payload.clone(),
        })
        .await?;

    // Capture the agent-internal session id as soon as an event carries it.
    if session.internal_session_id.is_none()
        && let Some(internal) = extract_internal_id(req.agent_kind, &req.payload)
    {
        tracing::info!(session = %session.id, internal, "captured internal session id");
        state
            .store
            .set_session_internal_id(&session.id, &internal)
            .await?;
    }

    // What the agent has spent, where the event says so. Cumulative totals
    // per transcript, so this is a replace and not an addition — see
    // `Store::upsert_session_usage` — and it rides on any event kind of any
    // agent kind, since which of them carries the figures is a decision of
    // the hook or plugin that reads the transcript, not of this handler.
    if let Some((source, usage)) = usage_for_event(&req.payload) {
        state
            .store
            .upsert_session_usage(&session.id, &source, usage)
            .await?;
    }

    // Track liveness from lifecycle events (never resurrect ended sessions).
    let status = status_for_event(&req.kind);
    if session.status().is_live()
        && let Some(status) = status
        && status != session.status()
    {
        state.store.set_session_status(&session.id, status).await?;
    }

    // Attention follows the event too: an agent that reported an error needs
    // the user, and one that is working again does not. A running-mapped
    // event on a live session clears the agent's own flags — a `waiting_user`
    // is not one of them (`clear_agent_attention`), or the daemon telling the
    // user their pull request is theirs to merge would be undone by the next
    // tool call of the agent that happened to be running at the time.
    //
    // An idle-mapped one clears the two reasons it disproves and nothing more
    // (`clear_attention_after_idle`): a session that reported anything at all
    // is not the silent one `stalled` describes, and one whose turn ended on
    // idle rather than on another error has recovered from the failed turn
    // `agent_error` was raised for. The prompts stand — going idle is exactly
    // when a dialog or a question is waiting — and so, either way, does what
    // an ended session ended carrying: a stray event must not wipe the reason
    // it ended needing attention.
    //
    // Raising it asks one thing more: whether anybody is still waiting on
    // this agent. A reviewer's approval dialog after it has voted, or a
    // planner's after the goal left planning, is nobody's to answer — the
    // event is recorded and the status still follows it, only the flag is
    // withheld. Whether the session is still live enough to be asking is a
    // second condition, and one this handler deliberately does not test
    // itself: the status read above is a moment old by the time the raise
    // runs, so the store makes it part of the write — a prompt only ever
    // lands on a session that is still live at that instant.
    if let Some(reason) = attention_for_event(&req.kind, &req.payload) {
        if crate::attention::work_is_active(&state.store, &session).await {
            state
                .store
                .set_session_attention(&session.id, reason)
                .await?;
        }
    } else if session.status().is_live() {
        match status {
            Some(ariadne_core::SessionStatus::Running) => {
                state.store.clear_agent_attention(&session.id).await?;
            }
            Some(ariadne_core::SessionStatus::Idle) => {
                state.store.clear_attention_after_idle(&session.id).await?;
            }
            _ => {}
        }
    }

    state.store.touch_session(&session.id).await?;
    state.notify_scheduler_session(&session.id);
    Ok(StatusCode::ACCEPTED)
}
