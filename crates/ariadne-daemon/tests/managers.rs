//! Integration tests for TmuxManager and GitManager.
//!
//! Marked #[ignore]: they need `git` and `tmux` on PATH and touch real
//! processes. Run with `cargo test -p ariadne-daemon -- --ignored`.

use std::path::{Path, PathBuf};

use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::tmux::{TmuxManager, TmuxSpawn, session_name};

async fn sh(dir: &Path, cmd: &str) {
    let status = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .status()
        .await
        .unwrap();
    assert!(status.success(), "command failed: {cmd}");
}

/// Create a toy repo with an initial commit on `main`.
async fn toy_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    sh(
        &repo,
        "git init -q -b main && echo v1 > file.txt && git add . && \
               git -c user.email=t@t -c user.name=t commit -qm init",
    )
    .await;
    (dir, repo)
}

#[tokio::test]
#[ignore = "requires git"]
async fn git_worktree_lifecycle_and_merge_verification() {
    let (dir, repo) = toy_repo().await;
    let git = GitManager;

    // Engineer worktree on a new branch.
    let wt = dir.path().join("wt-eng");
    git.add_worktree(&repo, &wt, "ariadne/task-1", "main")
        .await
        .unwrap();
    assert!(wt.join("file.txt").exists());

    // Commit on the task branch.
    sh(
        &wt,
        "echo v2 > file.txt && git add . && git -c user.email=t@t -c user.name=t commit -qm change",
    )
    .await;

    // Reviewer worktree, detached at the branch tip.
    let wt_rev = dir.path().join("wt-rev");
    git.add_detached_worktree(&repo, &wt_rev, "ariadne/task-1")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt_rev.join("file.txt"))
            .unwrap()
            .trim(),
        "v2"
    );

    // Diff base...branch shows the change.
    let diff = git.diff(&repo, "main", "ariadne/task-1").await.unwrap();
    assert!(
        diff.contains("-v1") && diff.contains("+v2"),
        "unexpected diff: {diff}"
    );

    // Not merged yet.
    assert!(
        !git.is_ancestor(&repo, "ariadne/task-1", "main")
            .await
            .unwrap()
    );

    // Merge in the primary checkout (what the engineer agent will do).
    sh(&repo, "git merge -q --no-ff ariadne/task-1 -m merge").await;
    assert!(
        git.is_ancestor(&repo, "ariadne/task-1", "main")
            .await
            .unwrap()
    );

    // Cleanup: remove worktrees, delete branch.
    git.remove_worktree(&repo, &wt).await.unwrap();
    git.remove_worktree(&repo, &wt_rev).await.unwrap();
    git.delete_branch(&repo, "ariadne/task-1").await.unwrap();
    assert!(!git.branch_exists(&repo, "ariadne/task-1").await.unwrap());

    // A new commit lands after crash: re-adding a worktree for an existing
    // branch reuses it.
    git.add_worktree(&repo, &wt, "ariadne/task-2", "main")
        .await
        .unwrap();
    git.remove_worktree(&repo, &wt).await.unwrap();
    git.add_worktree(&repo, &wt, "ariadne/task-2", "main")
        .await
        .unwrap();
    git.remove_worktree(&repo, &wt).await.unwrap();
}

#[tokio::test]
#[ignore = "requires git"]
async fn reviewer_worktree_refresh_between_rounds() {
    let (dir, repo) = toy_repo().await;
    let git = GitManager;

    let wt = dir.path().join("wt-eng");
    git.add_worktree(&repo, &wt, "ariadne/task-1", "main")
        .await
        .unwrap();
    sh(
        &wt,
        "echo r1 > file.txt && git add . && git -c user.email=t@t -c user.name=t commit -qm r1",
    )
    .await;

    let wt_rev = dir.path().join("wt-rev");
    git.add_detached_worktree(&repo, &wt_rev, "ariadne/task-1")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt_rev.join("file.txt"))
            .unwrap()
            .trim(),
        "r1"
    );

    // Round 2: engineer pushes more commits; reviewer worktree is refreshed.
    sh(
        &wt,
        "echo r2 > file.txt && git add . && git -c user.email=t@t -c user.name=t commit -qm r2",
    )
    .await;
    git.checkout_detached(&wt_rev, "ariadne/task-1")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt_rev.join("file.txt"))
            .unwrap()
            .trim(),
        "r2"
    );
}

