//! How a daemon stops, and how it refuses to start beside one that runs
//! (016): a bounded HTTP drain, the socket and pid files of the daemon that
//! came after it, and the databases of the daemon that still runs.

use axum::Router;
use axum::routing::get;

use crate::common::daemon::Daemon;
use crate::common::{RUNS_OUT, TIMEOUT, eventually, store_copy};

/// Whether a process with this pid still exists.
fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .status()
        .unwrap()
        .success()
}

#[tokio::test]
async fn a_request_that_never_finishes_does_not_hold_the_drain_past_its_bound() {
    let (entered, mut handled) = tokio::sync::mpsc::unbounded_channel::<()>();
    let app = Router::new().route(
        "/hang",
        get(move || {
            let entered = entered.clone();
            async move {
                let _ = entered.send(());
                std::future::pending::<()>().await;
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(ariadne_daemon::http::serve(
        listener,
        app,
        async move {
            let _ = stopped.await;
        },
        RUNS_OUT,
    ));
    let request = tokio::spawn(reqwest::get(format!("http://{address}/hang")));
    tokio::time::timeout(TIMEOUT, handled.recv())
        .await
        .expect("the request to reach its handler");
    stop.send(()).unwrap();
    let served = tokio::time::timeout(TIMEOUT, server)
        .await
        .expect("the server to stop with the request still open");
    assert!(served.unwrap().is_ok());
    request.abort();
}

#[tokio::test]
async fn a_stopping_daemon_leaves_the_socket_and_pid_file_of_the_one_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    store_copy(&home.join("ariadne.db")).await;
    let socket = ariadne_client::endpoint::socket_path(&home);
    // A new client for each read: a kept connection would still reach the
    // first daemon after its socket moved.
    let health = async || {
        ariadne_client::Client::for_home(Some(home.clone()))
            .health()
            .await
            .ok()
            .and_then(|h| h.pid)
    };

    let first = Daemon::spawn(&home, &[]);
    let mut first_pid = None;
    eventually(TIMEOUT, "the first daemon to answer", async || {
        first_pid = health().await;
        first_pid.is_some()
    })
    .await;
    let first = (first, first_pid.unwrap());
    let first_pid = first.1.to_string();
    assert_eq!(first.0.pid().as_deref(), Some(first_pid.as_str()));

    // The first daemon's socket moves aside, as a start that took it for
    // stale would leave it, and a second daemon binds the path again.
    std::fs::rename(&socket, dir.path().join("first.sock")).unwrap();
    let second = Daemon::spawn(&home, &[]);
    let mut second_pid = None;
    eventually(TIMEOUT, "the second daemon to answer", async || {
        second_pid = health().await.filter(|pid| *pid != first.1);
        second_pid.is_some()
    })
    .await;
    let second_pid = second_pid.unwrap().to_string();

    assert!(alive(&first_pid));
    std::process::Command::new("kill")
        .args(["-TERM", &first_pid])
        .status()
        .unwrap();
    eventually(TIMEOUT, "the first daemon to stop", async || {
        !alive(&first_pid)
    })
    .await;

    assert!(socket.exists(), "the second daemon's socket stays");
    assert_eq!(second.pid().as_deref(), Some(second_pid.as_str()));
    assert_eq!(health().await.map(|pid| pid.to_string()), Some(second_pid));
    drop(first);
}

#[tokio::test]
async fn a_start_beside_a_running_daemon_opens_no_database() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    // Something answers on the socket: the running daemon, as far as a new
    // one can tell.
    let _held =
        std::os::unix::net::UnixListener::bind(ariadne_client::endpoint::socket_path(&home))
            .unwrap();
    let mut start = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadned"))
        .arg("--home")
        .arg(&home)
        .env("PATH", "/usr/bin:/bin")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let status = tokio::time::timeout(TIMEOUT, start.wait())
        .await
        .expect("the refused start to exit")
        .unwrap();
    assert!(!status.success());
    let mut stderr = String::new();
    tokio::io::AsyncReadExt::read_to_string(&mut start.stderr.take().unwrap(), &mut stderr)
        .await
        .unwrap();
    assert!(
        stderr.contains("another ariadned is already listening"),
        "{stderr}"
    );
    assert!(!home.join("ariadne.db").exists(), "no database was opened");
    assert!(
        !home.join("telemetry.db").exists(),
        "no telemetry database was opened"
    );
}
