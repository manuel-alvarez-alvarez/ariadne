//! Integration tests for `GET /v1/logs` and `GET /v1/logs/stream`.
//!
//! Events are emitted through a thread-default subscriber carrying the
//! capture layer — exactly how the daemon wires it, minus the global install
//! a test process cannot afford — and observed through the HTTP API.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

use ariadne_api::logs::{LogLineDto, LogSnapshotResponse};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::Store;

const TIMEOUT: Duration = Duration::from_secs(5);

struct Harness {
    router: Router,
    logs: LogBuffer,
    _dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    build(LogBuffer::new()).await
}

async fn build(logs: LogBuffer) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: TmuxManager::default(),
        git: GitManager,
    });
    let state = AppState {
        store,
        started_at: Instant::now(),
        launcher,
        sched_tx: None,
        events: bus,
        logs: logs.clone(),
    };
    Harness {
        router: http::router(state),
        logs,
        _dir: dir,
    }
}

impl Harness {
    /// The capture layer as the daemon installs it, scoped to this thread:
    /// while the guard lives, `tracing` macros feed the buffer behind the API.
    fn capture(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(tracing_subscriber::registry().with(self.logs.layer()))
    }

    async fn snapshot(&self, path: &str) -> LogSnapshotResponse {
        let request = Request::get(path).body(Body::empty()).unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&body).unwrap()
    }
}

/// Read from an SSE body until one complete message (`\n\n`-terminated) is in.
async fn next_sse_message(body: &mut Body) -> String {
    tokio::time::timeout(TIMEOUT, async {
        let mut buf = String::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.expect("sse body error");
            if let Some(chunk) = frame.data_ref() {
                buf.push_str(&String::from_utf8_lossy(chunk));
                if buf.contains("\n\n") {
                    return buf;
                }
            }
        }
        panic!("the stream closed instead of sending a message");
    })
    .await
    .expect("expected an sse message within the timeout")
}

/// `event:` name and decoded `data:` payload of one SSE message.
fn parse(message: &str) -> (String, serde_json::Value) {
    let mut name = None;
    let mut data = None;
    for line in message.trim_end().lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            name = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("data: ") {
            assert!(
                data.is_none(),
                "payload must fit one data line: {message:?}"
            );
            data = Some(rest.to_string());
        }
    }
    let name = name.expect("every message carries an event name");
    let data = data.expect("every message carries a payload");
    (name, serde_json::from_str(&data).expect("payload is JSON"))
}

fn messages(lines: &[LogLineDto]) -> Vec<&str> {
    lines.iter().map(|l| l.message.as_str()).collect()
}

#[tokio::test]
async fn the_snapshot_returns_captured_lines_in_order() {
    let h = harness().await;
    {
        let _guard = h.capture();
        tracing::info!("the daemon started");
        tracing::warn!(socket = "/tmp/x.sock", "removing stale socket file");
    }
    // Nothing captured outside the guard leaks in.
    tracing::info!("not captured");

    let snapshot = h.snapshot("/v1/logs").await;
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
        let _guard = h.capture();
        for i in 0..5 {
            tracing::info!("line {i}");
        }
    }

    let tailed = h.snapshot("/v1/logs?tail=2").await;
    assert_eq!(messages(&tailed.lines), vec!["line 3", "line 4"]);

    let all = h.snapshot("/v1/logs?tail=100").await;
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
    let h = build(LogBuffer::with_capacity(3)).await;
    {
        let _guard = h.capture();
        for i in 0..5 {
            tracing::info!("line {i}");
        }
    }

    let snapshot = h.snapshot("/v1/logs").await;
    assert_eq!(
        messages(&snapshot.lines),
        vec!["line 2", "line 3", "line 4"],
        "only the newest `capacity` lines survive"
    );
}

#[tokio::test]
async fn the_stream_opens_with_a_snapshot_then_follows_with_deltas() {
    let h = harness().await;
    let _guard = h.capture();
    tracing::info!("before the stream opened");

    let response = h
        .router
        .clone()
        .oneshot(Request::get("/v1/logs/stream").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    let mut body = response.into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "snapshot");
    let snapshot: LogSnapshotResponse = serde_json::from_value(payload).unwrap();
    assert_eq!(messages(&snapshot.lines), vec!["before the stream opened"]);

    tracing::info!(answer = 42, "a line while the stream is open");
    let (name, payload) = parse(&next_sse_message(&mut body).await);
    assert_eq!(name, "delta");
    let line: LogLineDto = serde_json::from_value(payload).unwrap();
    assert_eq!(line.message, "a line while the stream is open answer=42");
    assert_eq!(line.level, "INFO");

    tracing::error!("another one");
    let (name, payload) = parse(&next_sse_message(&mut body).await);
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
    let _guard = h.capture();
    let hostile = "line one\nevent: fake\n\ndata: \"quoted\" \u{1b}[2J";
    tracing::info!("{hostile}");

    let response = h
        .router
        .clone()
        .oneshot(Request::get("/v1/logs/stream").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let mut body = response.into_body();

    let (name, payload) = parse(&next_sse_message(&mut body).await);
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
    let request = Request::get("/api-docs/openapi.json")
        .body(Body::empty())
        .unwrap();
    let response = h.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let doc: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(doc["paths"]["/v1/logs"]["get"].is_object());
    assert!(doc["paths"]["/v1/logs/stream"]["get"].is_object());
    assert!(doc["components"]["schemas"]["LogLineDto"].is_object());
    assert!(doc["components"]["schemas"]["LogSnapshotResponse"].is_object());
}
