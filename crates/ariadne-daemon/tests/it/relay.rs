//! Exercise the relay through its executable and Unix socket.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};

struct Relay {
    child: Child,
    home: tempfile::TempDir,
    status: Option<BufReader<std::process::ChildStdout>>,
    agent_pid: Option<rustix::process::Pid>,
}
impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(pid) = self.agent_pid {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Relay {
    fn start(script: &str) -> Self {
        Self::start_with_binary(script, std::path::Path::new(env!("CARGO_BIN_EXE_ariadned")))
    }
    fn start_with_binary(script: &str, binary: &std::path::Path) -> Self {
        Self::start_with_output(script, binary, Stdio::piped())
    }
    fn start_with_output(script: &str, binary: &std::path::Path, output: Stdio) -> Self {
        let home = tempfile::tempdir().unwrap();
        let mut child = Command::new(binary)
            .args([
                "relay",
                "--home",
                home.path().to_str().unwrap(),
                "--session",
                "session1",
                "--agent",
                "stub",
                "--task",
                "task1",
                "--",
                "python3",
                "-u",
                "-c",
                script,
            ])
            .current_dir(home.path())
            .stdout(output)
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let status = child.stdout.take().map(BufReader::new);
        let mut relay = Self {
            child,
            home,
            status,
            agent_pid: None,
        };
        if relay.status.is_none() {
            return relay;
        }
        let mut line = String::new();
        assert!(
            relay.status.as_mut().unwrap().read_line(&mut line).unwrap() > 0,
            "relay prints readiness"
        );
        assert!(
            line.contains("session1")
                && line.contains("stub")
                && line.contains("task1")
                && line.contains("ariadne attach session1")
                && line.contains("activity=")
        );
        relay.agent_pid = line
            .split_whitespace()
            .find_map(|part| part.strip_prefix("pid="))
            .and_then(|pid| pid.parse().ok())
            .and_then(rustix::process::Pid::from_raw);
        relay
    }
    fn raw_connect(&self) -> BufReader<UnixStream> {
        let socket = UnixStream::connect(self.home.path().join("relays/session1.sock")).unwrap();
        socket
            .set_read_timeout(Some(crate::common::TIMEOUT))
            .unwrap();
        BufReader::new(socket)
    }
    fn connect(&self) -> BufReader<UnixStream> {
        let mut peer = self.raw_connect();
        let hello = receive(&mut peer);
        assert_eq!(hello["type"], "hello");
        assert_eq!(hello["protocol_version"], 1);
        assert!(hello["last_input_seq"].is_u64());
        peer
    }
    fn journal(&self) -> String {
        std::fs::read_to_string(self.home.path().join("relays/session1.jsonl")).unwrap()
    }
}
fn send(peer: &mut BufReader<UnixStream>, frame: Value) {
    writeln!(peer.get_mut(), "{frame}").unwrap();
}
fn receive(peer: &mut BufReader<UnixStream>) -> Value {
    let mut line = String::new();
    assert!(peer.read_line(&mut line).unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}
fn replay(peer: &mut BufReader<UnixStream>, after: u64) {
    send(peer, json!({"type":"replay","after":after}));
}
fn stop(peer: &mut BufReader<UnixStream>) {
    send(peer, json!({"type":"stop"}));
}

#[test]
fn journals_before_delivery_and_replays_only_unacknowledged_messages() {
    let mut relay = Relay::start(
        "import sys\nprint('{\"method\":\"first\"}')\nprint('{\"method\":\"permission\"}')\nfor line in sys.stdin: print(line.strip())",
    );
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    let first = receive(&mut peer);
    assert_eq!(first["seq"], 1);
    assert_eq!(first["data"], "{\"method\":\"first\"}");
    assert!(relay.journal().contains("first"));
    send(&mut peer, json!({"type":"ack","seq":1}));
    // A response after the ACK proves the ACK was processed before disconnect.
    send(
        &mut peer,
        json!({"type":"message","seq":1,"data":"{\"id\":99}"}),
    );
    assert_eq!(receive(&mut peer)["seq"], 2);
    assert_eq!(receive(&mut peer)["seq"], 3);
    drop(peer);
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    assert_eq!(receive(&mut peer)["seq"], 2);
    assert_eq!(receive(&mut peer)["data"], "{\"id\":99}");
    assert!(relay.journal().contains("permission"));
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
    assert!(!relay.home.path().join("relays/session1.sock").exists());
    assert!(!relay.home.path().join("relays/session1.jsonl").exists());
}

#[test]
fn disconnected_agent_finishes_and_next_connection_receives_output_and_exit() {
    let mut relay = Relay::start(
        "import sys\nprint('{\"method\":\"permission\"}')\nanswer=sys.stdin.readline()\nprint(answer.strip())\nsys.stderr.write('diagnostic\\n')\nsys.exit(7)",
    );
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    assert!(
        receive(&mut peer)["data"]
            .as_str()
            .unwrap()
            .contains("permission")
    );
    drop(peer);
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    assert!(
        receive(&mut peer)["data"]
            .as_str()
            .unwrap()
            .contains("permission")
    );
    send(
        &mut peer,
        json!({"type":"message","seq":1,"data":"{\"result\":\"allow\"}"}),
    );
    // The response proves that the accepted input reached the agent.
    while receive(&mut peer)["data"] != "{\"result\":\"allow\"}" {}
    drop(peer);
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    let mut frames = vec![];
    loop {
        let frame = receive(&mut peer);
        let exit = frame["type"] == "exit";
        frames.push(frame);
        if exit {
            break;
        }
    }
    assert!(
        frames
            .iter()
            .any(|f| f["stream"] == "stdout" && f["data"] == "{\"result\":\"allow\"}")
    );
    assert!(
        frames
            .iter()
            .any(|f| f["stream"] == "stderr" && f["data"] == "diagnostic")
    );
    let exit = frames.last().unwrap();
    assert_eq!(exit["code"], 7);
    assert!(relay.journal().contains("\"code\":7"));
    send(&mut peer, json!({"type":"ack","seq":exit["seq"]}));
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn offline_status_and_output_do_not_need_a_daemon() {
    let mut relay = Relay::start(
        "import sys\nfor i in range(200): print('{\"n\":%d}'%i)\nprint('done',file=sys.stderr)",
    );
    loop {
        let mut line = String::new();
        assert!(relay.status.as_mut().unwrap().read_line(&mut line).unwrap() > 0);
        if line.contains("state=exited") {
            break;
        }
    }
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    let mut count = 0;
    let mut done = false;
    while count < 200 || !done {
        let frame = receive(&mut peer);
        if frame["stream"] == "stdout" {
            count += 1;
        } else {
            done = frame["data"] == "done";
        }
    }
    assert_eq!(count, 200);
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn stop_kills_the_full_process_group() {
    let script = "import subprocess,sys\nsubprocess.Popen(['python3','-u','-c',\"import socket,sys; s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[1]); s.listen(); print('ready',flush=True); c,_=s.accept(); print('accepted',flush=True); c.recv(1)\",sys.argv[0]+'.child'])\nsys.stdin.readline()";
    let mut relay = Relay::start(script);
    // The fixture uses a private path instead of a shared executable or socket.
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    assert_eq!(receive(&mut peer)["data"], "ready");
    let socket = UnixStream::connect(relay.home.path().join("-c.child")).unwrap();
    // Killed before it accepts, the child's closing listener resets a
    // connection still in its backlog, which reads as an error and not as the
    // end of the stream this test is after.
    assert_eq!(receive(&mut peer)["data"], "accepted");
    socket
        .set_read_timeout(Some(crate::common::TIMEOUT))
        .unwrap();
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
    let mut byte = [0];
    assert_eq!(std::io::Read::read(&mut &socket, &mut byte).unwrap(), 0);
}

#[tokio::test]
async fn stop_remains_available_when_daemon_does_not_read_output() {
    let mut relay = Relay::start("import sys\nfor i in range(200): print('x'*8192)");
    loop {
        let mut line = String::new();
        assert!(relay.status.as_mut().unwrap().read_line(&mut line).unwrap() > 0);
        if line.contains("state=exited") {
            break;
        }
    }
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    assert_eq!(receive(&mut peer)["seq"], 1);
    stop(&mut peer);
    let status = tokio::time::timeout(crate::common::TIMEOUT, async {
        loop {
            if let Some(status) = relay.child.try_wait().unwrap() {
                return status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("stop must not wait for output consumption");
    assert!(status.success());
}

#[test]
fn reconnect_replaces_owner_and_invalid_ack_does_not_stop_agent() {
    let mut relay = Relay::start("import sys\nprint('ready')\nsys.stdin.readline()");
    let mut first = relay.connect();
    replay(&mut first, 0);
    assert_eq!(receive(&mut first)["data"], "ready");
    let mut second = relay.connect();
    replay(&mut second, 0);
    assert_eq!(receive(&mut second)["data"], "ready");
    let mut line = String::new();
    assert_eq!(first.read_line(&mut line).unwrap(), 0);
    send(&mut second, json!({"type":"ack","seq":99}));
    assert_eq!(second.read_line(&mut line).unwrap(), 0);
    let mut third = relay.connect();
    replay(&mut third, 0);
    assert_eq!(receive(&mut third)["data"], "ready");
    stop(&mut third);
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn session_paths_are_private_and_cannot_be_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let mut relay = Relay::start("import sys\nsys.stdin.readline()");
    assert_eq!(
        std::fs::metadata(relay.home.path().join("relays"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(relay.home.path().join("relays/session1.jsonl"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let args = [
        "relay",
        "--home",
        relay.home.path().to_str().unwrap(),
        "--session",
        "session1",
        "--agent",
        "stub",
        "--task",
        "task1",
        "--",
        "python3",
        "-c",
        "raise Exception('must not start')",
    ];
    let output = Command::new(env!("CARGO_BIN_EXE_ariadned"))
        .args(args)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let mut peer = relay.connect();
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
    let invalid = Command::new(env!("CARGO_BIN_EXE_ariadned"))
        .args([
            "relay",
            "--session",
            "../bad",
            "--agent",
            "stub",
            "--task",
            "task1",
            "--",
            "true",
        ])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    let usage = Command::new(env!("CARGO_BIN_EXE_ariadned"))
        .arg("relay")
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
}

#[test]
fn first_socket_frame_identifies_protocol_before_replay() {
    let mut relay = Relay::start("import sys\nsys.stdin.readline()");
    let mut peer = relay.raw_connect();
    assert_eq!(
        receive(&mut peer),
        json!({"type":"hello","protocol_version":1,"last_input_seq":0})
    );
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn executable_upgrade_preserves_running_relay_and_protocol() {
    // Hard links cannot cross filesystems, and the system temporary directory can be another one.
    let built = std::path::Path::new(env!("CARGO_BIN_EXE_ariadned"));
    let install = tempfile::tempdir_in(built.parent().unwrap()).unwrap();
    let binary = install.path().join("ariadned");
    std::fs::hard_link(built, &binary).unwrap();
    let mut relay = Relay::start_with_binary(
        "import sys\nfor line in sys.stdin: print(line.strip())",
        &binary,
    );
    let mut before = relay.connect();
    replay(&mut before, 0);
    send(
        &mut before,
        json!({"type":"message","seq":1,"data":"before-upgrade"}),
    );
    assert_eq!(receive(&mut before)["data"], "before-upgrade");
    // Replace the installed executable. The replacement cannot serve the old socket.
    let replacement = install.path().join("replacement");
    std::os::unix::fs::symlink("/usr/bin/false", &replacement).unwrap();
    std::fs::rename(replacement, &binary).unwrap();
    assert_eq!(Command::new(&binary).status().unwrap().code(), Some(1));
    drop(before);
    let mut after = relay.connect();
    replay(&mut after, 0);
    assert_eq!(receive(&mut after)["data"], "before-upgrade");
    send(
        &mut after,
        json!({"type":"message","seq":2,"data":"after-upgrade"}),
    );
    assert_eq!(receive(&mut after)["data"], "after-upgrade");
    stop(&mut after);
    assert!(relay.child.wait().unwrap().success());
}

#[tokio::test]
async fn a_closed_status_pipe_does_not_stop_the_relay() {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    status_pipe_keeps_transport_running(writer).await;
}

#[tokio::test]
async fn a_full_status_pipe_does_not_stop_the_relay() {
    let (_reader, mut writer) = std::io::pipe().unwrap();
    let flags = rustix::fs::fcntl_getfl(&writer).unwrap();
    rustix::fs::fcntl_setfl(&writer, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
    loop {
        match writer.write(&[b'x'; 8192]) {
            Ok(count) => assert!(count > 0),
            Err(error) => {
                assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
                break;
            }
        }
    }
    rustix::fs::fcntl_setfl(&writer, flags).unwrap();
    status_pipe_keeps_transport_running(writer).await;
}

async fn status_pipe_keeps_transport_running(writer: std::io::PipeWriter) {
    let output = Stdio::from(writer);
    let mut relay = Relay::start_with_output(
        "import sys\nfor line in sys.stdin: print(line.strip())",
        std::path::Path::new(env!("CARGO_BIN_EXE_ariadned")),
        output,
    );
    let socket = crate::common::eventually_some(
        crate::common::TIMEOUT,
        "relay accepts a connection",
        || async { UnixStream::connect(relay.home.path().join("relays/session1.sock")).ok() },
    )
    .await;
    socket
        .set_read_timeout(Some(crate::common::TIMEOUT))
        .unwrap();
    let mut peer = BufReader::new(socket);
    let hello = receive(&mut peer);
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["protocol_version"], 1);
    assert!(hello["last_input_seq"].is_u64());
    replay(&mut peer, 0);
    send(
        &mut peer,
        json!({"type":"message","seq":1,"data":"status-barrier"}),
    );
    assert_eq!(receive(&mut peer)["data"], "status-barrier");
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn immediate_replacement_preserves_durable_input_and_drops_duplicates() {
    let mut relay = Relay::start("import sys\nfor line in sys.stdin: print(line.strip())");
    let mut peer = relay.connect();
    for seq in 1..=40 {
        replay(&mut peer, 0);
        send(
            &mut peer,
            json!({"type":"message","seq":seq,"data":format!("input-{seq}")}),
        );
        // Replace immediately. No response or journal wait precedes this connection.
        peer = relay.raw_connect();
        assert_eq!(
            receive(&mut peer),
            json!({"type":"hello","protocol_version":1,"last_input_seq":seq})
        );
        let records: Vec<Value> = relay
            .journal()
            .split_inclusive('\n')
            .filter(|line| line.ends_with('\n'))
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(
            records
                .iter()
                .any(|r| r == &json!({"type":"input","seq":seq}))
        );
    }
    replay(&mut peer, 0);
    send(
        &mut peer,
        json!({"type":"message","seq":40,"data":"duplicate"}),
    );
    send(
        &mut peer,
        json!({"type":"message","seq":10,"data":"older-duplicate"}),
    );
    send(
        &mut peer,
        json!({"type":"message","seq":41,"data":"barrier"}),
    );
    for seq in 1..=40 {
        assert_eq!(receive(&mut peer)["data"], format!("input-{seq}"));
    }
    assert_eq!(receive(&mut peer)["data"], "barrier");
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn rejected_input_is_not_reported_and_can_be_retried() {
    let mut relay = Relay::start(
        "import socket,sys\ns=socket.socket(socket.AF_UNIX); s.bind('gate'); s.listen()\nprint('ready')\nc,_=s.accept(); c.recv(1)\nfor line in sys.stdin: print(line.strip())",
    );
    let mut peer = relay.connect();
    peer.get_mut()
        .set_write_timeout(Some(crate::common::TIMEOUT))
        .unwrap();
    replay(&mut peer, 0);
    assert_eq!(receive(&mut peer)["data"], "ready");
    for seq in 1..=100 {
        if writeln!(
            peer.get_mut(),
            "{}",
            json!({"type":"message","seq":seq,"data":"x".repeat(8192)})
        )
        .is_err()
        {
            break;
        }
    }
    let mut peer = relay.raw_connect();
    let hello = receive(&mut peer);
    let accepted = hello["last_input_seq"].as_u64().unwrap();
    assert!(accepted > 0 && accepted < 100);
    let records: Vec<Value> = relay
        .journal()
        .split_inclusive('\n')
        .filter(|line| line.ends_with('\n'))
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        records.iter().filter(|r| r["type"] == "input").count(),
        accepted as usize
    );
    let mut gate = UnixStream::connect(relay.home.path().join("gate")).unwrap();
    gate.write_all(b"go").unwrap();
    replay(&mut peer, 0);
    assert_eq!(receive(&mut peer)["data"], "ready");
    for _ in 0..accepted {
        assert_eq!(receive(&mut peer)["data"], "x".repeat(8192));
    }
    send(
        &mut peer,
        json!({"type":"message","seq":accepted+1,"data":"retry"}),
    );
    assert_eq!(receive(&mut peer)["data"], "retry");
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn invalid_input_does_not_advance_acceptance() {
    let mut relay = Relay::start("import sys\nfor line in sys.stdin: print(line.strip())");
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    send(
        &mut peer,
        json!({"type":"message","seq":1,"data":"invalid\nline"}),
    );
    let mut line = String::new();
    assert_eq!(peer.read_line(&mut line).unwrap(), 0);
    let mut peer = relay.raw_connect();
    assert_eq!(
        receive(&mut peer),
        json!({"type":"hello","protocol_version":1,"last_input_seq":0})
    );
    replay(&mut peer, 0);
    send(&mut peer, json!({"type":"message","seq":1,"data":"retry"}));
    assert_eq!(receive(&mut peer)["data"], "retry");
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
}

fn relay_with_redirected_descendant() -> (Relay, BufReader<UnixStream>, UnixStream) {
    let descendant = "import socket; s=socket.socket(socket.AF_UNIX); s.bind('descendant.sock'); s.listen(); print('ready',flush=True); c,_=s.accept(); c.sendall(b'R'); c.recv(1)";
    let script = format!(
        "import os,subprocess,sys\np=subprocess.Popen(['python3','-u','-c',{descendant:?}],stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)\nassert p.stdout.readline()==b'ready\\n'\nprint('ready',flush=True)\nos.close(1); os.close(2)\nsys.stdin.readline()\nos._exit(7)"
    );
    let relay = Relay::start(&script);
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    assert_eq!(receive(&mut peer)["data"], "ready");
    let mut socket = UnixStream::connect(relay.home.path().join("descendant.sock")).unwrap();
    socket
        .set_read_timeout(Some(crate::common::TIMEOUT))
        .unwrap();
    let mut byte = [0];
    std::io::Read::read_exact(&mut socket, &mut byte).unwrap();
    assert_eq!(byte, [b'R']);
    (relay, peer, socket)
}

fn assert_descendant_stopped(mut socket: UnixStream) {
    let mut byte = [0];
    assert_eq!(std::io::Read::read(&mut socket, &mut byte).unwrap(), 0);
}

#[test]
fn agent_exit_kills_descendants_without_session_pipes() {
    let (mut relay, mut peer, socket) = relay_with_redirected_descendant();
    send(&mut peer, json!({"type":"message","seq":1,"data":"exit"}));
    let exit = receive(&mut peer);
    assert_eq!(exit["type"], "exit");
    assert_eq!(exit["code"], 7);
    assert_descendant_stopped(socket);
    send(&mut peer, json!({"type":"ack","seq":exit["seq"]}));
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn stop_kills_descendants_without_session_pipes() {
    let (mut relay, mut peer, socket) = relay_with_redirected_descendant();
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
    assert_descendant_stopped(socket);
}

#[test]
fn ctrl_c_kills_descendants_without_session_pipes() {
    let (mut relay, _peer, socket) = relay_with_redirected_descendant();
    let pid = rustix::process::Pid::from_raw(relay.child.id() as i32).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
    assert!(relay.child.wait().unwrap().success());
    assert_descendant_stopped(socket);
}

#[test]
fn stop_after_agent_exit_keeps_descendants_stopped() {
    let (mut relay, mut peer, socket) = relay_with_redirected_descendant();
    send(&mut peer, json!({"type":"message","seq":1,"data":"exit"}));
    assert_eq!(receive(&mut peer)["type"], "exit");
    assert_descendant_stopped(socket);
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
}

#[test]
fn ctrl_c_after_agent_exit_keeps_descendants_stopped() {
    let (mut relay, mut peer, socket) = relay_with_redirected_descendant();
    send(&mut peer, json!({"type":"message","seq":1,"data":"exit"}));
    assert_eq!(receive(&mut peer)["type"], "exit");
    assert_descendant_stopped(socket);
    let pid = rustix::process::Pid::from_raw(relay.child.id() as i32).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
    assert!(relay.child.wait().unwrap().success());
}
