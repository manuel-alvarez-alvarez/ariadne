//! Select session locations without changing the process environment.
use ariadne_daemon::place::Place;
use std::os::unix::fs::PermissionsExt;

#[test]
fn automatic_selection_uses_tmux_before_a_direct_child() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("tmux");
    std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Herdr remains unavailable, even if its executable exists.
    std::fs::copy(&binary, dir.path().join("herdr")).unwrap();
    assert_eq!(
        Place::find(None, Some(dir.path().as_os_str())).unwrap(),
        Place::Tmux(binary)
    );
    assert_eq!(
        Place::find(Some("none"), Some(dir.path().as_os_str())).unwrap(),
        Place::None
    );
}

#[test]
fn a_missing_forced_multiplexer_refuses_launch() {
    for name in ["tmux", "herdr"] {
        assert_eq!(
            Place::find(Some(name), None).unwrap_err().to_string(),
            format!("forced session place {name} was not found")
        );
    }
}

#[test]
fn automatic_selection_uses_a_direct_child_without_multiplexers() {
    assert_eq!(Place::find(None, None).unwrap(), Place::None);
}

struct PaneHome(tempfile::TempDir);
impl Drop for PaneHome {
    fn drop(&mut self) {
        use std::io::{Read, Write};
        if let Ok(entries) = std::fs::read_dir(self.0.path().join("relays")) {
            for entry in entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "sock"))
            {
                if let Ok(mut socket) = std::os::unix::net::UnixStream::connect(entry.path()) {
                    let _ = socket.set_read_timeout(Some(crate::common::TIMEOUT));
                    let _ = socket.write_all(b"{\"type\":\"stop\"}\n");
                    let _ = socket.read_to_end(&mut Vec::new());
                }
            }
        }
        let _ = std::process::Command::new("tmux")
            .arg("-S")
            .arg(self.0.path().join("tmux.sock"))
            .arg("kill-server")
            .output();
    }
}

#[tokio::test]
async fn a_detached_relay_pane_can_be_found_and_stopped() {
    use crate::common::{TIMEOUT, eventually};
    use ariadne_daemon::place::PaneLaunch;
    let home = PaneHome(tempfile::tempdir_in("/tmp").unwrap());
    let place = Place::find(Some("tmux"), std::env::var_os("PATH").as_deref()).unwrap();
    let args = vec![
        "-u".into(),
        "-c".into(),
        "import sys; [print(line, flush=True) for line in sys.stdin]".into(),
    ];
    let pane = place
        .start_pane(&PaneLaunch {
            home: home.0.path(),
            cwd: home.0.path(),
            session_id: "pane-test",
            launch_id: "launch-test",
            agent_id: "stub",
            task_id: "task",
            daemon: std::path::Path::new(env!("CARGO_BIN_EXE_ariadned")),
            program: "python3",
            args: &args,
            env: &[],
        })
        .await
        .unwrap();
    eventually(TIMEOUT, "relay socket", || async {
        home.0.path().join("relays/pane-test.sock").exists()
    })
    .await;
    assert_eq!(
        place.live_panes(home.0.path()).await.unwrap(),
        vec![pane.clone()]
    );
    place.stop_pane(home.0.path(), &pane).await.unwrap();
    assert!(place.live_panes(home.0.path()).await.unwrap().is_empty());
}

#[tokio::test]
async fn listing_panes_without_a_tmux_server_returns_an_empty_list() {
    let home = PaneHome(tempfile::tempdir_in("/tmp").unwrap());
    let place = Place::find(Some("tmux"), std::env::var_os("PATH").as_deref()).unwrap();
    assert!(place.live_panes(home.0.path()).await.unwrap().is_empty());
}

