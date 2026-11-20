//! Exercise the relay through its executable and Unix socket.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};

struct Relay {
    child: Child,
    home: tempfile::TempDir,
    status: BufReader<std::process::ChildStdout>,
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
        let home = tempfile::tempdir().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_ariadne-relay"))
            .args([
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
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let status = BufReader::new(child.stdout.take().unwrap());
        let mut relay = Self {
            child,
            home,
            status,
            agent_pid: None,
        };
        let mut line = String::new();
        assert!(
            relay.status.read_line(&mut line).unwrap() > 0,
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
    fn connect(&self) -> BufReader<UnixStream> {
        let socket = UnixStream::connect(self.home.path().join("relays/session1.sock")).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        BufReader::new(socket)
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
    send(&mut peer, json!({"type":"message","data":"{\"id\":99}"}));
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
        json!({"type":"message","data":"{\"result\":\"allow\"}"}),
    );
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
        assert!(relay.status.read_line(&mut line).unwrap() > 0);
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
    let script = "import subprocess,sys\nsubprocess.Popen(['python3','-u','-c',\"import socket,sys; s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[1]); s.listen(); print('ready',flush=True); c,_=s.accept(); c.recv(1)\",sys.argv[0]+'.child'])\nsys.stdin.readline()";
    let mut relay = Relay::start(script);
    // The fixture uses a private path instead of a shared executable or socket.
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    assert_eq!(receive(&mut peer)["data"], "ready");
    let socket = UnixStream::connect(relay.home.path().join("-c.child")).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
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
        assert!(relay.status.read_line(&mut line).unwrap() > 0);
        if line.contains("state=exited") {
            break;
        }
    }
    let mut peer = relay.connect();
    replay(&mut peer, 0);
    assert_eq!(receive(&mut peer)["seq"], 1);
    stop(&mut peer);
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), async {
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
    let output = Command::new(env!("CARGO_BIN_EXE_ariadne-relay"))
        .args(args)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let mut peer = relay.connect();
    stop(&mut peer);
    assert!(relay.child.wait().unwrap().success());
    let invalid = Command::new(env!("CARGO_BIN_EXE_ariadne-relay"))
        .args([
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
    let usage = Command::new(env!("CARGO_BIN_EXE_ariadne-relay"))
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
}
