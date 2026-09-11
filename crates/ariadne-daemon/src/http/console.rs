//! Ariadne's own console for an agent session.
//!
//! There is nothing to capture here that is not already an agent event: the
//! ACP runtime (`crate::acp`) reports what its agent does on the one
//! ingestion path, so the console is a session-scoped view of
//! that record — every event kind the runtime reports flows through
//! unchanged, so a message chunk, a thought, a tool call, a plan, a
//! permission request or a turn's status all show up exactly as the runtime
//! names them, with no fixed list to fall behind it.

use std::convert::Infallible;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use futures_util::stream::once;
use futures_util::{Stream, StreamExt};

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::ConsoleInputRequest;
use ariadne_api::stream::{DomainEvent, ResyncDto};

use super::AppState;
use super::convert::event_dto;
use super::error::{ApiError, ApiResult, Json};
use super::sse;

/// The session's events so far, in order: the whole transcript a console
/// opens on.
#[utoipa::path(get, path = "/v1/sessions/{id}/console", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200, body = [AgentEventDto]), (status = 404)))]
pub async fn snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<AgentEventDto>>> {
    state.store.get_session(&id).await?;
    let events = state.store.list_session_events(&id).await?;
    Ok(Json(events.into_iter().map(event_dto).collect()))
}

/// Follow a session's console.
///
/// Opens with a `snapshot` event carrying what `GET /console` would return —
/// every event recorded so far, oldest first — then an `event` per later one,
/// each an `AgentEventDto`. Subscribing happens before the snapshot is read
/// and every later event is compared against the snapshot's last id, so
/// nothing committed in between is ever missed or delivered twice.
///
/// There is no replay and no `Last-Event-ID`: reconnecting starts again from a
/// fresh snapshot. A client that falls too far behind gets a final `resync`
/// event and the connection closes, exactly as `/v1/events/stream` does.
#[utoipa::path(get, path = "/v1/sessions/{id}/console/stream", tag = "sessions",
    params(("id" = String, Path, description = "session id")),
    responses((status = 200,
        description = "SSE stream of console events (text/event-stream). A `snapshot` event \
                       carrying every event recorded so far (`[AgentEventDto]`), then an \
                       `event` per new one (`AgentEventDto`) — message chunks, thoughts, tool \
                       calls, plans, permission requests and turn status all arrive this way, \
                       in the vocabulary the ACP runtime reports them in. A client \
                       that falls behind gets a `resync` event (ResyncDto) and the connection \
                       closes.",
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
    let snapshot: Vec<AgentEventDto> = recorded.into_iter().map(event_dto).collect();
    let opening = once(async move { Ok(sse::json_event("snapshot", snapshot)) });

    let session_id = id.clone();
    let live = sse::follow(
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
    Ok(sse::respond(opening.chain(live)))
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