#[tokio::test]
async fn listing_panes_on_a_server_without_a_session_returns_an_empty_list() {
    let home = PaneHome(tempfile::tempdir_in("/tmp").unwrap());
    let place = Place::find(Some("tmux"), std::env::var_os("PATH").as_deref()).unwrap();
    // A server that outlives its last session answers `list-panes -a` with "no current target".
    let output = std::process::Command::new("tmux")
        .arg("-S")
        .arg(home.0.path().join("tmux.sock"))
        .args(["-f", "/dev/null", "start-server", ";"])
        .args(["set-option", "-g", "exit-empty", "off", ";"])
        .args(["list-sessions"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(place.live_panes(home.0.path()).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_pane_listing_directly_after_the_start_finds_the_pane() {
    use ariadne_daemon::place::PaneLaunch;
    let home = PaneHome(tempfile::tempdir_in("/tmp").unwrap());
    let place = Place::find(Some("tmux"), std::env::var_os("PATH").as_deref()).unwrap();
    let args = vec!["-c".into(), "import time; time.sleep(30)".into()];
    for round in 0..5 {
        let session = format!("direct-{round}");
        let pane = place
            .start_pane(&PaneLaunch {
                home: home.0.path(),
                cwd: home.0.path(),
                session_id: &session,
                launch_id: &session,
                agent_id: "stub",
                task_id: "task",
                daemon: std::path::Path::new(env!("CARGO_BIN_EXE_ariadned")),
                program: "python3",
                args: &args,
                env: &[],
            })
            .await
            .unwrap();
        assert!(
            place
                .live_panes(home.0.path())
                .await
                .unwrap()
                .contains(&pane),
            "round {round}"
        );
    }
}

struct WarningCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for WarningCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_tmux_that_cannot_run_fails_the_pane_listing() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("tmux");
    std::fs::write(
        &binary,
        "#!/bin/sh\necho 'protocol version mismatch' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let place = Place::find(Some("tmux"), Some(dir.path().as_os_str())).unwrap();
    let error = place.live_panes(dir.path()).await.unwrap_err().to_string();
    assert!(error.contains("protocol version mismatch"), "{error}");
    std::fs::write(
        &binary,
        "#!/bin/sh\necho 'server exited unexpectedly' >&2\nexit 1\n",
    )
    .unwrap();
    let error = place.live_panes(dir.path()).await.unwrap_err().to_string();
    assert!(error.contains("server exited unexpectedly"), "{error}");
    // A server that closes with its last session fails one call, then is gone.
    let count = dir.path().join("count");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nif [ -e {0} ]; then echo 'no server running' >&2; exit 1; fi\ntouch {0}\necho 'server exited unexpectedly' >&2\nexit 1\n",
            count.display()
        ),
    )
    .unwrap();
    let warnings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = warnings.clone();
    let guard = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || WarningCapture(captured.clone()))
            .finish(),
    );
    assert!(place.live_panes(dir.path()).await.unwrap().is_empty());
    drop(guard);
    let output = String::from_utf8(warnings.lock().unwrap().clone()).unwrap();
    assert_eq!(output.matches("WARN").count(), 1, "{output}");
    assert!(output.contains("the tmux server exited while"), "{output}");
    std::fs::remove_file(&binary).unwrap();
    std::fs::write(&binary, [0u8; 8]).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = place.live_panes(dir.path()).await.unwrap_err().to_string();
    assert!(error.contains("listing tmux relay sessions"), "{error}");
    assert!(error.contains("tmux"), "{error}");
}

