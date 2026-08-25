//! Integration tests for the daemon-side environment report.
//!
//! The contract is that `GET /v1/doctor` answers for the daemon's own
//! environment — every agent kind accounted for whether or not it is
//! installed, tmux and git beside them, and the paths this daemon was
//! configured with — and that it answers at all on a host that has none of
//! those binaries, because a report that fails where the news is bad is no
//! report. Which binaries the machine running the tests happens to have is
//! not asserted; that it says something about each of them is.

mod common;

use ariadne_api::doctor::DaemonReportDto;
use ariadne_core::AgentKind;

use common::{Harness, harness};

async fn report(h: &Harness) -> DaemonReportDto {
    h.get("/v1/doctor").await
}

/// Every agent kind is accounted for, installed or not: a kind left out of
/// the list would read as one nobody has to worry about.
#[tokio::test]
async fn every_agent_kind_is_reported() {
    let h = harness().await;
    let report = report(&h).await;
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

/// tmux and git are what a session is made of, and `gh` and `glab` are what a
/// published task is watched through, so all four are reported beside the
/// agents rather than left to the caller to ask about. A forge CLI that is
/// missing or signed out fails every poll of a pull request, and there is
/// nowhere else that shows.
#[tokio::test]
async fn the_tools_a_session_and_a_published_task_need_are_reported() {
    let h = harness().await;
    let report = report(&h).await;
    let names: Vec<&str> = report.tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["tmux", "git", "gh", "glab"]);
    assert!(report.tools.iter().all(|t| t.agent_kind.is_none()));

    // Which of them are installed on the machine running the tests is not
    // this test's business; that each is asked the questions that apply to it
    // is. Only the forge CLIs have credentials, and only one that was found
    // can be asked about them.
    for tool in &report.tools {
        let forge = matches!(tool.name.as_str(), "gh" | "glab");
        if !forge || tool.path.is_none() {
            assert_eq!(tool.authenticated, None, "{tool:?}");
        }
    }
    assert!(
        report.agents.iter().all(|a| a.authenticated.is_none()),
        "an agent CLI signs in to nothing this can ask about"
    );
}

/// The paths are this daemon's own, not the ambient home's — a report about
/// somebody else's directories would be worse than none. `Config::load`
/// creates the worktree root, so a freshly configured daemon reports it as
/// there and writable; and the report leaves the directory exactly as it
/// found it, which is why writability is asked of the kernel rather than
/// established by creating a file to see whether one can be created.
#[tokio::test]
async fn the_paths_are_this_daemons_own_and_are_left_as_they_were() {
    let h = harness().await;
    let cfg = h.launcher.cfg.clone();
    let report = report(&h).await;
    assert_eq!(report.home, cfg.root.display().to_string());
    assert_eq!(report.socket_path, cfg.socket_path.display().to_string());
    assert_eq!(report.db.path, cfg.db_path.display().to_string());
    assert_eq!(
        report.worktree_root.path,
        cfg.worktree_root.display().to_string()
    );
    assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
    assert!(report.worktree_root.exists);
    assert!(report.worktree_root.writable);
    let leftovers: Vec<_> = std::fs::read_dir(&cfg.worktree_root)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(leftovers.is_empty(), "the report wrote: {leftovers:?}");

    // A database file that does not exist yet is not a failure to report on:
    // the daemon creates it on its first write, and its directory takes it.
    assert!(!report.db.exists);
    assert!(report.db.writable, "its directory takes the file");
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
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    // /usr belongs to root and is 0755: present, plausible, and closed to us.
    std::fs::write(home.join("config.toml"), "worktree_root = \"/usr\"\n").unwrap();

    let h = harness().home(home).await;
    assert_eq!(
        h.launcher.cfg.worktree_root,
        std::path::Path::new("/usr"),
        "the daemon was configured with the home it was given"
    );
    let report = report(&h).await;
    assert!(report.worktree_root.exists);
    assert!(!report.worktree_root.writable);
}

/// The same through the endpoint for the other way a directory refuses new
/// entries: write permission without search. Creating a worktree in it is a
/// lookup followed by a write, so it fails, and a report reading the mode
/// bits would have called it healthy.
#[tokio::test]
async fn a_worktree_root_that_cannot_be_searched_is_reported_as_such() {
    use std::os::unix::fs::PermissionsExt;

    if rustix::process::geteuid().is_root() {
        return; // Root searches anything; there is nothing to tell apart.
    }
    let h = harness().await;
    let root = h.launcher.cfg.worktree_root.clone();
    let mode = std::fs::metadata(&root).unwrap().permissions();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o600)).unwrap();
    let report = report(&h).await;
    std::fs::set_permissions(&root, mode).unwrap();

    assert!(report.worktree_root.exists);
    assert!(!report.worktree_root.writable);
}

/// The endpoint is part of the OpenAPI document.
#[tokio::test]
async fn endpoint_is_in_the_openapi_document() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/doctor"]["get"].is_object());
    assert!(doc["components"]["schemas"]["DaemonReportDto"].is_object());
}
