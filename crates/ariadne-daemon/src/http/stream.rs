//! Server-sent domain events: the live view of everything the daemon changes.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_core::Stream;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tracing::warn;

use ariadne_api::stream::{DomainEvent, EventStreamQuery};
use ariadne_core::id::new_id;

use super::AppState;
use crate::bus::BusEvent;

/// How often a keep-alive comment is sent on an idle stream.
const KEEP_ALIVE_SECS: u64 = 15;

/// Subscribe to the live domain-event stream.
///
/// Every state change in the daemon — from HTTP calls, from the scheduler and
/// from agent activity alike — is published here. Each message carries a fresh
/// ULID `id`, the event kind as its `event` name, and the full updated DTO as
/// `data`, so clients patch their state without refetching.
///
/// There is **no replay or backfill**: the `id` is informational and
/// `Last-Event-ID` is ignored. On (re)connect, refetch the REST state you care
/// about and then follow the stream. The same applies when a client falls too
/// far behind: its buffered events are dropped and it must resync.
#[utoipa::path(get, path = "/v1/events/stream", tag = "events",
    params(EventStreamQuery),
    responses((status = 200,
        description = "SSE stream of domain events (text/event-stream); no replay on reconnect",
        content_type = "text/event-stream", body = DomainEvent)))]
pub async fn stream(
    State(state): State<AppState>,
    Query(q): Query<EventStreamQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events =
        BroadcastStream::new(state.events.subscribe()).filter_map(move |item| match item {
            Ok(event) if event.matches(q.goal.as_deref(), q.task.as_deref()) => {
                Some(Ok(sse_event(&event)))
            }
            Ok(_) => None,
            // Slow consumer: drop what it missed rather than stall the bus. The
            // client notices the gap only by resyncing over REST.
            Err(BroadcastStreamRecvError::Lagged(missed)) => {
                warn!(
                    missed,
                    "event stream subscriber lagged; it must resync over REST"
                );
                None
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
