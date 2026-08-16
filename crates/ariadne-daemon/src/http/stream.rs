//! Server-sent domain events: the live view of everything the daemon changes.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use futures_util::stream::unfold;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tracing::warn;

use ariadne_api::stream::{DomainEvent, EventStreamQuery, ResyncDto};
use ariadne_core::id::new_id;

use super::AppState;
use crate::bus::BusEvent;

/// How often a keep-alive comment is sent on an idle stream.
const KEEP_ALIVE_SECS: u64 = 15;

/// One connection's view of the bus.
struct Subscriber {
    rx: Receiver<BusEvent>,
    goal: Option<String>,
    task: Option<String>,
    /// Set once the resync signal is out: the next poll ends the stream.
    closing: bool,
}

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
    let subscriber = Subscriber {
        rx: state.events.subscribe(),
        goal: q.goal,
        task: q.task,
        closing: false,
    };
    let events = unfold(subscriber, |mut s| async move {
        if s.closing {
            return None;
        }
        loop {
            match s.rx.recv().await {
                Ok(event) if event.matches(s.goal.as_deref(), s.task.as_deref()) => {
                    return Some((Ok(sse_event(&event)), s));
                }
                // Filtered out: keep waiting rather than end the stream.
                Ok(_) => continue,
                // Slow consumer: what it missed is gone for good. Say so and
                // hang up — a stream that looks healthy while the client's
                // state silently rots is the worse failure.
                Err(RecvError::Lagged(missed)) => {
                    warn!(missed, "event stream subscriber lagged; signalling resync");
                    s.closing = true;
                    return Some((Ok(resync_event(missed)), s));
                }
                // The bus is gone: the daemon is shutting down.
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(KEEP_ALIVE_SECS)))
}

fn sse_event(event: &BusEvent) -> Event {
    Event::default()
        .id(new_id())
        .event(event.event.kind())
        // Compact JSON: never contains the newlines SSE framing cares about.
        .data(event.event.payload().to_string())
}

/// The control event closing a lagged connection. Not a [`DomainEvent`]: it
/// describes the stream itself, not a change in the daemon.
fn resync_event(missed: u64) -> Event {
    Event::default()
        .id(new_id())
        .event("resync")
        .data(serde_json::json!(ResyncDto { missed }).to_string())
}
