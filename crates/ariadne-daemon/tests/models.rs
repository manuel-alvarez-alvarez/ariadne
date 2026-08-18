//! Integration tests for the model catalog endpoint.
//!
//! The contract is that `GET /v1/models?agent=<kind>` returns exactly the
//! curated catalog of that agent with its descriptions, and that the endpoint
//! is in the OpenAPI document. OpenCode discovery is not exercised: it
//! depends on an installed `opencode` binary.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use ariadne_api::models::ModelDto;
use ariadne_core::AgentKind;
use ariadne_core::models::curated_models;
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::Store;

async fn router() -> (Router, tempfile::TempDir) {
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
        logs: LogBuffer::new(),
    };
    (http::router(state), dir)
}

async fn get(router: &Router, path: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::get(path).body(Body::empty()).unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, body.to_vec())
}

async fn models(router: &Router, path: &str) -> Vec<ModelDto> {
    let (status, body) = get(router, path).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).unwrap()
}

/// `?agent=<kind>` returns exactly that agent's curated catalog, in order,
/// each entry carrying its description.
#[tokio::test]
async fn curated_agents_return_their_catalog() {
    let (router, _dir) = router().await;
    for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
        let got = models(&router, &format!("/v1/models?agent={}", kind.as_str())).await;
        let want = curated_models(kind);
        assert_eq!(got.len(), want.len());
        for (got, want) in got.iter().zip(want) {
            assert_eq!(got.id, want.id);
            assert_eq!(got.agent_kind, kind);
            assert_eq!(got.description.as_deref(), Some(want.description));
        }
    }
}

/// Without `agent`, the union: every curated entry is present (plus whatever
/// opencode discovery yields on the machine, which is not asserted).
#[tokio::test]
async fn no_param_returns_the_union() {
    let (router, _dir) = router().await;
    let got = models(&router, "/v1/models").await;
    for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
        for want in curated_models(kind) {
            assert!(
                got.iter().any(|m| m.id == want.id && m.agent_kind == kind),
                "missing {}/{}",
                kind.as_str(),
                want.id
            );
        }
    }
}

/// An unknown agent kind is a 400, not a 500 or a silent union.
#[tokio::test]
async fn unknown_agent_is_rejected() {
    let (router, _dir) = router().await;
    let (status, _) = get(&router, "/v1/models?agent=nonsense").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The endpoint is part of the OpenAPI document.
#[tokio::test]
async fn endpoint_is_in_the_openapi_document() {
    let (router, _dir) = router().await;
    let (status, body) = get(&router, "/api-docs/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    let doc: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(doc["paths"]["/v1/models"]["get"].is_object());
    assert!(doc["components"]["schemas"]["ModelDto"].is_object());
}
