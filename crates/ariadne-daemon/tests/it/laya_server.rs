//! The local Laya server and its daily refresh clock.

use axum::body::Body;
use axum::http::StatusCode;
use serde_json::json;

use ariadne_api::permissions::{LayaState, LayaStatusDto};
use ariadne_daemon::timeouts::Timeouts;

use crate::common::{Harness, TIMEOUT, eventually, harness, post, put_json, shared_script};

const SERVER: &str = r#"#!/usr/bin/env python3
import http.server, os, sys
with open(sys.argv[1], 'w') as f:
    f.write(str(os.getpid()) + '\n' + os.environ['LAYA_MODELS'] + '\n' + os.environ['HF_HOME'])
class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        self.send_response(200 if self.path == '/v1/systemone' else 404)
        self.end_headers()
    def log_message(self, *args): pass
http.server.HTTPServer((os.environ['LAYA_HOST'], int(os.environ['LAYA_PORT'])), Handler).serve_forever()
"#;

fn python() -> String {
    shared_script("#!/bin/sh\necho 'Python 3.12.1'\n")
        .display()
        .to_string()
}

fn installer() -> Vec<String> {
    vec!["/usr/bin/true".into()]
}

async fn release() -> (String, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new().route("/release", axum::routing::get(|| async { axum::Json(json!({"tag_name":"v1", "assets":[{"browser_download_url":"https://example.test/laya.whl"}]})) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/release", listener.local_addr().unwrap());
    (
        url,
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }),
    )
}

async fn ready(h: &Harness) -> LayaStatusDto {
    eventually(TIMEOUT, "Laya to be ready", || async {
        h.get::<LayaStatusDto>("/v1/permissions/laya").await.state == LayaState::Ready
    })
    .await;
    h.get("/v1/permissions/laya").await
}

fn enable(checkpoints: &str) -> axum::http::Request<Body> {
    put_json(
        "/v1/permissions/laya",
        json!({"enabled":true,"checkpoints":checkpoints}),
    )
}

#[tokio::test]
async fn a_ready_laya_starts_the_server_with_its_selected_weights() {
    let (url, task) = release().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let server = shared_script(SERVER);
    let h = harness()
        .python_bin(python())
        .laya_release_url(url)
        .laya_installer(installer())
        .laya_serve_command(vec![
            server.display().to_string(),
            record.path().display().to_string(),
        ])
        .await;
    let _: LayaStatusDto = h.json(enable("all"), StatusCode::OK).await;
    ready(&h).await;
    eventually(TIMEOUT, "the Laya endpoint", || async {
        h.get::<LayaStatusDto>("/v1/permissions/laya")
            .await
            .endpoint
            .is_some()
    })
    .await;
    let status: LayaStatusDto = h.get("/v1/permissions/laya").await;
    assert!(status.endpoint.is_some());
    let saw = std::fs::read_to_string(record.path()).unwrap();
    assert!(saw.contains("english,multilingual,typed-decisions"));
    assert!(saw.contains("/laya/hf"));
    let _: LayaStatusDto = h
        .json(
            put_json("/v1/permissions/laya", json!({"enabled":false})),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the server endpoint to clear", || async {
        h.get::<LayaStatusDto>("/v1/permissions/laya")
            .await
            .endpoint
            .is_none()
    })
    .await;
    h.state.laya.shutdown().await;
    task.abort();
}

#[tokio::test]
async fn the_schedule_refreshes_once_per_local_day() {
    let (url, task) = release().await;
    let h = harness()
        .python_bin(python())
        .laya_release_url(url)
        .laya_installer(installer())
        .timeouts(Timeouts {
            laya_schedule_poll: std::time::Duration::from_millis(50),
            ..Timeouts::default()
        })
        .await;
    let now = chrono::Local::now().format("%H:%M").to_string();
    let _: LayaStatusDto = h
        .json(
            put_json(
                "/v1/permissions/laya",
                json!({"enabled":true,"schedule":now}),
            ),
            StatusCode::OK,
        )
        .await;
    ready(&h).await;
    eventually(TIMEOUT, "a scheduled refresh marker", || async {
        h.store
            .laya_settings()
            .await
            .unwrap()
            .last_scheduled_refresh
            .is_some()
    })
    .await;
    let marker = h
        .store
        .laya_settings()
        .await
        .unwrap()
        .last_scheduled_refresh;
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(
        h.store
            .laya_settings()
            .await
            .unwrap()
            .last_scheduled_refresh,
        marker
    );
    h.state.laya.shutdown().await;
    task.abort();
}

