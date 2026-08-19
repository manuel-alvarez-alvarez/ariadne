//! Integration tests for the daemon-side environment report.
//!
//! The contract is that `GET /v1/doctor` answers for the daemon's own
//! environment — every agent kind accounted for whether or not it is
//! installed, tmux and git beside them, and the paths this daemon was
//! configured with — and that it answers at all on a host that has none of
//! those binaries, because a report that fails where the news is bad is no
//! report. Which binaries the machine running the tests happens to have is
//! not asserted; that it says something about each of them is.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use ariadne_api::doctor::DaemonReportDto;
use ariadne_core::AgentKind;
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::Store;

async fn harness() -> (Router, Arc<Config>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    harness_in(dir, home).await
}

/// The same daemon around an already prepared home directory — a `config.toml`
/// in it is read as `ariadned` would read it.
async fn harness_with_home(home: std::path::PathBuf) -> (Router, Arc<Config>, tempfile::TempDir) {
    harness_in(tempfile::tempdir().unwrap(), home).await
}

async fn harness_in(
    dir: tempfile::TempDir,
    home: std::path::PathBuf,
) -> (Router, Arc<Config>, tempfile::TempDir) {
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(home)).unwrap());
    let launcher = Arc::new(Launcher {
        cfg: cfg.clone(),
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
    (http::router(state), cfg, dir)
}

async fn get(router: &Router, path: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::get(path).body(Body::empty()).unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, body.to_vec())
}

async fn report(router: &Router) -> DaemonReportDto {
    let (status, body) = get(router, "/v1/doctor").await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).unwrap()
}

/// Every agent kind is accounted for, installed or not: a kind left out of
/// the list would read as one nobody has to worry about.
#[tokio::test]
async fn every_agent_kind_is_reported() {
    let (router, _cfg, _dir) = harness().await;
    let report = report(&router).await;
    assert_eq!(report.agents.len(), AgentKind::ALL.len());
    for (binary, kind) in report.agents.iter().zip(AgentKind::ALL) {
        assert_eq!(binary.agent_kind, Some(kind));
        assert_eq!(binary.name, kind.binary());
        // Found or not, a path always comes with the binary it names.
        if binary.path.is_none() {
            assert!(binary.version.is_none(), "{binary:?}");
        }
    }
}

/// tmux and git are what a session is made of, so they are reported beside
/// the agents rather than left to the caller to ask about.
#[tokio::test]
async fn tmux_and_git_are_reported_as_tools() {
    let (router, _cfg, _dir) = harness().await;
    let report = report(&router).await;
    let names: Vec<&str> = report.tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["tmux", "git"]);
    assert!(report.tools.iter().all(|t| t.agent_kind.is_none()));
}

/// The paths are this daemon's own, not the ambient home's — a report about
/// somebody else's directories would be worse than none.
#[tokio::test]
async fn the_paths_are_the_ones_this_daemon_was_configured_with() {
    let (router, cfg, _dir) = harness().await;
    let report = report(&router).await;
    assert_eq!(report.home, cfg.root.display().to_string());
    assert_eq!(report.socket_path, cfg.socket_path.display().to_string());
    assert_eq!(report.db.path, cfg.db_path.display().to_string());
    assert_eq!(
        report.worktree_root.path,
        cfg.worktree_root.display().to_string()
    );
    assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
}

/// `Config::load` creates the worktree root, so a freshly configured daemon
/// must report it as there and writable — and the report has to leave the
/// directory exactly as it found it, which is why writability is asked of
/// the kernel rather than established by creating a file to see whether one
/// can be created.
#[tokio::test]
async fn the_worktree_root_is_reported_writable_and_left_alone() {
    let (router, cfg, _dir) = harness().await;
    let report = report(&router).await;
    assert!(report.worktree_root.exists);
    assert!(report.worktree_root.writable);
    let leftovers: Vec<_> = std::fs::read_dir(&cfg.worktree_root)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(leftovers.is_empty(), "the report wrote: {leftovers:?}");
}

/// A worktree root the daemon cannot write is the failure doctor exists to
/// name: the mode bits say its owner may write it, and the daemon is not its
/// owner, so nothing short of asking the kernel gets this right.
#[tokio::test]
async fn a_worktree_root_the_daemon_cannot_write_is_reported_as_such() {
    if rustix::process::geteuid().is_root() {
        return; // Root writes everything; there is nothing to tell apart.
    }
    let dir = tempfile::tempdir().unwrap();
    // /usr belongs to root and is 0755: present, plausible, and closed to us.
    std::fs::write(
        dir.path().join("home-config.toml"),
        "worktree_root = \"/usr\"\n",
    )
    .unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::rename(
        dir.path().join("home-config.toml"),
        home.join("config.toml"),
    )
    .unwrap();

    let (router, cfg, _dir) = harness_with_home(home).await;
    assert_eq!(cfg.worktree_root, std::path::Path::new("/usr"));
    let report = report(&router).await;
    assert!(report.worktree_root.exists);
    assert!(!report.worktree_root.writable);
}

/// A database file that does not exist yet is not a failure to report on: the
/// daemon creates it on its first write, and its directory takes it.
#[tokio::test]
async fn a_database_that_is_not_there_yet_is_reported_as_creatable() {
    let (router, _cfg, _dir) = harness().await;
    let report = report(&router).await;
    assert!(!report.db.exists);
    assert!(report.db.writable, "its directory takes the file");
}

/// The endpoint is part of the OpenAPI document.
#[tokio::test]
async fn endpoint_is_in_the_openapi_document() {
    let (router, _cfg, _dir) = harness().await;
    let (status, body) = get(&router, "/api-docs/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    let doc: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(doc["paths"]["/v1/doctor"]["get"].is_object());
    assert!(doc["components"]["schemas"]["DaemonReportDto"].is_object());
}
