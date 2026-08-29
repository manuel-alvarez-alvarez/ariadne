//! Server-sent domain events: the live view of everything the daemon changes.

use std::convert::Infallible;

use axum::extract::{Query, State};
use axum::response::sse::{Event, Sse};
use futures_util::stream::once;
use futures_util::{Stream, StreamExt};
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
/// Besides the domain events there is a `heartbeat` event (a `HeartbeatDto`:
/// the daemon's `version` and its `started_at`), sent as the connection opens
/// and every 15 s an idle connection goes without one. It is a named event
/// rather than the SSE comment other streams keep alive with, because a
/// browser's `EventSource` never surfaces a comment: a client watches it to
/// tell a live daemon from a dead one, and a changed `started_at` to tell a
/// restarted daemon from the one it was talking to.
///
/// A client that falls too far behind loses events. It is never left silently
/// stale: the daemon sends a final `resync` event (`{"missed": n}`) and closes
/// the connection, so an `EventSource` reconnects and takes the refetch path
/// above.
#[utoipa::path(get, path = "/v1/events/stream", tag = "events",
    params(EventStreamQuery),
    responses((status = 200,
        description = "SSE stream of domain events (text/event-stream). No replay on \
                       reconnect: refetch REST state first. A `heartbeat` event \
                       (HeartbeatDto) opens the connection and repeats every 15 idle \
                       seconds. A lagging client gets a final `resync` event (ResyncDto) \
                       and the connection is closed.",
        content_type = "text/event-stream", body = DomainEvent)))]
pub async fn stream(
    State(state): State<AppState>,
    Query(q): Query<EventStreamQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (goal, task) = (q.goal, q.task);
    // Not a `DomainEvent`: like `resync`, it describes the stream and the
    // daemon behind it, not a change in the daemon's state. No `id` either —
    // it is the same frame over and over, and one that never moves says
    // nothing an `EventSource` can use.
    let beat = sse::json_event("heartbeat", state.heartbeat());
    // Sent before the subscription is followed, so a client knows on open
    // which daemon it reached rather than 15 s later.
    let hello = once({
        let beat = beat.clone();
        async move { Ok(beat) }
    });
    let events = sse::follow(
        state.events.subscribe(),
        move |event| {
            // Filtered out: keep waiting rather than end the stream.
            event
                .matches(goal.as_deref(), task.as_deref())
                .then(|| sse::identified_event(event.event.kind(), event.event.payload()))
        },
        |missed| {
            warn!(missed, "event stream subscriber lagged; signalling resync");
            Some(sse::identified_event("resync", ResyncDto { missed }))
        },
    );
    sse::respond_alive(hello.chain(events), beat)
}
