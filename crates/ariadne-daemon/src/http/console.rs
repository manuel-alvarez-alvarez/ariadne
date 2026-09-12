//! Ariadne's own console for an agent session.
//!
//! There is nothing to capture here that is not already an agent event: the
//! ACP runtime (`crate::acp`) reports what its agent does on the one
//! ingestion path, so the console is a session-scoped view of
//! that record — every event kind the runtime reports flows through
//! unchanged, so a thought, a message, a tool call, a plan, a permission
//! request or a turn's status all show up exactly as the runtime names them,
//! with no fixed list to fall behind it.
//!
//! The one thing the record does not hold is the turn in flight. The runtime
//! streams that live — a chunk of message or thought text, a tool call's
//! progress — to the console alone: never stored, never on the domain bus.
//! The console stream carries both, and a snapshot taken mid-turn ends on
//! the text so far.

use std::convert::Infallible;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use futures_util::future::ready;
use futures_util::stream::{once, select};
use futures_util::{Stream, StreamExt};

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::ConsoleInputRequest;
use ariadne_api::stream::{DomainEvent, ResyncDto};

use super::AppState;
use super::convert::event_dto;
use super::error::{ApiError, ApiResult, Json};
use super::sse;

/// The session's events so far, in order: the whole transcript a console
/// opens on. While a turn runs, one `agent_thought_chunk` and one
/// `agent_message_chunk` holding the text so far follow the stored events.
#[utoipa::path(get, path = "/v1/sessions/{id}/console", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = [AgentEventDto]), (status = 404)))]
pub async fn snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<AgentEventDto>>> {
    state.store.get_session(&id).await?;
    let events = state.store.list_session_events(&id).await?;
    let mut snapshot: Vec<AgentEventDto> = events.into_iter().map(event_dto).collect();
    snapshot.extend(state.launcher.acp.turn_so_far(&id).await);
    Ok(Json(snapshot))
}

/// Follow a session's console.
///
/// Opens with a `snapshot` event carrying what `GET /console` would return —
/// every event recorded so far, oldest first, then the running turn's text so
/// far — then an `event` per later one, each an `AgentEventDto`. Subscribing
/// happens before the snapshot is read and every later stored event is
/// compared against the snapshot's last id, so nothing committed in between
/// is ever missed or delivered twice; the live events are read under the
/// runtime's own turn lock for the same guarantee.
///
/// There is no replay and no `Last-Event-ID`: reconnecting starts again from a
/// fresh snapshot. A client that falls too far behind gets a final `resync`
/// event and the connection closes, exactly as `/v1/events/stream` does.
#[utoipa::path(get, path = "/v1/sessions/{id}/console/stream", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200,
        description = "SSE stream of console events (text/event-stream). A `snapshot` event \
                       carrying every event recorded so far (`[AgentEventDto]`) and the \
                       running turn's text so far, then an `event` per new one \
                       (`AgentEventDto`) — message and thought chunks, tool call progress, \
                       tool calls, plans, permission requests and turn status all arrive \
                       this way, in the vocabulary the ACP runtime reports them in. The \
                       chunks and the progress are live only: they are never stored and \
                       never reach `/v1/events`. A client that falls behind gets a `resync` \
                       event (ResyncDto) and the connection closes.",
        content_type = "text/event-stream", body = AgentEventDto),
        (status = 404)))]
