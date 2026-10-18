//! Daemon-log endpoints: what the process itself is saying, read off the
//! telemetry store (033) rather than an in-memory ring.

use std::convert::Infallible;
use std::future::Future;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::sse::{Event, Sse};
use futures_util::stream::once;
use futures_util::{Stream, StreamExt};

use ariadne_api::logs::{LogLineDto, LogSnapshotResponse, LogsQuery};
use ariadne_store::telemetry::LogFilter;

use super::AppState;
use super::error::{ApiError, ApiResult};
use super::sse;

fn to_dto(record: ariadne_store::telemetry::LogRecord) -> LogLineDto {
    LogLineDto {
        message: ariadne_api::logs::render_message(record.body.as_deref(), &record.attributes),
        ts: record.ts,
        level: record.severity_text.unwrap_or_default(),
        target: record.target.unwrap_or_default(),
        service: record.service.unwrap_or_default(),
    }
}

fn filter_of(q: &LogsQuery) -> LogFilter {
    LogFilter {
        tail: q.tail,
        service: q.service.clone(),
        min_level: q.level.clone(),
        since: q.since.clone(),
        before_id: None,
    }
}

/// Subscribe before starting the store read. A log exporter that commits and
/// broadcasts while the read is in flight is therefore either in the
/// snapshot, or waiting in this receiver for the delta half to compare with
/// the snapshot's persisted ID.
async fn snapshot_after_subscribe<T, E>(
    stream: &crate::telemetry::LogStream,
    read: impl Future<Output = Result<T, E>>,
) -> Result<
    (
        T,
        tokio::sync::broadcast::Receiver<crate::telemetry::StreamedLog>,
    ),
    E,
> {
    let receiver = stream.subscribe();
    Ok((read.await?, receiver))
}

fn after_snapshot(snapshot_id: Option<&str>, record: &crate::telemetry::StreamedLog) -> bool {
    snapshot_id.is_none_or(|id| record.id.as_str() > id)
}

/// The opening snapshot comes from SQLite, but deltas have just been written
/// and arrive from the exporter's broadcast. Apply the same public filters to
/// both halves of a stream.
fn line_matches(q: &LogsQuery, line: &LogLineDto) -> bool {
    if q.service
        .as_deref()
        .is_some_and(|service| service != line.service)
    {
        return false;
    }
    if q.since.as_deref().is_some_and(|since| {
        ariadne_store::telemetry::normalize_timestamp(since).is_ok_and(|since| line.ts < since)
    }) {
        return false;
    }
    q.level.as_deref().is_none_or(|level| {
        let Some(floor) = ariadne_store::telemetry::severity_number_for_level(level) else {
            return false;
        };
        ariadne_store::telemetry::severity_number_for_level(&line.level)
            .is_some_and(|actual| actual >= floor)
    })
}

/// Recent daemon log lines, read off the telemetry store, oldest first.
#[utoipa::path(get, path = "/v1/logs", tag = "logs",
    params(LogsQuery),
    responses((status = 200, body = LogSnapshotResponse)))]
pub(super) async fn snapshot(
    State(state): State<AppState>,
    Query(q): Query<LogsQuery>,
) -> ApiResult<Json<LogSnapshotResponse>> {
    let tail = q.tail.unwrap_or(200);
    let records = state
        .telemetry_store
        .read_logs(LogFilter {
            tail: Some(tail),
            ..filter_of(&q)
        })
        .await
        .map_err(ApiError::from)?;
    Ok(Json(LogSnapshotResponse {
        lines: records.into_iter().map(to_dto).collect(),
    }))
}

