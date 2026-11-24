//! Every test stub ends when its direct parent ends.

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};

use serde_json::json;

use crate::common::acp::{pid_is_alive, script, stub_acp_agent};
use crate::common::forge::stub_forge_cli;
use crate::common::ssh::{StubSsh, stub_ssh};
use crate::common::{TIMEOUT, eventually};

/// A stand-in parent: it starts the program under test, inheriting its own
/// standard input, then outlives it for 30 seconds, the way a real one
/// would. [`parent`] pipes that standard input in from the test and keeps
/// the write end open past the kill below, so nothing but the stub's own
/// parent check can end it — the ACP stub also exits on a stdin closed by
/// anything else, and that other rule must not be what this test catches.
const PARENT: &str = r#"
import subprocess, sys, time
sys.stdin.buffer.read(1)
subprocess.Popen(sys.argv[1:])
time.sleep(30)
"#;

fn parent(program: &str, args: &[&str]) -> (Child, ChildStdin) {
    let mut process = Command::new("python3")
        .arg("-c")
        .arg(PARENT)
        .arg(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = process.stdin.take().unwrap();
    (process, stdin)
}

fn launch(stdin: &mut ChildStdin) {
    stdin.write_all(b"x").unwrap();
}

async fn wait_for_exit(pid: u32) {
    eventually(TIMEOUT, "the orphaned stub to exit", || async {
        !pid_is_alive(pid)
    })
    .await;
}

#[tokio::test]
async fn a_forge_stub_exits_when_its_parent_is_killed() {
    let gate_dir = tempfile::tempdir().unwrap();
    let gate = gate_dir.path().join("never");
    let forge = stub_forge_cli(json!([{
        "args": ["api"], "wait_for": gate.display().to_string()
    }]));
    let (mut process, mut stdin) = parent(&forge.gh, &["api"]);
    forge.set_parent_pid(process.id());
    launch(&mut stdin);
    eventually(TIMEOUT, "the forge stub to start", || async {
        forge.pid().is_some()
    })
    .await;
    let pid = forge.pid().unwrap();
    process.kill().unwrap();
    // Keep the dead parent unreaped while proving the stub notices its
    // reparenting; kill(pid, 0) alone would call that zombie alive.
    wait_for_exit(pid).await;
    process.wait().unwrap();
}

#[tokio::test]
async fn an_acp_stub_exits_when_its_parent_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), script());
    // Held open for the whole test: were it closed, the stub would exit on
    // reading the end of its input, the rule it already had, and the kill
    // below would prove nothing about the parent check this test is for.
    let (mut process, mut stdin) = parent(&stub.bin, &[]);
    stub.set_parent_pid(process.id());
    launch(&mut stdin);
    eventually(TIMEOUT, "the ACP stub to start", || async {
        stub.pid().is_some()
    })
    .await;
    let pid = stub.pid().unwrap();
    process.kill().unwrap();
    // Keep the dead parent unreaped while proving the stub notices its
    // reparenting; kill(pid, 0) alone would call that zombie alive.
    wait_for_exit(pid).await;
    process.wait().unwrap();
}

/// Start `ssh` with `args` under a stand-in parent, kill and reap that
/// parent, and wait for the run to end. The stub checks its parent with
/// `kill -0`, as its Python form did, so the parent is reaped first.
async fn ssh_run_exits_with_its_parent(ssh: &StubSsh, args: &[&str]) {
    let (mut process, mut stdin) = parent(&ssh.ssh, args);
    ssh.set_parent_pid(process.id());
    launch(&mut stdin);
    eventually(TIMEOUT, "the ssh stub to start", || async {
        !ssh.runs().is_empty()
    })
    .await;
    let pid = ssh.runs()[0].pid;
    process.kill().unwrap();
    process.wait().unwrap();
    wait_for_exit(pid).await;
    drop(stdin);
}

#[tokio::test]
async fn an_ssh_stub_whose_home_call_hangs_exits_when_its_parent_is_killed() {
    let ssh = stub_ssh(json!({"home_hang": true}));
    ssh_run_exits_with_its_parent(&ssh, &["box", "printf %s \"$HOME\""]).await;
}

/// A runtime command that waits for the disconnect FIFO ends with its
/// parent too, while no writer ever opens that FIFO: the read alone would
/// never end.
#[tokio::test]
async fn an_ssh_stub_waiting_for_its_disconnect_exits_when_its_parent_is_killed() {
    let home = tempfile::tempdir().unwrap();
    let fifo = home.path().join("disconnect");
    let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(made.success(), "mkfifo {}", fifo.display());
    let ssh = stub_ssh(json!({"setup": {
        "home": home.path(), "runtime_disconnect": fifo
    }, "exit": 0}));
    ssh_run_exits_with_its_parent(&ssh, &["-T", "box", "sh -c 'bounded provide node'"]).await;
}