#[tokio::test]
async fn reconnect_resends_only_unaccepted_inputs_and_restores_pending_output() {
    use crate::common::{TIMEOUT, eventually};
    use ariadne_daemon::{place::PaneLaunch, relay_link::RelayLink};
    use ariadne_store::{NewSession, Store};
    let home = PaneHome(tempfile::tempdir_in("/tmp").unwrap());
    let database = home.0.path().join("store.db");
    let store = Store::open(&database).await.unwrap();
    let session = store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: "stub:model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    let place = Place::find(Some("tmux"), std::env::var_os("PATH").as_deref()).unwrap();
    let args = vec![
        "-u".into(),
        "-c".into(),
        "import sys; [print(line.strip(), flush=True) for line in sys.stdin]".into(),
    ];
    let pane = place
        .start_pane(&PaneLaunch {
            home: home.0.path(),
            cwd: home.0.path(),
            session_id: &session.id,
            launch_id: "link-test",
            agent_id: "stub",
            task_id: "task",
            daemon: std::path::Path::new(env!("CARGO_BIN_EXE_ariadned")),
            program: "python3",
            args: &args,
            env: &[],
        })
        .await
        .unwrap();
    eventually(TIMEOUT, "relay socket", || async {
        home.0
            .path()
            .join(format!("relays/{}.sock", session.id))
            .exists()
    })
    .await;
    assert_eq!(
        store
            .enqueue_relay_input(&session.id, "link-test", "permission-answer")
            .await
            .unwrap(),
        1
    );
    let mut link = RelayLink::connect(store.clone(), home.0.path(), &session.id, "link-test")
        .await
        .unwrap();
    let first = link.receive().await.unwrap();
    assert_eq!(first["data"], "permission-answer");
    drop(link);
    store.close().await;
    let store = Store::open(&database).await.unwrap();
    let mut link = RelayLink::connect(store, home.0.path(), &session.id, "link-test")
        .await
        .unwrap();
    assert_eq!(link.receive().await.unwrap(), first);
    link.processed(1).await.unwrap();
    assert_eq!(link.send("next-input").await.unwrap(), 2);
    let second = link.receive().await.unwrap();
    assert_eq!(second["seq"], 2);
    assert_eq!(second["data"], "next-input");
    place.stop_pane(home.0.path(), &pane).await.unwrap();
}

#[tokio::test]
async fn a_relay_pane_preserves_arguments_environment_and_working_directory() {
    use crate::common::{TIMEOUT, eventually};
    use ariadne_daemon::place::PaneLaunch;
    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let home = PaneHome(tempfile::tempdir_in("/tmp").unwrap());
    let place = Place::find(Some("tmux"), std::env::var_os("PATH").as_deref()).unwrap();
    let args = vec!["-u".into(), "-c".into(),
        "import sys,os,json; print(json.dumps([sys.argv[1:],os.environ['PANE_VALUE'],os.getcwd()]),flush=True); sys.stdin.read()".into(),
        "space value".into(), "trailing;".into(), "\\;".into(), ";".into(), "$(echo should-stay-literal)".into(), "'quoted'".into()];
    let pane = place
        .start_pane(&PaneLaunch {
            home: home.0.path(),
            cwd: home.0.path(),
            session_id: "argv-test",
            launch_id: "argv-launch",
            agent_id: "stub",
            task_id: "task",
            daemon: std::path::Path::new(env!("CARGO_BIN_EXE_ariadned")),
            program: "python3",
            args: &args,
            env: &[("PANE_VALUE".into(), "literal $HOME ; value".into())],
        })
        .await
        .unwrap();
    let path = home.0.path().join("relays/argv-test.sock");
    eventually(TIMEOUT, "argument test relay socket", || async {
        path.exists()
    })
    .await;
    let mut socket = BufReader::new(tokio::net::UnixStream::connect(path).await.unwrap());
    let mut line = String::new();
    socket.read_line(&mut line).await.unwrap();
    socket
        .get_mut()
        .write_all(b"{\"type\":\"replay\",\"after\":0}\n")
        .await
        .unwrap();
    let actual = tokio::time::timeout(TIMEOUT, async {
        loop {
            line.clear();
            assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
            let frame: Value = serde_json::from_str(&line).unwrap();
            if frame["stream"] == "stdout" {
                break serde_json::from_str::<Value>(frame["data"].as_str().unwrap()).unwrap();
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        actual,
        json!([
            [
                "space value",
                "trailing;",
                "\\;",
                ";",
                "$(echo should-stay-literal)",
                "'quoted'"
            ],
            "literal $HOME ; value",
            home.0.path().canonicalize().unwrap()
        ])
    );
    drop(socket);
    place.stop_pane(home.0.path(), &pane).await.unwrap();
}

/// Run the real pane lifecycle with an isolated, non-UTF-8 client locale.
#[tokio::test]
async fn a_detached_relay_pane_is_discoverable_in_the_c_locale() {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "place::a_detached_relay_pane_can_be_found_and_stopped",
            "--nocapture",
        ])
        .env("LC_ALL", "C")
        .kill_on_drop(true);
    let output = tokio::time::timeout(crate::common::TIMEOUT, command.output())
        .await
        .unwrap()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("1 passed"),
        "the pane lifecycle test did not run: {stdout}"
    );
}
