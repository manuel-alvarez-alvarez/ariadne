//! Integration tests for `GET /v1/logs` and `GET /v1/logs/stream`, now read
//! off the telemetry store (033) rather than an in-memory ring.
//!
//! Events are emitted through a thread-default subscriber carrying the
//! daemon's real OpenTelemetry bridge — exactly how the daemon wires it,
//! minus the global install a test process cannot afford — and observed
//! through the HTTP API. The bridge batches, so every assertion flushes
//! the pipeline first: deterministic, unlike a sleep, and cheaper than
//! polling the schedule delay out with `eventually`.

use crate::common;

use axum::body::Body;
use axum::http::{StatusCode, header};
use tracing_subscriber::layer::SubscriberExt;

use ariadne_api::logs::{LogLineDto, LogSnapshotResponse};
use ariadne_core::flaky_test;

use common::{Harness, Sse, TIMEOUT, eventually, get, harness, next_sse, parse_sse};

/// The daemon's real bridge layer, scoped to this thread: while the guard
/// lives, `tracing` macros feed the telemetry pipeline behind the API.
fn capture(h: &Harness) -> tracing::subscriber::DefaultGuard {
    tracing::subscriber::set_default(tracing_subscriber::registry().with(h.telemetry.log_layer()))
}

/// Flush, then wait until at least `count` lines are stored. A flush gives
/// up after the SDK's fixed 5s when the machine is loaded, so the read waits
/// for the lines themselves, not for the flush. Call it outside a capture
/// guard: a flush that gives up warns, and that warning must not be stored
/// as one of the test's own lines.
async fn flush_until_stored(h: &Harness, count: usize) {
    h.telemetry.flush();
    eventually(
        h.timeouts.telemetry_pipeline_patience,
        "the captured lines to be stored",
        async || {
            let stored: LogSnapshotResponse = h.get("/v1/logs?tail=100000").await;
            stored.lines.len() >= count
        },
    )
    .await;
}

/// The next SSE message on a log stream, patient the same way
/// [`flush_until_stored`] is: a delta has no snapshot read to poll instead,
/// so the wait is on the message itself, under
/// `Timeouts::telemetry_pipeline_patience` rather than the generic `TIMEOUT`.
async fn next_log_message(h: &Harness, body: &mut Body) -> String {
    match next_sse(body, h.timeouts.telemetry_pipeline_patience).await {
        Sse::Message(message) => message,
        other => panic!("expected an sse message, got {other:?}"),
    }
}

fn messages(lines: &[LogLineDto]) -> Vec<&str> {
    lines.iter().map(|l| l.message.as_str()).collect()
}

/// The default snapshot without an explicit `tail` is bounded (200 lines):
/// the store keeps every record, but `GET /v1/logs` is a recent-activity
/// read, not a full-history dump.
#[tokio::test]
async fn the_default_snapshot_is_bounded_even_without_an_explicit_tail() {
    let h = harness().await;
    {
        let _guard = capture(&h);
        for i in 0..250 {
            tracing::info!("line {i}");
        }
    }
    flush_until_stored(&h, 250).await;

    let snapshot: LogSnapshotResponse = h.get("/v1/logs").await;
    assert_eq!(snapshot.lines.len(), 200);
    assert_eq!(snapshot.lines[0].message, "line 50");
    assert_eq!(snapshot.lines[199].message, "line 249");
}

#[tokio::test]
async fn service_narrows_the_snapshot_to_one_service() {
    let h = harness().await;
    {
        let _guard = capture(&h);
        tracing::info!("from the daemon");
    }
    flush_until_stored(&h, 1).await;
    // `service` always carries the resource's own `service.name`: every
    // record this daemon writes is "ariadned", so the filter either keeps
    // everything or keeps nothing.
    let matching: LogSnapshotResponse = h.get("/v1/logs?service=ariadned").await;
    assert_eq!(messages(&matching.lines), vec!["from the daemon"]);

    let other: LogSnapshotResponse = h.get("/v1/logs?service=ariadne-cli").await;
    assert!(other.lines.is_empty());
}