#[tokio::test]
#[ignore = "requires tmux"]
async fn tmux_session_lifecycle() {
    let tmux = TmuxManager::default();
    let dir = tempfile::tempdir().unwrap();
    let name = format!("ariadne-test-{}", std::process::id());
    let log = dir.path().join("console.log");

    let spawn = TmuxSpawn {
        session: name.clone(),
        cwd: dir.path().to_path_buf(),
        env: vec![("ARIADNE_TEST_VAR".into(), "hello-tmux".into())],
        // Emits repeatedly: pipe-pane only sees output produced after it
        // attaches, so a one-shot echo would race it.
        argv: vec![
            "sh".into(),
            "-c".into(),
            "while true; do echo VAR=$ARIADNE_TEST_VAR; pwd; sleep 0.2; done".into(),
        ],
        log_file: Some(log.clone()),
    };
    tmux.new_session(&spawn).await.unwrap();
    assert!(tmux.has_session(&name).await);
    assert!(tmux.list_sessions().await.unwrap().contains(&name));

    // Give the shell a moment, then capture output.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let pane = tmux.capture_pane(&name, 100).await.unwrap();
    assert!(pane.contains("VAR=hello-tmux"), "pane: {pane}");
    assert!(
        pane.contains(dir.path().file_name().unwrap().to_str().unwrap()),
        "cwd not honored: {pane}"
    );

    // pipe-pane wrote the console log too.
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(logged.contains("VAR=hello-tmux"), "log: {logged}");

    tmux.kill_session(&name).await.unwrap();
    assert!(!tmux.has_session(&name).await);
}

/// What `POST /v1/sessions/{id}/input` relies on: the bytes a terminal emits
/// arrive at the pane unaltered — Return submits, Ctrl-C interrupts, and an
/// escape sequence stays an escape sequence rather than becoming literal text.
#[tokio::test]
#[ignore = "requires tmux"]
async fn tmux_send_raw_delivers_control_bytes_verbatim() {
    let tmux = TmuxManager::default();
    let dir = tempfile::tempdir().unwrap();
    let name = format!("ariadne-test-raw-{}", std::process::id());

    // `cat -v` renders control bytes visibly, so the pane shows what arrived.
    tmux.new_session(&TmuxSpawn {
        session: name.clone(),
        cwd: dir.path().to_path_buf(),
        env: vec![],
        argv: vec!["sh".into(), "-c".into(), "cat -v".into()],
        log_file: None,
    })
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Text, an Up-arrow escape sequence, then Return.
    tmux.send_raw(&name, b"hi\x1b[A\r").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let pane = tmux.capture_pane(&name, 100).await.unwrap();
    assert!(
        pane.contains("hi^[[A"),
        "the escape sequence reached the pane as bytes: {pane}"
    );

    // Ctrl-C is a signal, not text: it kills `cat` and the session with it.
    tmux.send_raw(&name, b"\x03").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        !tmux.has_session(&name).await,
        "ctrl-c interrupted the process instead of being typed"
    );

    let _ = tmux.kill_session(&name).await;
}

#[test]
fn session_names_are_stable_and_short() {
    let goal = "01m02trjnexw78vdrftjs6gk44";
    let task = "01m02trjp2sf4mb93vc5dm7hk9";
    assert_eq!(
        session_name(goal, None, "planner", None),
        "ariadne-tjs6gk44-pla"
    );
    assert_eq!(
        session_name(goal, Some(task), "engineer", None),
        "ariadne-tjs6gk44-c5dm7hk9-eng"
    );
    assert_eq!(
        session_name(goal, Some(task), "reviewer", Some(2)),
        "ariadne-tjs6gk44-c5dm7hk9-rev-r2"
    );
}
