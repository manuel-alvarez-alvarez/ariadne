//! Daemon-log endpoints: what the process itself is saying, wherever its
//! stdout happens to be going.

use std::convert::Infallible;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::sse::{Event, Sse};
use futures_util::stream::once;
use futures_util::{Stream, StreamExt};

use ariadne_api::logs::{LogLineDto, LogSnapshotResponse, LogsQuery};

use super::AppState;
use super::sse;

/// Recent daemon log lines from the in-memory ring buffer, oldest first.
#[utoipa::path(get, path = "/v1/logs", tag = "logs",
    params(LogsQuery),
    responses((status = 200, body = LogSnapshotResponse)))]
pub async fn snapshot(
    State(state): State<AppState>,
    Query(q): Query<LogsQuery>,
) -> Json<LogSnapshotResponse> {
    let mut lines = state.logs.snapshot();
    if let Some(tail) = q.tail
        && lines.len() > tail
    {
        lines.drain(..lines.len() - tail);
    }
    Json(LogSnapshotResponse { lines })
}

/// Follow the daemon log.
///
/// The stream opens with a `snapshot` event carrying the current ring buffer
/// (a `LogSnapshotResponse`, what `GET /v1/logs` would have returned), then
/// sends a `delta` event per new line (a `LogLineDto`). Payloads are compact
/// JSON, so log content cannot break SSE framing. There is no replay on
/// reconnect: every connection starts over from a fresh snapshot, which is
/// also the resync path for a follower that fell behind and was dropped.
#[utoipa::path(get, path = "/v1/logs/stream", tag = "logs",
    responses((status = 200,
        description = "SSE stream of daemon log lines (text/event-stream). A `snapshot` \
                       event with the current buffer (LogSnapshotResponse), then a `delta` \
                       event per new line (LogLineDto). A follower that falls too far \
                       behind is disconnected; reconnecting starts over from a fresh \
                       snapshot.",
        content_type = "text/event-stream", body = LogSnapshotResponse)))]
pub async fn stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (lines, rx) = state.logs.snapshot_and_follow();
    let snapshot =
        once(async move { Ok(sse::json_event("snapshot", LogSnapshotResponse { lines })) });
    let deltas = sse::follow(
        rx,
        |line: LogLineDto| Some(sse::json_event("delta", line)),
        // Nothing to say to a follower that fell behind: the reconnect's
        // snapshot is the resync.
        |_| None,
    );
    sse::respond(snapshot.chain(deltas))
}