#[tokio::test]
async fn level_narrows_the_snapshot_to_that_level_and_above() {
    let h = harness().await;
    {
        let _guard = capture(&h);
        tracing::info!("routine");
        tracing::warn!("worth a look");
        tracing::error!("on fire");
    }
    flush_until_stored(&h, 3).await;

    let at_or_above_warn: LogSnapshotResponse = h.get("/v1/logs?level=warn").await;
    assert_eq!(
        messages(&at_or_above_warn.lines),
        vec!["worth a look", "on fire"]
    );

    let at_or_above_error: LogSnapshotResponse = h.get("/v1/logs?level=error").await;
    assert_eq!(messages(&at_or_above_error.lines), vec!["on fire"]);
}

#[tokio::test]
async fn an_unknown_level_is_a_bad_request() {
    let h = harness().await;
    let response = h.response(get("/v1/logs?level=deafening")).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// Confirmed intermittent under the full suite's load on every core at once
// (observed failing in the same run that a sibling OS job, or the isolated
// crate run, passed at the identical revision): the batch export thread
// behind `flush_until_stored` and `next_log_message` starves past
// `telemetry_pipeline_patience` when every core is already busy. See
// `crates/AGENTS.md#checks` for the marker's import and retry rule.
flaky_test! {
    #[tokio::test]
    async fn the_snapshot_returns_captured_lines_in_order() {
        let h = harness().await;
        {
            let _guard = capture(&h);
            tracing::info!("the daemon started");
            tracing::warn!(socket = "/tmp/x.sock", "removing stale socket file");
        }
        flush_until_stored(&h, 2).await;
        // Nothing captured outside the guard leaks in.
        tracing::info!("not captured");

        let snapshot: LogSnapshotResponse = h.get("/v1/logs").await;
        assert_eq!(
            messages(&snapshot.lines),
            vec![
                "the daemon started",
                "removing stale socket file socket=/tmp/x.sock",
            ]
        );
        let line = &snapshot.lines[0];
        assert_eq!(line.level, "INFO");
        assert_eq!(
            line.target,
            module_path!(),
            "the target is the emitting module"
        );
        assert_eq!(line.service, "ariadned");
        assert!(
            chrono::DateTime::parse_from_rfc3339(&line.ts).is_ok(),
            "ts is RFC 3339: {:?}",
            line.ts
        );
        assert_eq!(snapshot.lines[1].level, "WARN");
    }

    #[tokio::test]
    async fn tail_limits_the_snapshot_to_the_last_n_lines() {
        let h = harness().await;
        {
            let _guard = capture(&h);
            for i in 0..5 {
                tracing::info!("line {i}");
            }
        }
        flush_until_stored(&h, 5).await;

        let tailed: LogSnapshotResponse = h.get("/v1/logs?tail=2").await;
        assert_eq!(messages(&tailed.lines), vec!["line 3", "line 4"]);

        let all: LogSnapshotResponse = h.get("/v1/logs?tail=100").await;
        assert_eq!(
            all.lines.len(),
            5,
            "a tail longer than what was written is a no-op"
        );
    }

    #[tokio::test]
    async fn level_narrows_the_stream_snapshot_and_its_deltas() {
        let h = harness().await;
        {
            let _guard = capture(&h);
            tracing::warn!("before the filtered stream opened");
        }
        flush_until_stored(&h, 1).await;

        let mut body = h.stream(get("/v1/logs/stream?level=warn")).await;
        let (name, payload) = parse_sse(&next_log_message(&h, &mut body).await);
        assert_eq!(name, "snapshot");
        let snapshot: LogSnapshotResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(
            messages(&snapshot.lines),
            vec!["before the filtered stream opened"]
        );

        {
            let _guard = capture(&h);
            tracing::info!("skip this delta");
            tracing::warn!("keep this delta");
        }
        h.telemetry.flush();
        let (name, payload) = parse_sse(&next_log_message(&h, &mut body).await);
        assert_eq!(name, "delta");
        let line: LogLineDto = serde_json::from_value(payload).unwrap();
        assert_eq!(line.message, "keep this delta");
    }

    #[tokio::test]
    async fn the_stream_opens_with_a_snapshot_then_follows_with_deltas() {
        let h = harness().await;
        {
            let _guard = capture(&h);
            tracing::info!("before the stream opened");
        }
        flush_until_stored(&h, 1).await;

        let response = h.response(get("/v1/logs/stream")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );
        let mut body = response.into_body();

        let (name, payload) = parse_sse(&next_log_message(&h, &mut body).await);
        assert_eq!(name, "snapshot");
        let snapshot: LogSnapshotResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(messages(&snapshot.lines), vec!["before the stream opened"]);

        {
            let _guard = capture(&h);
            tracing::info!(answer = 42, "a line while the stream is open");
        }
        h.telemetry.flush();
        let (name, payload) = parse_sse(&next_log_message(&h, &mut body).await);
        assert_eq!(name, "delta");
        let line: LogLineDto = serde_json::from_value(payload).unwrap();
        assert_eq!(line.message, "a line while the stream is open answer=42");
        assert_eq!(line.level, "INFO");
        assert_eq!(line.service, "ariadned");

        {
            let _guard = capture(&h);
            tracing::error!("another one");
        }
        h.telemetry.flush();
        let (name, payload) = parse_sse(&next_log_message(&h, &mut body).await);
        assert_eq!(name, "delta");
        let line: LogLineDto = serde_json::from_value(payload).unwrap();
        assert_eq!(line.message, "another one");
        assert_eq!(line.level, "ERROR");
    }

    /// Escape sequences, newlines and quotes in a message ride inside JSON, so
    /// they cannot break SSE framing (nothing in the payload starts a new event).
    #[tokio::test]
    async fn hostile_log_content_survives_sse_framing() {
        let h = harness().await;
        let hostile = "line one\nevent: fake\n\ndata: \"quoted\" \u{1b}[2J";
        {
            let _guard = capture(&h);
            tracing::info!("{hostile}");
        }
        flush_until_stored(&h, 1).await;

        let mut body = h.stream(get("/v1/logs/stream")).await;

        let (name, payload) = parse_sse(&next_log_message(&h, &mut body).await);
        assert_eq!(name, "snapshot");
        let snapshot: LogSnapshotResponse = serde_json::from_value(payload).unwrap();
        assert_eq!(
            messages(&snapshot.lines),
            vec![hostile],
            "the message round-trips byte for byte"
        );
    }

    /// Rule 5: the pipeline counts what it exports on `ariadne.telemetry.records`,
    /// a counter with `signal` and `service` — proof the metrics half of the
    /// pipeline, not only the logs half, is wired end to end.
    #[tokio::test]
    async fn the_pipeline_counts_exported_log_records_on_its_own_metric() {
        let h = harness().await;
        {
            let _guard = capture(&h);
            tracing::info!("counted by the pipeline");
        }
        flush_until_stored(&h, 1).await;

        // The count is recorded once the log export lands, and reaches the store
        // with the next metric export: flush again until it has.
        let log_series = common::eventually_some(TIMEOUT, "the log-signal series", || async {
            h.telemetry.flush();
            let series = h
                .telemetry_store
                .list_series(&[ariadne_store::telemetry::LabelMatcher::new(
                    ariadne_store::telemetry::NAME_LABEL,
                    ariadne_store::telemetry::MatchOp::Eq,
                    "ariadne.telemetry.records",
                )])
                .await
                .unwrap();
            series.into_iter().find(|s| s.labels["signal"] == "log")
        })
        .await;
        assert_eq!(log_series.labels["service"], "ariadned");
        // The exporter puts the resource's service name on every point too,
        // under the key the query routes read it from (034).
        assert_eq!(log_series.labels["__resource_service_name__"], "ariadned");

        let samples = h
            .telemetry_store
            .read_samples(
                &log_series.id,
                ariadne_store::telemetry::TimeRange {
                    since: "1970-01-01T00:00:00.000Z".to_string(),
                    until: "2999-01-01T00:00:00.000Z".to_string(),
                },
            )
            .await
            .unwrap();
        // Cumulative temporality: each sample is the running total as of that
        // export, so the latest one (not a sum across samples) is the count.
        let latest = samples.last().expect("at least one export by now").value;
        assert!(
            latest >= 1.0,
            "at least the one record just exported: {samples:?}"
        );
    }
}

#[tokio::test]
async fn both_endpoints_are_in_the_openapi_document() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/logs"]["get"].is_object());
    assert!(doc["paths"]["/v1/logs/stream"]["get"].is_object());
    assert!(doc["components"]["schemas"]["LogLineDto"].is_object());
    assert!(doc["components"]["schemas"]["LogSnapshotResponse"].is_object());
}