#[tokio::test]
async fn a_refresh_and_an_unexpected_exit_restart_the_server() {
    let (url, task) = release().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let server = shared_script(SERVER);
    let h = harness()
        .python_bin(python())
        .laya_release_url(url)
        .laya_installer(installer())
        .laya_serve_command(vec![
            server.display().to_string(),
            record.path().display().to_string(),
        ])
        .await;
    let _: LayaStatusDto = h.json(enable("english"), StatusCode::OK).await;
    ready(&h).await;
    eventually(TIMEOUT, "the first server", || async {
        std::fs::read_to_string(record.path())
            .ok()
            .is_some_and(|record| record.lines().next().is_some_and(|pid| !pid.is_empty()))
    })
    .await;
    let first = std::fs::read_to_string(record.path())
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    let _: LayaStatusDto = h
        .json(post("/v1/permissions/laya/refresh"), StatusCode::ACCEPTED)
        .await;
    eventually(TIMEOUT, "the refreshed server", || async {
        std::fs::read_to_string(record.path())
            .ok()
            .and_then(|s| s.lines().next().map(str::to_owned))
            .is_some_and(|pid| pid != first)
    })
    .await;
    let second = std::fs::read_to_string(record.path())
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    let status = std::process::Command::new("/bin/kill")
        .args(["-9", &second])
        .status()
        .unwrap();
    assert!(status.success());
    eventually(TIMEOUT, "the restarted server", || async {
        std::fs::read_to_string(record.path())
            .ok()
            .and_then(|s| s.lines().next().map(str::to_owned))
            .is_some_and(|pid| pid != second)
    })
    .await;
    h.state.laya.shutdown().await;
    task.abort();
}

#[tokio::test]
async fn a_server_that_never_answers_health_is_not_live() {
    let (url, task) = release().await;
    let h = harness()
        .python_bin(python())
        .laya_release_url(url)
        .laya_installer(installer())
        .laya_serve_command(vec!["/bin/sleep".into(), "10".into()])
        .timeouts(Timeouts {
            laya_serve_start: std::time::Duration::from_millis(100),
            laya_serve_restart: std::time::Duration::from_secs(10),
            ..Timeouts::default()
        })
        .await;
    let _: LayaStatusDto = h.json(enable("english"), StatusCode::OK).await;
    ready(&h).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let status: LayaStatusDto = h.get("/v1/permissions/laya").await;
    assert_eq!(status.endpoint, None);
    h.state.laya.shutdown().await;
    task.abort();
}

#[tokio::test]
async fn a_ready_enabled_laya_starts_after_a_daemon_restart() {
    let (url, task) = release().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let server = shared_script(SERVER);
    let first = harness()
        .python_bin(python())
        .laya_release_url(url)
        .laya_installer(installer())
        .laya_serve_command(vec![
            server.display().to_string(),
            record.path().display().to_string(),
        ])
        .await;
    let _: LayaStatusDto = first.json(enable("english"), StatusCode::OK).await;
    ready(&first).await;
    eventually(TIMEOUT, "the first server", || async {
        std::fs::read_to_string(record.path()).is_ok()
    })
    .await;
    first.state.laya.shutdown().await;
    std::fs::write(record.path(), "").unwrap();
    let second = ariadne_daemon::laya::Laya::new(
        first.store.clone(),
        first.bus.clone(),
        &first.launcher.cfg,
        first.timeouts,
    );
    eventually(TIMEOUT, "the restarted daemon server", || async {
        std::fs::read_to_string(record.path())
            .ok()
            .is_some_and(|record| !record.is_empty())
    })
    .await;
    assert_eq!(second.status().await.state, LayaState::Ready);
    second.shutdown().await;
    task.abort();
}
