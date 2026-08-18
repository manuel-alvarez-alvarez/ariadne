//! Daemon-log endpoints: what the process itself is saying, wherever its
//! stdout happens to be going.

use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use futures_util::stream::{once, unfold};
use serde::Deserialize;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use utoipa::IntoParams;

use ariadne_api::logs::{LogLineDto, LogSnapshotResponse};

use super::AppState;

/// How often a keep-alive comment is sent on a quiet stream.
const KEEP_ALIVE_SECS: u64 = 15;

#[derive(Debug, Default, Deserialize, IntoParams)]
pub struct LogsQuery {
    /// Return only the last N lines.
    pub tail: Option<usize>,
}

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
    let snapshot = once(async move {
        Ok(Event::default()
            .event("snapshot")
            .data(serde_json::json!(LogSnapshotResponse { lines }).to_string()))
    });
    let deltas = unfold(rx, |mut rx: Receiver<LogLineDto>| async move {
        match rx.recv().await {
            Ok(line) => Some((
                Ok(Event::default()
                    .event("delta")
                    .data(serde_json::json!(line).to_string())),
                rx,
            )),
            // Lagged: a slow consumer's missed lines are gone from the channel
            // for good. Hang up rather than stream a gap it cannot see; the
            // reconnect's snapshot is the resync. Closed: the daemon is
            // shutting down.
            Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => None,
        }
    });
    Sse::new(futures_util::StreamExt::chain(snapshot, deltas))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(KEEP_ALIVE_SECS)))
}
