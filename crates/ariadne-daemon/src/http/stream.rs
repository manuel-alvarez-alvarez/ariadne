//! Server-sent domain events: the live view of everything the daemon changes.

use std::convert::Infallible;

use axum::extract::{Query, State};
use axum::response::sse::{Event, Sse};
use futures_util::Stream;
use tracing::warn;

use super::AppState;
use super::sse;
use ariadne_api::stream::{DomainEvent, EventStreamQuery, ResyncDto};

/// Subscribe to the live domain-event stream.
///
/// Every state change in the daemon — from HTTP calls, from the scheduler and
/// from agent activity alike — is published here. Each message carries a fresh
/// ULID `id`, the event kind as its `event` name, and the full updated DTO as
/// `data`, so clients patch their state without refetching.
///
/// There is **no replay or backfill**: the `id` is informational and
/// `Last-Event-ID` is ignored. On (re)connect, refetch the REST state you care
/// about and then follow the stream.
///
/// A client that falls too far behind loses events. It is never left silently
/// stale: the daemon sends a final `resync` event (`{"missed": n}`) and closes
/// the connection, so an `EventSource` reconnects and takes the refetch path
/// above.
#[utoipa::path(get, path = "/v1/events/stream", tag = "events",
    params(EventStreamQuery),
    responses((status = 200,
        description = "SSE stream of domain events (text/event-stream). No replay on \
                       reconnect: refetch REST state first. A lagging client gets a final \
                       `resync` event (ResyncDto) and the connection is closed.",
        content_type = "text/event-stream", body = DomainEvent)))]
pub async fn stream(
    State(state): State<AppState>,
    Query(q): Query<EventStreamQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (goal, task) = (q.goal, q.task);
    sse::respond(sse::follow(
        state.events.subscribe(),
        move |event| {
            // Filtered out: keep waiting rather than end the stream.
            event
                .matches(goal.as_deref(), task.as_deref())
                .then(|| sse::identified_event(event.event.kind(), event.event.payload()))
        },
        |missed| {
            warn!(missed, "event stream subscriber lagged; signalling resync");
            // Not a `DomainEvent`: it describes the stream itself, not a
            // change in the daemon.
            Some(sse::identified_event("resync", ResyncDto { missed }))
        },
    ))
}
