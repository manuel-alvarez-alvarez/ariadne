//! Integration tests for TmuxManager and GitManager.
//!
//! Marked #[ignore]: they need `git` and `tmux` on PATH and touch real
//! processes. Run with `cargo test -p ariadne-daemon -- --ignored`; the spawn
//! plan test also needs the `ariadne` CLI built into the same target dir
//! (`cargo build -p ariadne-cli`), since a plan is launched by it.

use std::path::{Path, PathBuf};

use ariadne_core::spawn_plan::SpawnPlanFile;
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
        "git init -q -b main && git config user.email t@t && git config user.name t && \
               echo v1 > file.txt && git add . && git commit -qm init",
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
    git.add_worktree(&repo, &wt, "fix-the-widget-aaa111", "main")
        .await
        .unwrap();
    assert!(wt.join("file.txt").exists());

    // Commit on the task branch.
    sh(
        &wt,
        "echo v2 > file.txt && git add . && git commit -qm change",
    )
    .await;

    // Reviewer worktree, detached at the branch tip.
    let wt_rev = dir.path().join("wt-rev");
    git.add_detached_worktree(&repo, &wt_rev, "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt_rev.join("file.txt"))
            .unwrap()
            .trim(),
        "v2"
    );

    // Diff base...branch shows the change.
    let diff = git
        .diff(&repo, "main", "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert!(
        diff.contains("-v1") && diff.contains("+v2"),
        "unexpected diff: {diff}"
    );

    // Not merged yet.
    assert!(
        !git.is_ancestor(&repo, "fix-the-widget-aaa111", "main")
            .await
            .unwrap()
    );

    // Merge in the primary checkout (what the engineer agent will do).
    sh(&repo, "git merge -q --no-ff fix-the-widget-aaa111 -m merge").await;
    assert!(
        git.is_ancestor(&repo, "fix-the-widget-aaa111", "main")
            .await
            .unwrap()
    );

    // Cleanup: remove worktrees, delete branch.
    git.remove_worktree(&repo, &wt).await.unwrap();
    git.remove_worktree(&repo, &wt_rev).await.unwrap();
    git.delete_branch(&repo, "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert!(
        !git.branch_exists(&repo, "fix-the-widget-aaa111")
            .await
            .unwrap()
    );

    // A new commit lands after crash: re-adding a worktree for an existing
    // branch reuses it.
    git.add_worktree(&repo, &wt, "fix-the-widget-bbb222", "main")
        .await
        .unwrap();
    git.remove_worktree(&repo, &wt).await.unwrap();
    git.add_worktree(&repo, &wt, "fix-the-widget-bbb222", "main")
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
    git.add_worktree(&repo, &wt, "fix-the-widget-aaa111", "main")
        .await
        .unwrap();
    sh(&wt, "echo r1 > file.txt && git add . && git commit -qm r1").await;

    let wt_rev = dir.path().join("wt-rev");
    git.add_detached_worktree(&repo, &wt_rev, "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt_rev.join("file.txt"))
            .unwrap()
            .trim(),
        "r1"
    );

    // Round 2: engineer pushes more commits; reviewer worktree is refreshed.
    sh(&wt, "echo r2 > file.txt && git add . && git commit -qm r2").await;
    git.checkout_detached(&wt_rev, "fix-the-widget-aaa111")
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

    // The screen the pane draws on: what a viewer of those bytes has to
    // reproduce for them to land where they were addressed.
    let geometry = tmux.pane_geometry(&name).await.unwrap();
    assert!(
        geometry.cols > 0 && geometry.rows > 0,
        "pane geometry: {geometry:?}"
    );
    assert!(
        geometry.cursor_x < geometry.cols && geometry.cursor_y < geometry.rows,
        "the cursor is on the screen it was measured with: {geometry:?}"
    );

    tmux.kill_session(&name).await.unwrap();
    assert!(!tmux.has_session(&name).await);
}

