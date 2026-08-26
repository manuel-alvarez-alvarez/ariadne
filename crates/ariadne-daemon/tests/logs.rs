//! Integration tests for `GET /v1/logs` and `GET /v1/logs/stream`.
//!
//! Events are emitted through a thread-default subscriber carrying the
//! capture layer — exactly how the daemon wires it, minus the global install
//! a test process cannot afford — and observed through the HTTP API.

mod common;

use axum::http::{StatusCode, header};
use tracing_subscriber::layer::SubscriberExt;

use ariadne_api::logs::{LogLineDto, LogSnapshotResponse};
use ariadne_daemon::log::LogBuffer;

use common::{Harness, get, harness, next_sse_message, parse_sse};

/// The capture layer as the daemon installs it, scoped to this thread: while
/// the guard lives, `tracing` macros feed the buffer behind the API.
fn capture(h: &Harness) -> tracing::subscriber::DefaultGuard {
    tracing::subscriber::set_default(tracing_subscriber::registry().with(h.logs.layer()))
}

fn messages(lines: &[LogLineDto]) -> Vec<&str> {
    lines.iter().map(|l| l.message.as_str()).collect()
}

#[tokio::test]
async fn the_snapshot_returns_captured_lines_in_order() {
    let h = harness().await;
    {
        let _guard = capture(&h);
        tracing::info!("the daemon started");
        tracing::warn!(socket = "/tmp/x.sock", "removing stale socket file");
    }
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
    assert_eq!(line.target, "logs", "the target is the emitting module");
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

    let tailed: LogSnapshotResponse = h.get("/v1/logs?tail=2").await;
    assert_eq!(messages(&tailed.lines), vec!["line 3", "line 4"]);

    let all: LogSnapshotResponse = h.get("/v1/logs?tail=100").await;
    assert_eq!(
        all.lines.len(),
        5,
        "a tail longer than the buffer is a no-op"
    );
}

/// The buffer is a bounded ring: past capacity the oldest lines are dropped,
/// so memory cannot grow with daemon lifetime.
#[tokio::test]
async fn the_ring_buffer_evicts_its_oldest_lines() {
    let h = harness().logs(LogBuffer::with_capacity(3)).await;
    {
        let _guard = capture(&h);
        for i in 0..5 {
            tracing::info!("line {i}");
        }
    }

    let snapshot: LogSnapshotResponse = h.get("/v1/logs").await;
    assert_eq!(
        messages(&snapshot.lines),
        vec!["line 2", "line 3", "line 4"],
        "only the newest `capacity` lines survive"
    );
}

#[tokio::test]
async fn the_stream_opens_with_a_snapshot_then_follows_with_deltas() {
    let h = harness().await;
    let _guard = capture(&h);
    tracing::info!("before the stream opened");

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

    let (name, payload) = parse_sse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    let snapshot: LogSnapshotResponse = serde_json::from_value(payload).unwrap();
    assert_eq!(messages(&snapshot.lines), vec!["before the stream opened"]);

    tracing::info!(answer = 42, "a line while the stream is open");
    let (name, payload) = parse_sse(&next_sse_message(&mut body).await);
    assert_eq!(name, "delta");
    let line: LogLineDto = serde_json::from_value(payload).unwrap();
    assert_eq!(line.message, "a line while the stream is open answer=42");
    assert_eq!(line.level, "INFO");

    tracing::error!("another one");
    let (name, payload) = parse_sse(&next_sse_message(&mut body).await);
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
    let _guard = capture(&h);
    let hostile = "line one\nevent: fake\n\ndata: \"quoted\" \u{1b}[2J";
    tracing::info!("{hostile}");

    let mut body = h.stream(get("/v1/logs/stream")).await;

    let (name, payload) = parse_sse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    let snapshot: LogSnapshotResponse = serde_json::from_value(payload).unwrap();
    assert_eq!(
        messages(&snapshot.lines),
        vec![hostile],
        "the message round-trips byte for byte"
    );
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