/// Follow the daemon log.
///
/// The stream opens with a `snapshot` event carrying the current tail of
/// the telemetry store (a `LogSnapshotResponse`, what `GET /v1/logs` would
/// have returned), then sends a `delta` event per new line (a
/// `LogLineDto`), fed by the telemetry log exporter as it writes. Payloads
/// are compact JSON, so log content cannot break SSE framing. There is no
/// replay on reconnect: every connection starts over from a fresh snapshot,
/// which is also the resync path for a follower that fell behind and was
/// dropped.
#[utoipa::path(get, path = "/v1/logs/stream", tag = "logs",
    params(LogsQuery),
    responses((status = 200,
        description = "SSE stream of daemon log lines (text/event-stream). A `snapshot` \
                       event with the current tail (LogSnapshotResponse), then a `delta` \
                       event per new line (LogLineDto). A follower that falls too far \
                       behind is disconnected; reconnecting starts over from a fresh \
                       snapshot.",
        content_type = "text/event-stream", body = LogSnapshotResponse)))]
pub(super) async fn stream(
    State(state): State<AppState>,
    Query(q): Query<LogsQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let tail = q.tail.unwrap_or(200);
    // Subscribe first. The persisted ID at the end of the subsequent
    // snapshot suppresses already-present broadcasts, while later IDs become
    // deltas. Thus a writer in the handoff cannot disappear between reads.
    let (records, rx) = snapshot_after_subscribe(
        &state.log_stream,
        state.telemetry_store.read_logs(LogFilter {
            tail: Some(tail),
            ..filter_of(&q)
        }),
    )
    .await
    .map_err(ApiError::from)?;
    let snapshot_id = records.last().map(|record| record.id.clone());
    let lines: Vec<LogLineDto> = records.into_iter().map(to_dto).collect();
    let delta_filter = q.clone();
    let snapshot =
        once(async move { Ok(sse::json_event("snapshot", LogSnapshotResponse { lines })) });
    let deltas = sse::follow(
        rx,
        move |record| {
            (after_snapshot(snapshot_id.as_deref(), &record)
                && line_matches(&delta_filter, &record.line))
            .then(|| sse::json_event("delta", record.line))
        },
        // Nothing to say to a follower that fell behind: the reconnect's
        // snapshot is the resync.
        |_| None,
    );
    Ok(sse::respond(snapshot.chain(deltas)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::telemetry::StreamedLog;
    use ariadne_store::telemetry::{NewLogRecord, TelemetryStore};

    #[tokio::test]
    async fn an_export_during_stream_initialization_is_in_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = TelemetryStore::open(dir.path().join("telemetry.db"))
            .await
            .unwrap();
        let stream = crate::telemetry::LogStream::new();
        let publisher = stream.clone();
        let write = async {
            let written = store
                .write_logs(&[NewLogRecord {
                    ts: "2026-01-01T10:00:00Z".to_string(),
                    observed_ts: "2026-01-01T10:00:00Z".to_string(),
                    severity_number: Some(9),
                    severity_text: Some("INFO".to_string()),
                    body: Some("during stream setup".to_string()),
                    service: Some("ariadned".to_string()),
                    scope: None,
                    target: Some("test".to_string()),
                    attributes: json!({}),
                    trace_id: None,
                    span_id: None,
                }])
                .await
                .unwrap();
            let record = written.into_iter().next().unwrap();
            publisher.feed(StreamedLog {
                id: record.id.clone(),
                line: to_dto(record),
            });
            store.read_logs(LogFilter::default()).await
        };

        let (snapshot, mut receiver) = snapshot_after_subscribe(&stream, write).await.unwrap();
        let snapshot_id = snapshot.last().map(|record| record.id.as_str());
        assert_eq!(snapshot.len(), 1, "the committed export is in the snapshot");

        let broadcast = receiver.recv().await.unwrap();
        assert!(
            !after_snapshot(snapshot_id, &broadcast),
            "the boundary removes only the duplicate broadcast, not the record"
        );
    }

    #[test]
    fn a_live_since_filter_normalizes_fractional_precision_and_offsets() {
        let query = LogsQuery {
            since: Some("2026-01-01T10:00:00+00:00".to_string()),
            ..Default::default()
        };
        let line = LogLineDto {
            ts: "2026-01-01T10:00:00.001Z".to_string(),
            level: "INFO".to_string(),
            target: "test".to_string(),
            message: "after the boundary".to_string(),
            service: "ariadned".to_string(),
        };

        assert!(line_matches(&query, &line));
    }
}