pub async fn stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    state.store.get_session(&id).await?;
    // Subscribed before the snapshot is queried: an event committed in
    // between lands in both places, which the id watermark below resolves in
    // the snapshot's favour rather than sending it twice.
    let rx = state.events.subscribe();
    let recorded = state.store.list_session_events(&id).await?;
    let last_id = recorded.last().map(|e| e.id.clone());
    let mut snapshot: Vec<AgentEventDto> = recorded.into_iter().map(event_dto).collect();
    let (live_rx, so_far) = state.launcher.acp.subscribe_console(&id).await;
    snapshot.extend(so_far);
    let opening = once(async move { Ok(sse::json_event("snapshot", snapshot)) });

    let session_id = id.clone();
    let stored = sse::follow(
        rx,
        move |bus_event| match bus_event.event {
            DomainEvent::AgentEvent(dto)
                if dto.session_id.as_deref() == Some(session_id.as_str())
                    && last_id.as_deref().is_none_or(|last| dto.id.as_str() > last) =>
            {
                Some(sse::json_event("event", dto))
            }
            _ => None,
        },
        |missed| Some(sse::identified_event("resync", ResyncDto { missed })),
    );
    let session_id = id;
    let live = sse::follow(
        live_rx,
        move |dto: AgentEventDto| {
            (dto.session_id.as_deref() == Some(session_id.as_str()))
                .then(|| sse::json_event("event", dto))
        },
        |missed| Some(sse::identified_event("resync", ResyncDto { missed })),
    );
    // Either half ending ends the connection: a `resync` on one of them is
    // the client's cue to start over from a fresh snapshot, and the other
    // half must not keep it on a transcript with holes in it.
    let both = select(
        stored.map(Some).chain(once(async { None })),
        live.map(Some).chain(once(async { None })),
    )
    .take_while(|event| ready(event.is_some()))
    .filter_map(ready);
    Ok(sse::respond(opening.chain(both)))
}

/// Type into a session.
///
/// While a permission request is pending, the text selects that request's
/// option; otherwise it becomes a fresh `session/prompt` — sent at once if
/// the agent is between turns, or queued, in order, behind whichever one is
/// running and sent the moment it ends.
///
/// Both halves of "live" matter: the row's status, because a finished
/// session takes no more input, and the runtime itself, which has no agent to
/// hand a prompt to for a session whose process is gone.
///
/// And it is the user acting on the session, so whatever it was flagged for
/// comes down with the input: a permission answered, a question typed back,
/// a message read. An agent still blocked raises its own again with its next
/// event. The scheduler hears about it as it does about an ingested event, so
/// the quiet clock and the stream follow.
#[utoipa::path(post, path = "/v1/sessions/{id}/console/input", tag = "sessions",
    operation_id = "console_input",
    request_body = ConsoleInputRequest,
    params(("id" = String, Path, description = "session id")),
    responses((status = 204, description = "Permission answer or prompt accepted"),
        (status = 404), (status = 409)))]
pub async fn input(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ConsoleInputRequest>,
) -> ApiResult<StatusCode> {
    let session = state.store.get_session(&id).await?;
    if !session.status().is_live() {
        return Err(ApiError::conflict(format!(
            "session {id} is {} and cannot take input",
            session.status
        )));
    }
    state
        .launcher
        .acp
        .send_input(&id, req.text)
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    state.store.clear_session_attention(&id).await?;
    state.notify_scheduler_session(&id);
    Ok(StatusCode::NO_CONTENT)
}

/// Cancel the turn a session is running.
///
/// Sends ACP `session/cancel` to the agent while its `session/prompt` is
/// still in flight. The turn then ends as any other does — the text so far
/// stored, then a `stop` whose `stop_reason` is `cancelled`. A session that
/// is not live, one whose agent process is gone, and one between turns all
/// have nothing to cancel, and say so with `409`.
#[utoipa::path(post, path = "/v1/sessions/{id}/console/cancel", tag = "sessions",
    operation_id = "console_cancel",
    params(("id" = String, Path, description = "session id")),
    responses((status = 204, description = "The running turn was told to cancel"),
        (status = 404), (status = 409)))]
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let session = state.store.get_session(&id).await?;
    if !session.status().is_live() {
        return Err(ApiError::conflict(format!(
            "session {id} is {} and has no turn to cancel",
            session.status
        )));
    }
    state
        .launcher
        .acp
        .cancel(&id)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}
