//! The SSH forward of each `ssh://` host, against a stub `ssh` (037).
//!
//! The stub relays its `-L` port to its remote as `ssh` does, so a second
//! harness daemon served on a socket stands in for the far machine.

use std::path::PathBuf;

use crate::common::acp::pid_is_alive;
use crate::common::ssh::{StubSsh, stub_ssh};
use crate::common::{Harness, RUNS_OUT, TIMEOUT, eventually, harness, next_event, roomy_hardware};
use ariadne_api::hosts::{HostDto, HostStatus, SyncState};
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
    std::fs::write(
        home.join(if toml.contains("[hosts.") {
            "host-fixture.toml"
        } else {
            "config.toml"
        }),
        toml,
    )
    .unwrap();
    (dir, home)
}

/// A daemon whose one host `box` is `ssh://me@box:2222` at `remote`, on
/// `stub`, with its forward restarted after `RUNS_OUT`.
async fn local(stub: &StubSsh, remote: Option<&str>) -> (Harness, tempfile::TempDir) {
    local_with(stub, remote, Timeouts::default()).await
}

/// `local`, with `timeouts` and its forward restarted after `RUNS_OUT`.
async fn local_with(
    stub: &StubSsh,
    remote: Option<&str>,
    timeouts: Timeouts,
) -> (Harness, tempfile::TempDir) {
    let remote = remote.map_or(String::new(), |remote| format!("remote = {remote:?}\n"));
    let (dir, home) = home_with(&format!(
        "[hosts.box]\nendpoint = \"ssh://me@box:2222\"\n{remote}"
    ));
    let h = harness()
        .ai_permissions_hardware(roomy_hardware())
        .home(home)
        .ssh(stub)
        .timeouts(Timeouts {
            ssh_retry: RUNS_OUT,
            ..timeouts
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

/// The `host_updated` events of a forward, read off the bus.
struct Forward {
    rx: Receiver<BusEvent>,
    /// The status, endpoint and error of the last one read.
    last: Option<(HostStatus, Option<String>, Option<String>)>,
}

impl Forward {
    fn new(rx: Receiver<BusEvent>) -> Self {
        Self { rx, last: None }
    }

    /// The next `host_updated` that moves the forward's status, endpoint or
    /// error. One that moves only the host's sync is skipped.
    async fn next(&mut self) -> HostDto {
        loop {
            let event = next_event(&mut self.rx, |event| {
                matches!(event.event, DomainEvent::HostUpdated(_))
            })
            .await;
            let DomainEvent::HostUpdated(host) = event.event else {
                unreachable!()
            };
            let now = Some((host.status, host.endpoint.clone(), host.error.clone()));
            if now != self.last {
                self.last = now;
                return host;
            }
        }
    }
}

/// `listed` is `host`, whatever its sync has moved to since.
fn same_forward(listed: &HostDto, host: &HostDto) -> bool {
    HostDto {
        color: None,
        sync: host.sync.clone(),
        ..listed.clone()
    } == *host
}

/// The port of the `-L` argument of `args`.
fn forwarded_port(args: &[String]) -> String {
    let at = args.iter().position(|arg| arg == "-L").unwrap();
    args[at + 1].split(':').nth(1).unwrap().to_string()
}

/// The ssh call that asks the far machine for its user's `$HOME`.
fn home_call() -> Vec<String> {
    [
        "-o",
        "BatchMode=yes",
        "-p",
        "2222",
        "me@box",
        "printf %s \"$HOME\"",
    ]
    .map(String::from)
    .into()
}

#[tokio::test]
async fn a_host_with_no_remote_forwards_to_the_far_users_own_socket() {
    let stub = stub_ssh(json!({"home": "/home/malvarez", "stderr": "refused\n", "exit": 255}));
    let (h, _dir) = local(&stub, None).await;
    h.state.hosts.start();
    eventually(TIMEOUT, "the forward's ssh run", || async {
        stub.runs().len() >= 2
    })
    .await;
    let runs = stub.runs();
    assert_eq!(runs[0].args, home_call());
    let args = runs[1].args.clone();
    let port = forwarded_port(&args);
    assert_eq!(
        args,
        [
            "-N".to_string(),
            "-L".into(),
            format!("127.0.0.1:{port}:/home/malvarez/.ariadne/ariadne.sock"),
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
    // The stored host keeps no remote, and neither does its DTO.
    assert_eq!(h.store.list_hosts().await.unwrap()[0].remote, None);
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert_eq!(hosts[1].remote, None);
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn a_host_with_a_remote_forwards_to_it_and_asks_for_no_home() {
    let stub = stub_ssh(json!({"home": "/home/malvarez", "stderr": "refused\n", "exit": 255}));
    let (h, _dir) = local(&stub, Some("/run/far.sock")).await;
    h.state.hosts.start();
    eventually(TIMEOUT, "two forward runs", || async {
        stub.runs().len() >= 2
    })
    .await;
    for run in stub.runs() {
        let at = run.args.iter().position(|arg| arg == "-L").unwrap();
        assert!(run.args[at + 1].ends_with(":/run/far.sock"), "{run:?}");
    }
    h.state.hosts.shutdown().await;
}

/// The host `box` is `down` with `error` after one home call that `script`
/// scripts, and a second forward attempt follows.
async fn a_failed_home_lookup_is_down(script: serde_json::Value, error: &str) {
    let stub = stub_ssh(script);
    let error = error.replace("{ssh}", &stub.ssh);
    let (h, _dir) = local_with(
        &stub,
        None,
        Timeouts {
            ssh_home: RUNS_OUT,
            ..Timeouts::default()
        },
    )
    .await;
    let mut rx = Forward::new(h.bus.subscribe());
    h.state.hosts.start();
    assert_eq!(rx.next().await.status, HostStatus::Connecting);
    let down = rx.next().await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint, None);
    assert_eq!(down.error.as_deref(), Some(error.as_str()));
    assert_eq!(rx.next().await.status, HostStatus::Connecting);
    eventually(TIMEOUT, "a second home call", || async {
        stub.runs().len() == 2
    })
    .await;
    assert!(stub.runs().iter().all(|run| run.args == home_call()));
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn a_home_call_that_fails_makes_the_host_down_with_its_last_stderr_line() {
    a_failed_home_lookup_is_down(
        json!({"home": "", "home_exit": 255, "home_stderr": "debug1: x\nssh: Could not resolve hostname box\n"}),
        "the far home lookup failed: {ssh} exited: exit status: 255: ssh: Could not resolve hostname box",
    )
    .await;
}

/// A last stderr line longer than any buffer of the daemon.
fn long_line() -> String {
    format!("ssh: {}", "x".repeat(20_000))
}

#[tokio::test]
async fn a_home_call_that_exits_keeps_a_long_last_stderr_line_whole() {
    let line = long_line();
    a_failed_home_lookup_is_down(
        json!({"home": "", "home_exit": 255, "home_stderr": format!("first\n{line}\n")}),
        &format!("the far home lookup failed: {{ssh}} exited: exit status: 255: {line}"),
    )
    .await;
}

#[tokio::test]
async fn a_home_call_past_its_bound_keeps_a_long_unended_stderr_line_whole() {
    let line = long_line();
    let stub = stub_ssh(json!({"home_hang": true, "home_stderr": format!("first\n{line}")}));
    let (h, _dir) = local_with(
        &stub,
        None,
        Timeouts {
            ssh_home: std::time::Duration::from_millis(300),
            ..Timeouts::default()
        },
    )
    .await;
    let mut rx = Forward::new(h.bus.subscribe());
    h.state.hosts.start();
    assert_eq!(rx.next().await.status, HostStatus::Connecting);
    let error = rx.next().await.error.unwrap();
    assert!(error.ends_with(&format!(": {line}")), "{}", &error[..80]);
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn an_empty_home_makes_the_host_down() {
    a_failed_home_lookup_is_down(
        json!({"home": ""}),
        "the far home lookup failed: the far $HOME is empty",
    )
    .await;
}

#[tokio::test]
async fn a_relative_home_makes_the_host_down() {
    a_failed_home_lookup_is_down(
        json!({"home": "home/far", "home_stderr": "motd\n"}),
        "the far home lookup failed: the far $HOME \"home/far\" is not an absolute path: motd",
    )
    .await;
}

#[tokio::test]
async fn a_home_call_past_its_bound_is_down_and_its_process_ends() {
    let stub = stub_ssh(json!({
        "home_hang": true,
        "home_stderr": "debug1: x\nssh: connect to host box port 2222: Operation timed out",
    }));
    let (h, _dir) = local_with(
        &stub,
        None,
        Timeouts {
            ssh_home: std::time::Duration::from_millis(300),
            ..Timeouts::default()
        },
    )
    .await;
    let mut rx = Forward::new(h.bus.subscribe());
    h.state.hosts.start();
    assert_eq!(rx.next().await.status, HostStatus::Connecting);
    let down = rx.next().await;
    assert_eq!(down.status, HostStatus::Down);
    let error = down.error.unwrap();
    assert!(
        error.starts_with("the far home lookup failed: no answer in"),
        "{error}"
    );
    assert!(
        error.ends_with(": ssh: connect to host box port 2222: Operation timed out"),
        "{error}"
    );
    let pid = stub.runs()[0].pid;
    eventually(TIMEOUT, "the home call's process to end", || async {
        !pid_is_alive(pid)
    })
    .await;
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn a_reconnect_ends_a_home_call_under_way() {
    let stub = stub_ssh(json!({"home_hang": true}));
    let (h, _dir) = local(&stub, None).await;
    h.state.hosts.start();
    eventually(TIMEOUT, "the home call", || async {
        !stub.runs().is_empty()
    })
    .await;
    let pid = stub.runs()[0].pid;
    let _ = h.state.hosts.reconnect("box").await;
    eventually(TIMEOUT, "the first call to end", || async {
        !pid_is_alive(pid)
    })
    .await;
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn shutdown_ends_a_home_call_under_way() {
    let stub = stub_ssh(json!({"home_hang": true}));
    let (h, _dir) = local(&stub, None).await;
    h.state.hosts.start();
    eventually(TIMEOUT, "the home call", || async {
        !stub.runs().is_empty()
    })
    .await;
    let pid = stub.runs()[0].pid;
    assert!(pid_is_alive(pid));
    // Shutdown aborts the host's task, which drops the call's future.
    h.state.hosts.shutdown().await;
    eventually(TIMEOUT, "the home call's process to end", || async {
        !pid_is_alive(pid)
    })
    .await;
}

#[tokio::test]
async fn an_unanswered_forward_names_the_far_daemon_and_ssh_last_line() {
    let stub = stub_ssh(json!({
        "forward": true,
        "connect_stderr": "channel 2: open failed: connect failed: No such file or directory\n",
    }));
    let (h, _dir) = local_with(
        &stub,
        Some("/nowhere/far.sock"),
        Timeouts {
            host_unanswered: std::time::Duration::from_millis(500),
            ..Timeouts::default()
        },
    )
    .await;
    let mut rx = Forward::new(h.bus.subscribe());
    h.state.hosts.start();
    assert_eq!(rx.next().await.status, HostStatus::Connecting);
    let down = rx.next().await;
    assert_eq!(down.status, HostStatus::Down);
    let error = down.error.unwrap();
    assert!(
        error.starts_with("the far daemon does not answer through the forward: "),
        "{error}"
    );
    assert!(
        error.ends_with(": channel 2: open failed: connect failed: No such file or directory"),
        "{error}"
    );
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn an_exited_forward_is_down_with_its_last_stderr_line_and_starts_again() {
    let stub = stub_ssh(json!({
        "stderr": "debug1: Reading configuration data\nssh: connect to host box port 2222: Connection refused\n",
        "exit": 255,
    }));
    let (h, _dir) = local(&stub, Some("/run/far.sock")).await;
    let mut rx = Forward::new(h.bus.subscribe());
    h.state.hosts.start();

    let connecting = rx.next().await;
    assert_eq!(connecting.name, "box");
    assert_eq!(connecting.status, HostStatus::Connecting);
    assert_eq!(connecting.endpoint, None);
    assert_eq!(connecting.error, None);

    let down = rx.next().await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint, None);
    assert_eq!(
        down.error.as_deref(),
        Some("ssh: connect to host box port 2222: Connection refused")
    );
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert!(same_forward(&hosts[1], &down), "{hosts:?}");

    let again = rx.next().await;
    assert_eq!(again.status, HostStatus::Connecting);
    eventually(TIMEOUT, "a second ssh run", || async {
        stub.runs().len() == 2
    })
    .await;
}

#[tokio::test]
async fn a_signalled_forward_reports_ssh_stderr_without_guard_diagnostics() {
    let stub = stub_ssh(json!({
        "stderr": "Connection to box closed by remote host.\n",
        "signal": 15,
    }));
    let (h, _dir) = local(&stub, Some("/run/far.sock")).await;
    let mut rx = Forward::new(h.bus.subscribe());
    h.state.hosts.start();
    assert_eq!(rx.next().await.status, HostStatus::Connecting);
    let down = rx.next().await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(
        down.error.as_deref(),
        Some("Connection to box closed by remote host.")
    );
    assert!(!pid_is_alive(stub.runs()[0].pid));
    h.state.hosts.shutdown().await;
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
    let mut rx = Forward::new(h.bus.subscribe());
    h.state.hosts.start();

    assert_eq!(rx.next().await.status, HostStatus::Connecting);
    let ready = rx.next().await;
    let port = forwarded_port(&stub.runs()[0].args);
    assert_eq!(ready.status, HostStatus::Ready);
    assert_eq!(ready.endpoint, Some(format!("http://127.0.0.1:{port}")));
    assert_eq!(ready.error, None);
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    assert!(same_forward(&hosts[1], &ready), "{hosts:?}");

    // The far daemon takes the local catalogs through the forward.
    eventually(TIMEOUT, "the sync in sync", || async {
        let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
        hosts[1].sync.state == SyncState::InSync
    })
    .await;

    std::fs::write(&gone, "").unwrap();
    let down = rx.next().await;
    assert_eq!(down.status, HostStatus::Down);
    assert_eq!(down.endpoint, None);
    assert_eq!(
        down.error.as_deref(),
        Some("Connection to box closed by remote host.")
    );
    // What is pushed next waits for the forward to be up again.
    eventually(TIMEOUT, "the sync pending", || async {
        let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
        hosts[1].sync.state == SyncState::Pending
    })
    .await;
    assert_eq!(rx.next().await.status, HostStatus::Connecting);
}

#[tokio::test]
async fn a_forward_to_a_socket_path_comes_up_and_goes_down() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let (socket, _dir) = serve_on_socket(&far).await;
    a_forward_comes_up_and_goes_down(&socket).await;
}

#[tokio::test]
async fn a_forward_to_a_tcp_port_comes_up_and_goes_down() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
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
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
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
    let h = harness()
        .ai_permissions_hardware(roomy_hardware())
        .home(home)
        .ssh(&stub)
        .await;
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
    assert!(!help.contains("[hosts"), "{help}");

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

/// A device set on the far daemon stays when its host leaves `ready` and
/// becomes `ready` again, and the full push that follows sends the flavour
/// without it. Local's own best device is `mlx`, the far daemon's is set
/// to `cpu`.
#[tokio::test]
async fn a_device_set_on_the_host_stays_when_the_host_becomes_ready_again() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let (socket, _socket_dir) = serve_on_socket(&far).await;
    let until = tempfile::tempdir().unwrap();
    let gone = until.path().join("gone");
    let script = |until: &std::path::Path| json!({"forward": true, "until": until.display().to_string(), "exit": 255});
    let stub = stub_ssh(script(&gone));
    let (local, _dir) = local(&stub, Some(&socket)).await;
    let _: ariadne_api::permissions::AiPermissionsStatusDto = far
        .json(
            crate::common::put_json("/v1/permissions/ai", json!({"device": "cpu"})),
            axum::http::StatusCode::OK,
        )
        .await;
    local.state.hosts.start();
    let host = &local;
    let box_is = |status: HostStatus, sync: SyncState| async move {
        let hosts: Vec<HostDto> = host.get("/v1/hosts").await;
        hosts[1].status == status && hosts[1].sync.state == sync
    };
    eventually(TIMEOUT, "the host ready and in sync", || {
        box_is(HostStatus::Ready, SyncState::InSync)
    })
    .await;
    let ai: ariadne_api::permissions::AiPermissionsStatusDto = far.get("/v1/permissions/ai").await;
    assert_eq!(ai.device.as_str(), "cpu", "the first push keeps the device");

    // The next run of the stub ends only on a file nobody writes.
    let never = until.path().join("never");
    let dir = std::path::Path::new(&stub.ssh).parent().unwrap();
    std::fs::write(dir.join("ssh-script.json"), script(&never).to_string()).unwrap();
    std::fs::write(&gone, "").unwrap();
    eventually(TIMEOUT, "the host left ready", || async {
        let hosts: Vec<HostDto> = local.get("/v1/hosts").await;
        hosts[1].status != HostStatus::Ready && hosts[1].sync.state == SyncState::Pending
    })
    .await;
    eventually(TIMEOUT, "the host ready again and in sync", || async {
        stub.runs().len() == 2 && box_is(HostStatus::Ready, SyncState::InSync).await
    })
    .await;
    let ai: ariadne_api::permissions::AiPermissionsStatusDto = far.get("/v1/permissions/ai").await;
    assert_eq!(ai.device.as_str(), "cpu", "the full push keeps the device");
}

#[tokio::test]
async fn deleting_a_host_ends_its_forward() {
    let stub = stub_ssh(json!({"forward": true}));
    let (h, _dir) = local(&stub, Some("/run/worker.sock")).await;
    h.state.hosts.start();
    eventually(TIMEOUT, "forward starts", || async {
        !stub.runs().is_empty()
    })
    .await;
    let pid = stub.runs()[0].pid;
    let (status, body) = h.send(crate::common::delete("/v1/hosts/box")).await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    assert!(body.is_empty());
    eventually(TIMEOUT, "deleted forward ends", || async {
        !pid_is_alive(pid)
    })
    .await;
    assert_eq!(h.store.list_hosts().await.unwrap().len(), 0);
    h.state.hosts.shutdown().await;
}

#[tokio::test]
async fn changing_a_remote_replaces_the_forward_and_keeps_its_colour() {
    let far = harness().ai_permissions_hardware(roomy_hardware()).await;
    let (first, _first_dir) = serve_on_socket(&far).await;
    let (second, _second_dir) = serve_on_socket(&far).await;
    let stub = stub_ssh(json!({"forward":true}));
    let (h, _dir) = local(&stub, Some(&first)).await;
    h.state.hosts.start();
    eventually(TIMEOUT, "first forward ready", || async {
        h.state.hosts.list()[1].status == HostStatus::Ready
    })
    .await;
    let pid = stub.runs()[0].pid;
    let coloured: HostDto = h
        .json(
            crate::common::patch_json("/v1/hosts/box", json!({"color":"violet"})),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(coloured.status, HostStatus::Ready);
    assert!(pid_is_alive(pid));
    assert_eq!(stub.runs().len(), 1);
    let changed: HostDto = h
        .json(
            crate::common::patch_json("/v1/hosts/box", json!({"remote":second})),
            axum::http::StatusCode::OK,
        )
        .await;
    assert_eq!(changed.color, Some(ariadne_api::hosts::HostColor::Violet));
    assert_eq!(changed.remote.as_deref(), Some(second.as_str()));
    eventually(TIMEOUT, "replacement forward ready", || async {
        stub.runs().len() == 2
            && !pid_is_alive(pid)
            && h.state.hosts.list()[1].status == HostStatus::Ready
    })
    .await;
    assert_eq!(
        h.store.list_hosts().await.unwrap()[0].remote.as_deref(),
        Some(second.as_str())
    );
    h.state.hosts.shutdown().await;
}
