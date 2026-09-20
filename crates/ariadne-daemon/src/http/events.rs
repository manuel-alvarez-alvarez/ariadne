//! Agent events: the public listing, and the ingestion every event the ACP
//! runtime reports takes into the store.

use axum::extract::{Query, State};

use ariadne_api::Page;
use ariadne_api::events::{
    AgentEventDto, EventListQuery, EventOrder as ApiEventOrder, IngestEventRequest,
};
use ariadne_store::{EventFilter, EventOrder, NewAgentEvent, Store, StoreError};

use super::AppState;
use super::classify::{
    attention_for_event, extract_internal_id, status_for_event, usage_for_event,
};
use super::convert::event_dto;
use super::error::{ApiResult, Json};

/// List agent events (poll with `after` for tailing, or read the newest with
/// `order=desc` and walk back with `before`).
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
            goal_id: q.goal,
            limit: page.limit(),
            after: page.after,
            before: q.before,
            order: match q.order.unwrap_or_default() {
                ApiEventOrder::Asc => EventOrder::Asc,
                ApiEventOrder::Desc => EventOrder::Desc,
            },
        })
        .await?;
    Ok(Json(events.into_iter().map(event_dto).collect()))
}

/// One agent event, applied to the store: recorded, and read for the status,
/// attention, internal id and usage it moves. The one path every report the
/// daemon's ACP runtime (`crate::acp`) makes takes — the caller wakes the
/// scheduler. Public so that a test can report as an agent would, down the
/// same path.
pub async fn ingest_event(store: &Store, req: &IngestEventRequest) -> Result<(), StoreError> {
    // The session must exist; its task link is copied onto the event.
    let session = store.get_session(&req.session_id).await?;

    // A report from a process the session has moved past changes nothing.
    //
    // A relaunch puts a new agent under the same row, and the agent it
    // replaced still has its exit to report — a moment later, under the same
    // session and, on a resumed conversation, the same internal id. Read as
    // the live agent's, that report retires a session whose agent is up and
    // working: the goal then wants an orchestrator it already has, and the
    // row is left `exited` while its agent goes on writing to it.
    //
    // The launch each of them carries is what tells them apart. Only a
    // mismatch is refused: a report that names no launch, and a row that has
    // not been launched under this daemon, have none to compare — neither is
    // a dead process talking, and both are believed.
    let superseded = matches!(
        (&req.launch, &session.launch_id),
        (Some(reported), Some(current)) if reported != current
    );

    // The event itself is kept, what it says about the agent simply no longer
    // news — all but its end. A `session_end` in a session's events is what
    // every console closes on, and the session it would announce has not
    // ended: its next agent is already running. Stored, it would close a
    // console open over the relaunch, and every console opened after it.
    if superseded && req.kind == "session_end" {
        tracing::debug!(
            session = %session.id, launch = ?req.launch,
            "dropping the end of a launch this session has moved past"
        );
        return Ok(());
    }

    store
        .create_event(NewAgentEvent {
            session_id: Some(session.id.clone()),
            task_id: session.task_id.clone(),
            kind: req.kind.clone(),
            payload: req.payload.clone(),
        })
        .await?;

    // What the agent has spent, where the event says so. Cumulative totals
    // per source, so this is a replace and not an addition — see
    // `Store::upsert_session_usage` — and it rides on any event kind. It is
    // news even from a launch the session has moved past: the turn a
    // relaunch cancelled reports what it spent only after the relaunch, and
    // its source is its own launch, which nothing later replaces.
    if let Some((source, usage)) = usage_for_event(&req.payload) {
        store
            .upsert_session_usage(&session.id, &source, usage)
            .await?;
    }

    if superseded {
        tracing::debug!(
            session = %session.id, kind = %req.kind, launch = ?req.launch,
            "ignoring an event from a launch this session has moved past"
        );
        return Ok(());
    }

    // Capture the agent-internal session id as soon as an event carries it.
    if session.internal_session_id.is_none()
        && let Some(internal) = extract_internal_id(&req.payload)
    {
        tracing::info!(session = %session.id, internal, "captured internal session id");
        store
            .set_session_internal_id(&session.id, &internal)
            .await?;
    }

    // Track liveness from lifecycle events (never resurrect ended sessions).
    // Read here, written last: see the end of this function.
    let status = status_for_event(&req.kind);

    // Attention follows the event too: an agent that reported an error or
    // asked for a permission needs the user, and one that is working again
    // does not. A running-mapped event on a live session clears the agent's
    // own flags — a `waiting_user` is not one of them
    // (`clear_agent_attention`), or the daemon telling the user their pull
    // request is theirs to merge would be undone by the next tool call of the
    // agent that happened to be running at the time.
    //
    // An idle-mapped one clears the two reasons it disproves and nothing more
    // (`clear_attention_after_idle`): a session that reported anything at all
    // is not the silent one `stalled` describes, and one whose turn ended on
    // idle rather than on another error has recovered from the failed turn
    // `agent_error` was raised for. A standing prompt stays — going idle is
    // exactly when a permission is waiting — and so, either way, does what an
    // ended session ended carrying: a stray event must not wipe the reason it
    // ended needing attention.
    //
    // Raising it asks one thing more: whether anybody is still waiting on
    // this agent. A reviewer's approval request after it has voted, or an
    // orchestrator's after the goal left planning, is nobody's to answer —
    // the event is recorded and the status still follows it, only the flag is
    // withheld. Whether the session is still live enough to be asking is a
    // second condition, and one this handler deliberately does not test
    // itself: the status read above is a moment old by the time the raise
    // runs, so the store makes it part of the write — a prompt only ever
    // lands on a session that is still live at that instant.
    if let Some(reason) = attention_for_event(&req.kind) {
        if crate::attention::work_is_active(store, &session).await {
            store.set_session_attention(&session.id, reason).await?;
        }
    } else if session.status().is_live() {
        match status {
            Some(ariadne_core::SessionStatus::Running) => {
                store.clear_agent_attention(&session.id).await?;
            }
            Some(ariadne_core::SessionStatus::Idle) => {
                store.clear_attention_after_idle(&session.id).await?;
            }
            _ => {}
        }
    }

    // The end of an agent's process is no word from the agent: the runtime
    // reports its going, and a launch that went before it ever spoke is one
    // that died on arrival (009) — which stamping its activity here would
    // hide, and a goal would start that agent again every tick.
    if !matches!(req.kind.as_str(), "session_end" | "session.error") {
        store.touch_session(&session.id).await?;
    }

    // The status goes last. Everything else the event moves — the agent's
    // id, the flag, the clock — is in place before the row says what the
    // agent is doing, so a row a concurrent reader sees at some new status
    // already carries everything that status implies: idle already carries
    // the clock the turn that just ended stamped, and the watchdog measures
    // the silence from there rather than from the report before it.
    //
    // Guarded against the row this write touches, not the one read at the
    // top of this function: a kill, or a relaunch, landing while this event
    // is still being ingested must not have its retirement — or its new
    // launch's `starting` — undone by a status this event decided before
    // either. The superseded check above is itself a moment old by the time
    // this write runs, so it rides along too, on the same terms.
    if let Some(status) = status {
        store
            .set_session_status_if_live(&session.id, status, req.launch.as_deref())
            .await?;
    }
    Ok(())
}
