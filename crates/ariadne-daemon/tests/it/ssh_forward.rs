//! The SSH forward of each `ssh://` host, against a stub `ssh` (037).
//!
//! The stub relays its `-L` port to its remote as `ssh` does, so a second
//! harness daemon served on a socket stands in for the far machine.

use std::path::PathBuf;

use crate::common::acp::pid_is_alive;
use crate::common::ssh::{StubSsh, stub_ssh};
use crate::common::{Harness, RUNS_OUT, TIMEOUT, eventually, harness, next_event};
use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_api::stream::{DeletedDto, DomainEvent};
use ariadne_daemon::bus::BusEvent;
use ariadne_daemon::timeouts::Timeouts;
use serde_json::json;
use tokio::sync::broadcast::Receiver;

/// A home whose `config.toml` says `toml`.
fn home_with(toml: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("config.toml"), toml).unwrap();
    (dir, home)
}

/// A daemon whose one host `box` is `ssh://me@box:2222` at `remote`, on
/// `stub`, with its forward restarted after `RUNS_OUT`.
async fn local(stub: &StubSsh, remote: Option<&str>) -> (Harness, tempfile::TempDir) {
    let remote = remote.map_or(String::new(), |remote| format!("remote = {remote:?}\n"));
    let (dir, home) = home_with(&format!(
        "[hosts.box]\nendpoint = \"ssh://me@box:2222\"\n{remote}"
    ));
    let h = harness()
        .home(home)
        .ssh(stub)
        .timeouts(Timeouts {
            ssh_retry: RUNS_OUT,
            ..Timeouts::default()
        })
        .await;
    (h, dir)
}

/// The far daemon: `remote`'s router served on a unix socket of its own.
async fn serve_on_socket(remote: &Harness) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("far.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let router = remote.router.clone();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (path.display().to_string(), dir)
}

/// The far daemon on a loopback TCP port.
async fn serve_on_tcp(remote: &Harness) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let router = remote.router.clone();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    address
}

/// The next `host_updated` on the bus.
async fn next_host(rx: &mut Receiver<BusEvent>) -> HostDto {
    let event = next_event(rx, |event| {
        matches!(event.event, DomainEvent::HostUpdated(_))
    })
    .await;
    match event.event {
        DomainEvent::HostUpdated(host) => host,
        _ => unreachable!(),
    }
}

/// The port of the `-L` argument of `args`.
fn forwarded_port(args: &[String]) -> String {
    let at = args.iter().position(|arg| arg == "-L").unwrap();
    args[at + 1].split(':').nth(1).unwrap().to_string()
}

#[tokio::test]
async fn a_forward_runs_ssh_to_the_local_socket_path_by_default() {
    let stub = stub_ssh(json!({"stderr": "refused\n", "exit": 255}));
    let (h, _dir) = local(&stub, None).await;
    h.state.hosts.start();
    eventually(TIMEOUT, "the first ssh run", || async {
        !stub.runs().is_empty()
    })
    .await;
    let args = stub.runs()[0].args.clone();
    let port = forwarded_port(&args);
    let socket = h.launcher.cfg.socket_path.display().to_string();
    assert_eq!(
        args,
        [
            "-N".to_string(),
            "-L".into(),
            format!("127.0.0.1:{port}:{socket}"),
            "-o".into(),
            "ServerAliveInterval=15".into(),
            "-o".into(),
            "ServerAliveCountMax=3".into(),
            "-o".into(),
            "ExitOnForwardFailure=yes".into(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-p".into(),
            "2222".into(),
            "me@box".into(),
        ]
    );
}

#[tokio::test]
async fn an_exited_forward_is_down_with_its_last_stderr_line_and_starts_again() {
    let stub = stub_ssh(json!({
        "stderr": "debug1: Reading configuration data\nssh: connect to host box port 2222: Connection refused\n",
        "exit": 255,
    }));
    let (h, _dir) = local(&stub, Some("/run/far.sock")).await;
    let mut rx = h.bus.subscribe();
    h.state.hosts.start();

    let connecting = next_host(&mut rx).await;
    assert_eq!(connecting.name, "box");
    assert_eq!(connecting.status, HostStatus::Connecting);
    assert_eq!(connecting.endpoint, None);
    assert_eq!(connecting.error, None);

    let down = next_host(&mut rx).await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint, None);
    assert_eq!(
        down.error.as_deref(),
        Some("ssh: connect to host box port 2222: Connection refused")
    );
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(hosts[1], down);

    let again = next_host(&mut rx).await;
    assert_eq!(again.status, HostStatus::Connecting);
    eventually(TIMEOUT, "a second ssh run", || async {
        stub.runs().len() == 2
    })
    .await;
}

