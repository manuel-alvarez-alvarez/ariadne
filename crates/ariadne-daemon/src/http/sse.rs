//! What the three server-sent streams have in common: compact JSON payloads,
//! so newlines and control bytes cannot break SSE's line framing; one
//! keep-alive interval; and, for two of the three, a broadcast subscription
//! turned into a stream of frames.
//!
//! None of them replays. A subscriber that falls too far behind is told to
//! start over — over REST for the domain stream, from a fresh snapshot for
//! the log ones — rather than left quietly stale on state that has moved.

use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use futures_util::stream::unfold;
use serde::Serialize;

use ariadne_core::id::new_id;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;

/// How often a keep-alive comment is sent on an idle stream.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Wrap a stream of events as the response every SSE endpoint here returns.
pub fn respond<S>(events: S) -> Sse<impl Stream<Item = Result<Event, Infallible>>>
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    Sse::new(events).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}

/// One named event carrying `payload` as compact JSON.
pub fn json_event(name: &str, payload: impl Serialize) -> Event {
    Event::default().event(name).data(encode(payload))
}

/// The same with a fresh ULID `id` in front of it — the field order the
/// domain stream has always put on the wire, and what its clients parse.
pub fn identified_event(name: &str, payload: impl Serialize) -> Event {
    Event::default()
        .id(new_id())
        .event(name)
        .data(encode(payload))
}

fn encode(payload: impl Serialize) -> String {
    serde_json::json!(payload).to_string()
}

/// One connection's view of a broadcast channel.
struct Follower<T, F, L> {
    rx: Receiver<T>,
    frame: F,
    lagged: L,
    /// Set once the lag signal is out: the next poll ends the stream.
    closing: bool,
}

/// Follow a broadcast channel as a stream of SSE frames.
///
/// `frame` turns one message into what goes out, or `None` to keep waiting —
/// which is how a filtered stream skips what it does not want without ending.
/// `lagged` says what a subscriber that fell behind is sent before the
/// connection closes; the missed messages are gone from the channel for good,
/// so hanging up is the honest answer. A closed channel is the daemon
/// shutting down, and ends the stream with nothing.
pub fn follow<T, F, L>(
    rx: Receiver<T>,
    frame: F,
    lagged: L,
) -> impl Stream<Item = Result<Event, Infallible>>
where
    T: Clone,
    F: FnMut(T) -> Option<Event>,
    L: FnMut(u64) -> Option<Event>,
{
    unfold(
        Follower {
            rx,
            frame,
            lagged,
            closing: false,
        },
        |mut f| async move {
            if f.closing {
                return None;
            }
            loop {
                match f.rx.recv().await {
                    Ok(message) => {
                        if let Some(event) = (f.frame)(message) {
                            return Some((Ok(event), f));
                        }
                    }
                    Err(RecvError::Lagged(missed)) => {
                        let signal = (f.lagged)(missed);
                        f.closing = true;
                        return signal.map(|event| (Ok(event), f));
                    }
                    Err(RecvError::Closed) => return None,
                }
            }
        },
    )
}