/// The spawn plan against the real tmux: a briefing no command line could
/// carry, and a pane whose root process is still the agent.
///
/// A hundred kilobytes of argv is "command too long" from `new-session` —
/// tmux ships a command to its server in a single message capped near 16KB —
/// and that is what took a task down. Through a plan file it is an ordinary
/// session: `ariadne _spawn` applies the environment and `exec`s the program
/// the plan names, so what tmux watches is that program and not the `ariadne`
/// that read the plan for it.
#[tokio::test]
#[ignore = "requires tmux and a built ariadne CLI"]
async fn tmux_runs_a_plan_no_command_line_could_carry() {
    let tmux = TmuxManager::default();
    let dir = tempfile::tempdir().unwrap();
    let name = format!("ariadne-test-plan-{}", std::process::id());
    let log = dir.path().join("console.log");
    let briefing = "B".repeat(100_000);

    // The plan a launch writes: an argument far past what tmux would accept,
    // plus an environment variable that used to arrive as an `-e` pair. The
    // script reports how much of each reached it and then idles, so the pane
    // is still there to be asked about.
    let plan_file = dir.path().join("spawn.json");
    let plan = SpawnPlanFile::new(
        vec![
            "sh".into(),
            "-c".into(),
            "printf 'BRIEFING=%s VAR=%s\\n' \"${#1}\" \"$ARIADNE_TEST_VAR\"; \
             while true; do sleep 1; done"
                .into(),
            // $0, then $1 — the briefing.
            "plan-test".into(),
            briefing.clone(),
        ],
        vec![("ARIADNE_TEST_VAR".into(), "hello-plan".into())],
        dir.path().to_path_buf(),
    );
    std::fs::write(&plan_file, plan.to_json().unwrap()).unwrap();

    tmux.new_session(&TmuxSpawn {
        session: name.clone(),
        cwd: dir.path().to_path_buf(),
        // Empty, as the launcher leaves them: everything is in the plan.
        env: vec![],
        argv: vec![
            ariadne_bin().display().to_string(),
            "_spawn".into(),
            plan_file.display().to_string(),
        ],
        log_file: Some(log.clone()),
    })
    .await
    .unwrap();
    assert!(tmux.has_session(&name).await, "the session started");

    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let pane = tmux.capture_pane(&name, 100).await.unwrap();
    assert!(
        pane.contains(&format!("BRIEFING={}", briefing.len())),
        "the whole briefing reached the program: {pane}"
    );
    assert!(
        pane.contains("VAR=hello-plan"),
        "the plan's environment reached the program: {pane}"
    );

    // The pane's root process is the program the plan named. `ariadne` exec'd
    // itself away, so nothing about pane liveness has a wrapper in it.
    let pid = capture(
        "tmux",
        &["display-message", "-p", "-t", &name, "#{pane_pid}"],
    )
    .await;
    let comm = capture("ps", &["-o", "comm=", "-p", pid.trim()]).await;
    let comm = comm.trim();
    assert!(
        comm.ends_with("sh"),
        "the pane's root process is the plan's program: {comm}"
    );
    assert!(
        !comm.contains("ariadne"),
        "the pane's root process is a wrapper: {comm}"
    );

    tmux.kill_session(&name).await.unwrap();
}

/// The `ariadne` CLI as built beside the test binary (`cargo build`'s target
/// dir), which is the one that speaks this build's plan format.
fn ariadne_bin() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    // target/<profile>/deps/<test-binary>
    let bin = exe
        .parent()
        .and_then(Path::parent)
        .expect("target dir")
        .join("ariadne");
    assert!(
        bin.is_file(),
        "build the CLI first (cargo build -p ariadne-cli): no {}",
        bin.display()
    );
    bin
}

/// Run a command and return its stdout, failing the test if it does not.
async fn capture(bin: &str, args: &[&str]) -> String {
    let out = tokio::process::Command::new(bin)
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "{bin} {args:?} failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
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

/// What `POST /v1/sessions/{id}/resize` relies on: a pane nobody is attached
/// to takes the size it is given, and keeps it.
///
/// This is the half a stubbed tmux cannot answer. tmux sizes a window after
/// the client last attached to it, and these sessions are created detached —
/// so without sizing being taken off its hands the pane stays at 80x24 and a
/// web viewer's resize does nothing at all.
#[tokio::test]
#[ignore = "requires tmux"]
async fn tmux_resizes_a_pane_with_no_client_attached() {
    let tmux = TmuxManager::default();
    let dir = tempfile::tempdir().unwrap();
    let name = format!("ariadne-test-resize-{}", std::process::id());

    tmux.new_session(&TmuxSpawn {
        session: name.clone(),
        cwd: dir.path().to_path_buf(),
        env: vec![],
        argv: vec!["sh".into(), "-c".into(), "sleep 30".into()],
        log_file: None,
    })
    .await
    .unwrap();

    tmux.resize_window(&name, 137, 41).await.unwrap();
    let geometry = tmux.pane_geometry(&name).await.unwrap();
    assert_eq!(
        (geometry.cols, geometry.rows),
        (137, 41),
        "the detached pane draws at the size it was given: {geometry:?}"
    );

    // And the next size wins, the way the last client to attach does.
    tmux.resize_window(&name, 90, 30).await.unwrap();
    let geometry = tmux.pane_geometry(&name).await.unwrap();
    assert_eq!((geometry.cols, geometry.rows), (90, 30));

    // Sizing is ours only for as long as nobody attaches: the hook hands it
    // back the moment a real client arrives, so an `ariadne attach` still
    // sizes the window to the terminal it runs in rather than being shown a
    // window it does not fit.
    let hooks = std::process::Command::new("tmux")
        .args(["show-hooks", "-t", &name])
        .output()
        .unwrap();
    let hooks = String::from_utf8_lossy(&hooks.stdout);
    assert!(
        hooks.contains("client-attached") && hooks.contains("window-size"),
        "the attach hook unsets the manual sizing: {hooks}"
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
    // Reviewers are told apart by profile, not by round: one name for all of
    // a reviewer's rounds on a task.
    assert_eq!(
        session_name(goal, Some(task), "reviewer", Some("dm7hk9pf")),
        "ariadne-tjs6gk44-c5dm7hk9-rev-dm7hk9pf"
    );
}
