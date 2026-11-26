//! Server-sent domain events: the live view of everything the daemon changes.

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
/// Not every event is a database write. `task_branch_updated` is published by
/// the daemon's watch on each live task's branch ref, so a commit an agent
/// makes in its worktree — which changes nothing in the store — still says
/// that the task's diff (`GET /v1/tasks/{id}/diff`) is no longer the one you
/// hold. It carries the branch and the full sha of its new head.
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
/// A source daemon names itself with `source`, its persistent daemon ID.
/// Its stream then also carries `source_post` frames (`SourcePostDto`): the
/// posts of worker agents to the orchestrator of that source (039). A post
/// goes on the newest open stream of the source alone, at once, and no stream
/// gets it again.
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
                       and the connection is closed. `task_branch_updated` (TaskBranchDto) \
                       comes from the daemon's watch on the task branch rather than from a \
                       store write: it says a commit landed and the task's diff has moved. \
                       A stream with `source` also carries `source_post` (SourcePostDto).",
        content_type = "text/event-stream", body = DomainEvent)))]
pub(super) async fn stream(
    State(state): State<AppState>,
    Query(q): Query<EventStreamQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (goal, task) = (q.goal, q.task);
    // Open before the stream returns, so a post sent once the source holds
    // this stream finds it.
    let posts = q
        .source
        .as_deref()
        .map(|source| state.source_posts.open(source));
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
    let client = crate::metrics::EventStreamClient::open();
    let ended = Arc::new(AtomicBool::new(false));
    let resync_sent = ended.clone();
    let events = sse::follow(
        state.events.subscribe(),
        move |event| {
            // Held by the stream, so the follower is counted until it goes.
            let _ = &client;
            // Filtered out: keep waiting rather than end the stream.
            event
                .matches(goal.as_deref(), task.as_deref())
                .then(|| sse::identified_event(event.event.kind(), event.event.payload()))
        },
        move |missed| {
            // `resync` is the last frame: no post follows it.
            resync_sent.store(true, Ordering::SeqCst);
            warn!(missed, "event stream subscriber lagged; signalling resync");
            crate::metrics::record_events_resync();
            Some(sse::identified_event("resync", ResyncDto { missed }))
        },
    );
    let posts = futures_util::stream::unfold(posts, |posts| async move {
        let (open, mut rx) = posts?;
        let post = rx.recv().await?;
        let event = sse::identified_event("source_post", post);
        Some((Ok(event), Some((open, rx))))
    });
    let merged = with_posts(events, posts, ended);
    sse::respond_alive(hello.chain(merged), beat)
}

/// The domain events and the posts as one stream. The domain events end it,
/// and so does the flag `ended` once it is set: a post that is ready when the
/// last event goes out never follows it.
fn with_posts<T>(
    events: impl Stream<Item = T>,
    posts: impl Stream<Item = T>,
    ended: Arc<AtomicBool>,
) -> impl Stream<Item = T> {
    let events = events.map(Some).chain(once(async { None }));
    let posts = posts
        .take_while(move |_| std::future::ready(!ended.load(Ordering::SeqCst)))
        .map(Some);
    futures_util::stream::select(events, posts)
        .take_while(|frame| std::future::ready(frame.is_some()))
        .filter_map(std::future::ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_post_that_is_ready_never_follows_the_resync() {
        let ended = Arc::new(AtomicBool::new(false));
        let flag = ended.clone();
        let events = futures_util::stream::iter(["resync"]).inspect(move |_| {
            flag.store(true, Ordering::SeqCst);
        });
        // Ready only once the resync is out, as after a lag with a queued post.
        let waiting = ended.clone();
        let posts = once(async move {
            while !waiting.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
            "source_post"
        });
        let frames: Vec<_> = with_posts(events, posts, ended).collect().await;
        assert_eq!(frames, ["resync"]);
    }
}