/// A forward goes `connecting`, then `ready` at its port once the far daemon
/// answers through it, then `down` when the process exits: one event each.
async fn a_forward_comes_up_and_goes_down(remote: &str) {
    let until = tempfile::tempdir().unwrap();
    let gone = until.path().join("gone");
    let stub = stub_ssh(json!({
        "forward": true,
        "until": gone.display().to_string(),
        "stderr": "Connection to box closed by remote host.\n",
        "exit": 255,
    }));
    let (h, _dir) = local(&stub, Some(remote)).await;
    let mut rx = h.bus.subscribe();
    h.state.hosts.start();

    assert_eq!(next_host(&mut rx).await.status, HostStatus::Connecting);
    let ready = next_host(&mut rx).await;
    let port = forwarded_port(&stub.runs()[0].args);
    assert_eq!(ready.status, HostStatus::Ready);
    assert_eq!(ready.endpoint, Some(format!("http://127.0.0.1:{port}")));
    assert_eq!(ready.error, None);
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(hosts[1], ready);

    std::fs::write(&gone, "").unwrap();
    let down = next_host(&mut rx).await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint, None);
    assert_eq!(
        down.error.as_deref(),
        Some("Connection to box closed by remote host.")
    );
    assert_eq!(next_host(&mut rx).await.status, HostStatus::Connecting);
}

#[tokio::test]
async fn a_forward_to_a_socket_path_comes_up_and_goes_down() {
    let far = harness().await;
    let (socket, _dir) = serve_on_socket(&far).await;
    a_forward_comes_up_and_goes_down(&socket).await;
}

#[tokio::test]
async fn a_forward_to_a_tcp_port_comes_up_and_goes_down() {
    let far = harness().await;
    let address = serve_on_tcp(&far).await;
    a_forward_comes_up_and_goes_down(&address).await;
}

/// The next chunk of `stream`, within `TIMEOUT`.
async fn next_chunk(stream: &mut reqwest::Response) -> String {
    let chunk = tokio::time::timeout(TIMEOUT, stream.chunk())
        .await
        .expect("a chunk in time")
        .unwrap()
        .expect("the stream stays open");
    String::from_utf8_lossy(&chunk).into_owned()
}

/// Read `stream` until it has carried `needle`.
async fn read_until(stream: &mut reqwest::Response, needle: &str) {
    let mut seen = String::new();
    while !seen.contains(needle) {
        seen.push_str(&next_chunk(stream).await);
    }
}

#[tokio::test]
async fn an_event_stream_and_a_post_both_cross_a_ready_forward() {
    let far = harness().await;
    let (socket, _dir) = serve_on_socket(&far).await;
    let stub = stub_ssh(json!({"forward": true}));
    let (h, _home) = local(&stub, Some(&socket)).await;
    h.state.hosts.start();
    let endpoint = crate::common::eventually_some(TIMEOUT, "the forward ready", || async {
        let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
        (hosts[1].status == HostStatus::Ready).then(|| hosts[1].endpoint.clone().unwrap())
    })
    .await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    // An event the far daemon publishes after the stream opened comes back.
    let mut stream = client
        .get(format!("{endpoint}/v1/events/stream"))
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status(), reqwest::StatusCode::OK);
    read_until(&mut stream, "event: heartbeat").await;
    far.bus.publish(BusEvent {
        event: DomainEvent::SkillDeleted(DeletedDto {
            id: "published-after-open".into(),
        }),
        goal_id: None,
        task_id: None,
        recorded: None,
    });
    read_until(&mut stream, "published-after-open").await;

    // A goal message posted through the forward lands on the far daemon.
    let (goal, _) = far.goal().await;
    let posted = client
        .post(format!("{endpoint}/v1/goals/{}/messages", goal.id))
        .header("content-type", "application/json")
        .body(json!({"to_actor": "orchestrator", "body": "Sent through ssh."}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(posted.status(), reqwest::StatusCode::CREATED);
    let messages: serde_json::Value = far.get(&format!("/v1/goals/{}/messages", goal.id)).await;
    assert!(
        messages.to_string().contains("Sent through ssh."),
        "{messages}"
    );
}

#[tokio::test]
async fn shutdown_ends_every_ssh_child() {
    let stub = stub_ssh(json!({"forward": true}));
    let (_dir, home) = home_with(
        "[hosts.one]\nendpoint = \"ssh://one\"\nremote = \"/run/one.sock\"\n\
         [hosts.two]\nendpoint = \"ssh://two\"\nremote = \"/run/two.sock\"\n",
    );
    let h = harness().home(home).ssh(&stub).await;
    h.state.hosts.start();
    eventually(TIMEOUT, "both forwards running", || async {
        stub.runs().len() == 2
    })
    .await;
    let pids: Vec<u32> = stub.runs().iter().map(|run| run.pid).collect();
    assert!(pids.iter().all(|pid| pid_is_alive(*pid)));
    h.state.hosts.shutdown().await;
    eventually(TIMEOUT, "every ssh child to end", || async {
        pids.iter().all(|pid| !pid_is_alive(*pid))
    })
    .await;
}

#[test]
fn help_and_check_config_list_ssh_bin() {
    let help = std::process::Command::new(env!("CARGO_BIN_EXE_ariadned"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("ssh_bin"), "{help}");

    let (_dir, home) = home_with("");
    let default = ariadne_daemon::config::Config::check(Some(home)).unwrap();
    assert!(
        default.lines().any(|line| line == "ssh_bin = ssh"),
        "{default}"
    );
    let (_dir, home) = home_with("ssh_bin = \"/opt/bin/ssh\"\n");
    let set = ariadne_daemon::config::Config::check(Some(home)).unwrap();
    assert!(
        set.lines().any(|line| line == "ssh_bin = /opt/bin/ssh"),
        "{set}"
    );
}
