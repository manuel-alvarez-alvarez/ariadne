//! A scriptable stub ACP agent, as test support.
//!
//! [`stub_acp_agent`] writes a python3 program that speaks ACP version 1 over
//! its standard input and output, answering from a script the test wrote:
//! the capabilities it declares, the configuration options it offers, the
//! reply to each prompt — updates of any kind (message and thought chunks, a
//! plan, tool calls and as many updates of each as the test lists), a
//! permission request, the stop reason, an exit mid-turn, or a pause
//! (`wait_for`) a test holds the turn open on after its updates went out,
//! during which a `session/cancel` ends the turn as `cancelled`, with the
//! turn's usage, unless `ignore_cancel` says otherwise, and during which
//! `updates_when` sends more updates once a file exists — and the
//! stored sessions a load or resume finds, which `session/list` answers
//! whole, or `session_page_size` at a time behind a `nextCursor` — and never
//! answers a page from `session_list_stall_from` on, and how long the agent
//! takes to come up at all (`start_delay`, in seconds). With `writer_child`
//! it is codex-acp's shape: the conversation is written by a child process
//! the agent starts, which outlives the agent by [`WRITER_LINGER_SECS`]
//! unless it is killed too, and a stored session resumed while another
//! agent's child still writes it is refused as having an active writer. The
//! harness registers it as the registry agent `stub` ([`registry_home`] for
//! a script of the test's own), the daemon spawns it as the agent, and the
//! test reads everything the daemon sent back out of its log — each message
//! tagged with the Ariadne session the agent process ran under.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// How long a `writer_child` agent's writer lives on once the agent is gone,
/// the way `codex app-server` finishes up after its client closes: longer
/// than any wait a test gives a kill.
pub(crate) const WRITER_LINGER_SECS: u64 = 30;

/// One stub agent on disk: the executable the registry runs, and the files it
/// reports through.
pub(crate) struct StubAcpAgent {
    /// The wrapper script the daemon spawns.
    pub bin: String,
    script_file: PathBuf,
    log: PathBuf,
    launches: PathBuf,
    pid_file: PathBuf,
}

impl StubAcpAgent {
    /// Every JSON-RPC message the daemon sent, in the order it arrived.
    pub(crate) fn messages(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("a logged JSON-RPC message"))
            .collect()
    }

    /// The methods of the requests among them.
    pub(crate) fn methods(&self) -> Vec<String> {
        self.messages()
            .iter()
            .filter_map(|m| m.get("method").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    /// The params of every request of `method`.
    pub(crate) fn calls_of(&self, method: &str) -> Vec<Value> {
        self.messages()
            .into_iter()
            .filter(|m| m.get("method").and_then(Value::as_str) == Some(method))
            .map(|m| m.get("params").cloned().unwrap_or_default())
            .collect()
    }

    /// The text of every `session/prompt` the agent processes of one Ariadne
    /// session were sent, in order.
    pub(crate) fn prompts_for(&self, session_id: &str) -> Vec<String> {
        self.messages()
            .into_iter()
            .filter(|m| m.get("method").and_then(Value::as_str) == Some("session/prompt"))
            .filter(|m| m.get("ariadne_session").and_then(Value::as_str) == Some(session_id))
            .filter_map(|m| {
                m.pointer("/params/prompt/0/text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }

    /// The arguments every agent process of one Ariadne session was started
    /// with behind the stub's own command, one list per process, in order.
    pub(crate) fn launches_for(&self, session_id: &str) -> Vec<Vec<String>> {
        std::fs::read_to_string(&self.launches)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("a logged launch"))
            .filter(|launch| {
                launch.get("ariadne_session").and_then(Value::as_str) == Some(session_id)
            })
            .map(|launch| {
                launch["argv"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .collect()
    }

    /// The codex-acp initial mode each process of one Ariadne session received.
    pub(crate) fn codex_modes_for(&self, session_id: &str) -> Vec<Option<String>> {
        std::fs::read_to_string(&self.launches)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("a logged launch"))
            .filter(|launch| {
                launch.get("ariadne_session").and_then(Value::as_str) == Some(session_id)
            })
            .map(|launch| {
                launch
                    .get("codex_mode")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }

    /// Forget every message logged so far: what a test starts asserting from
    /// after the discovery probe has already driven the stub once.
    pub(crate) fn clear_messages(&self) {
        let _ = std::fs::remove_file(&self.log);
    }

    /// Change what the stub answers from now on. Each agent process reads
    /// the script as it starts, so the next one the daemon spawns follows
    /// this one: how a test moves the world under a daemon between two
    /// requests.
    pub(crate) fn reprogram(&self, script: Value) {
        write_script_file(
            &self.script_file,
            script,
            &self.log,
            &self.launches,
            &self.pid_file,
        );
    }

    /// The pid of the child a `writer_child` agent process started, once it
    /// has written it down — the last process's, like [`Self::pid`].
    pub(crate) fn writer_pid(&self) -> Option<u32> {
        std::fs::read_to_string(format!("{}.writer", self.pid_file.display()))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// The agent process's pid, once it has written it down.
    pub(crate) fn pid(&self) -> Option<u32> {
        std::fs::read_to_string(&self.pid_file)
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Whether that process still exists — a reaped child does not, where an
    /// unreaped zombie still would.
    pub(crate) fn process_is_alive(&self) -> bool {
        self.pid().is_some_and(pid_is_alive)
    }

    /// A `PATH` holding this stub under each of `names`: the machine the
    /// registry searches, with those agents installed on it.
    ///
    /// Every name is a link to the one launcher the stubs share, so no new
    /// executable is created — macOS checks each of those alone, and the
    /// tests then wait for each other.
    pub(crate) fn path_with(&self, names: &[&str]) -> OsString {
        let dir = Path::new(&self.bin).parent().unwrap();
        for name in names {
            let link = dir.join(name);
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&self.bin, &link).unwrap();
        }
        dir.as_os_str().to_os_string()
    }
}

/// Whether `pid` still exists. A relaunch starts a second agent process that
/// overwrites the pid file, so a test that watches the first one dies keeps
/// its pid and asks here.
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

/// A home whose `config.toml` registers the stub as the agent `stub`, for a
/// harness built over it with [`super::HarnessBuilder::home`].
pub(crate) fn registry_home(stub: &StubAcpAgent) -> PathBuf {
    let home = Path::new(&stub.bin).parent().unwrap().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[[acp_agents]]\nid = \"stub\"\ncommand = [{:?}]\n",
            stub.bin
        ),
    )
    .unwrap();
    home
}

/// Wait for discovery to accept the stub, then drop the probes' traffic
/// from its log, so a test reads only what the daemon sent its agents.
///
/// A probe under full-suite load can run out its timeout: probe again until
/// the stub is accepted, so no test reads a timed-out snapshot.
pub(crate) async fn discovery_settled(h: &super::Harness, stub: &StubAcpAgent) {
    discovery_accepted(h, stub, "stub").await;
}

/// The same, for a stub registered under `agent_id`.
pub(crate) async fn discovery_accepted(h: &super::Harness, stub: &StubAcpAgent, agent_id: &str) {
    let accepted = || async {
        h.launcher.registry.agents().await.iter().any(|agent| {
            agent.id == agent_id && agent.status == ariadne_api::agents::AcpAgentStatus::Ready
        })
    };
    super::eventually(super::TIMEOUT, "discovery to accept the stub", || async {
        if accepted().await {
            return true;
        }
        h.launcher.registry.refresh().await;
        accepted().await
    })
    .await;
    stub.clear_messages();
}

/// One configuration option in the ACP shape, as the stub offers it.
pub(crate) fn option(id: &str, category: &str, current: &str) -> Value {
    json!({
        "id": id,
        "name": id,
        "category": category,
        "type": "select",
        "currentValue": current,
        "options": [],
    })
}

/// The script most tests want: version 1, a resumable agent, a model and a
/// thought-level option, and one prompt turn that runs a tool call and
/// answers "done".
pub(crate) fn script() -> Value {
    json!({
        "capabilities": {"loadSession": true, "sessionCapabilities": {"resume": {}}},
        "session_id": "stub-session",
        "config_options": [
            option("model-id", "model", "old-model"),
            option("effort-id", "thought_level", "low"),
        ],
        "prompts": [{
            "updates": [
                {"sessionUpdate": "tool_call", "toolCallId": "call-1", "title": "Read",
                 "rawInput": {"path": "README.md"}},
                {"sessionUpdate": "tool_call_update", "toolCallId": "call-1", "title": "Read",
                 "status": "completed"},
                {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "done"}},
            ],
            "stop_reason": "end_turn",
        }],
    })
}

/// Write the stub into `dir` and answer with the handle the test drives it
/// by. The script is `script()` with whatever the test changed.
pub(crate) fn stub_acp_agent(dir: &Path, script: Value) -> StubAcpAgent {
    let log = dir.join("acp-messages.jsonl");
    let launches = dir.join("acp-launches.jsonl");
    let pid_file = dir.join("acp-agent.pid");
    let script_file = dir.join("acp-script.json");
    write_script_file(&script_file, script, &log, &launches, &pid_file);

    std::fs::write(dir.join("acp-stub.py"), STUB).unwrap();
    // The launcher is one shared file (see `shared_script`), linked into
    // `dir`. It finds the stub and the script of this test next to the link
    // it was started by. A second stub in the same `dir` replaces the link.
    let bin = dir.join("acp");
    let _ = std::fs::remove_file(&bin);
    std::os::unix::fs::symlink(super::shared_script(LAUNCHER), &bin).unwrap();
    StubAcpAgent {
        bin: bin.display().to_string(),
        script_file,
        log,
        launches,
        pid_file,
    }
}

/// The script as the stub reads it: the test's, plus where to report.
fn write_script_file(
    script_file: &Path,
    mut script: Value,
    log: &Path,
    launches: &Path,
    pid_file: &Path,
) {
    script["log"] = json!(log.display().to_string());
    script["launches"] = json!(launches.display().to_string());
    script["pid_file"] = json!(pid_file.display().to_string());
    script["writer_linger_secs"] = json!(WRITER_LINGER_SECS);
    std::fs::write(script_file, serde_json::to_string_pretty(&script).unwrap()).unwrap();
}

/// What the registry launches: the stub and the script next to the link
/// the launcher was started by. `$0` is that link, not the shared file.
const LAUNCHER: &str = "#!/bin/sh\n\
dir=$(dirname \"$0\")\n\
exec python3 \"$dir/acp-stub.py\" \"$dir/acp-script.json\" \"$@\"\n";

/// The stub itself: single-threaded, line-oriented, and honest about order —
/// it answers exactly what the script says, logs every incoming message, and
/// exits on stdin closing, the way an ACP agent ends with its client.
const STUB: &str = r#"#!/usr/bin/env python3
import json, os, select, subprocess, sys, time

script = json.load(open(sys.argv[1]))
# An agent slow to come up: nothing is said, and nothing read, until
# `start_delay` seconds have passed — the window in which the daemon has
# launched a session and heard nothing from it yet.
time.sleep(float(script.get("start_delay", 0)))
# Unbuffered, so a poll on the descriptor is the truth about what is left to
# read: a buffered reader could hold a line the poll can no longer see.
stdin = os.fdopen(0, "rb", buffering=0)
with open(script["pid_file"], "w") as f:
    f.write(str(os.getpid()))
with open(script["launches"], "a") as f:
    f.write(json.dumps({"ariadne_session": os.environ.get("ARIADNE_SESSION_ID"),
                        "codex_mode": os.environ.get("INITIAL_AGENT_MODE"),
                        "argv": sys.argv[2:]}) + "\n")

# The process that writes the conversation, as codex-acp's `codex
# app-server` does: a child of the agent that ends once the agent is gone —
# its stdin closes — but only after a while, unless it is killed with the
# agent's process group. Only an agent launched for a session starts one, so
# a discovery probe leaves nothing behind.
writer = None
if script.get("writer_child") and os.environ.get("ARIADNE_SESSION_ID"):
    writer = subprocess.Popen(
        [sys.executable, "-c",
         "import sys, time; sys.stdin.buffer.read(); time.sleep(%d)"
         % script["writer_linger_secs"]],
        stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    with open(script["pid_file"] + ".writer", "w") as f:
        f.write(str(writer.pid))


def alive(pid):
    """Whether `pid` runs: a zombie has let go of everything it held."""
    state = subprocess.run(["ps", "-o", "stat=", "-p", str(pid)],
                           capture_output=True, text=True).stdout.strip()
    return bool(state) and not state.startswith("Z")


def claim_writer(session, resumed):
    """Make this agent's child the conversation's writer — refused, for a
    resume, while another agent's child still is."""
    if writer is None:
        return
    lock = os.path.join(os.path.dirname(script["pid_file"]), "writer-%s" % session)
    try:
        holder = int(open(lock).read())
    except (OSError, ValueError):
        holder = None
    if resumed and holder is not None and holder != writer.pid and alive(holder):
        raise Failure(-32603, "thread %s already has an active writer" % session)
    with open(lock, "w") as f:
        f.write(str(writer.pid))


options = script.get("config_options", [])
prompts = list(script.get("prompts", []))
permissions = 0


class Failure(Exception):
    def __init__(self, code, message):
        self.code, self.message = code, message


def log(message):
    message = dict(message)
    message["ariadne_session"] = os.environ.get("ARIADNE_SESSION_ID")
    with open(script["log"], "a") as f:
        f.write(json.dumps(message) + "\n")


def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def read():
    line = stdin.readline()
    if not line:
        sys.exit(0)
    message = json.loads(line)
    log(message)
    return message


def cancelled_meanwhile():
    """Whether a `session/cancel` arrived on stdin — read without blocking,
    so a paused turn can keep waiting on its file. Anything else that
    arrives is logged and dropped, as an agent mid-turn would ignore it."""
    while select.select([stdin], [], [], 0)[0]:
        if read().get("method") == "session/cancel":
            return True
    return False


def respond(request):
    global permissions
    method = request.get("method")
    if method in script.get("unsupported_methods", []):
        raise Failure(-32601, "method not supported: %s" % method)
    sid = script.get("session_id", "stub-session")
    if method == "initialize":
        initialized = {"protocolVersion": script.get("protocol_version", 1),
                       "agentCapabilities": script.get("capabilities", {})}
        if "agent_info" in script:
            initialized["agentInfo"] = script["agent_info"]
        return initialized
    if method == "session/new":
        claim_writer(sid, resumed=False)
        return {"sessionId": sid, "configOptions": options}
    if method in ("session/load", "session/resume"):
        wanted = request.get("params", {}).get("sessionId")
        if wanted in script.get("stored_sessions", []):
            claim_writer(wanted, resumed=True)
            return {"configOptions": options}
        raise Failure(-32001, "unknown session %s" % wanted)
    if method == "session/close":
        return {}
    if method == "session/list":
        sessions = script.get("session_list", [])
        size = script.get("session_page_size")
        if not size:
            return {"sessions": sessions}
        # A paging agent: `size` sessions per page, and a cursor that names
        # where the next page starts, until nothing remains.
        cursor = request.get("params", {}).get("cursor")
        start = int(cursor) if cursor else 0
        stall_from = script.get("session_list_stall_from")
        if stall_from is not None and start >= stall_from:
            # An agent that never answers this page: the client's budget
            # is what ends the listing.
            while True:
                time.sleep(0.1)
        listed = {"sessions": sessions[start:start + size]}
        if start + size < len(sessions):
            listed["nextCursor"] = str(start + size)
        return listed
    if method == "session/set_config_option":
        params = request["params"]
        for option in options:
            if option.get("id") == params.get("configId"):
                option["currentValue"] = params.get("value")
        return {"configOptions": options}
    if method == "session/prompt":
        turn = prompts.pop(0) if prompts else {}
        if "permission" in turn:
            permissions += 1
            request_permission = dict(turn["permission"])
            request_permission["sessionId"] = sid
            send({"jsonrpc": "2.0", "id": "permission-%d" % permissions,
                  "method": "session/request_permission",
                  "params": request_permission})
            read()  # the client's answer, logged like everything else
        for update in turn.get("updates", []):
            send({"jsonrpc": "2.0", "method": "session/update",
                  "params": {"sessionId": sid, "update": update}})
        stop_reason = turn.get("stop_reason", "end_turn")
        wait_for = turn.get("wait_for")
        if wait_for:
            # Everything the turn has to say is out; now sit on it until the
            # test lets go — the window a "while a turn runs" test needs —
            # or until the client cancels the turn, unless the script has
            # this agent ignore the cancel.
            with open(wait_for + ".reached", "w") as f:
                f.write("1")
            # What the held turn still has to say, once the test says so:
            # `updates_when` names a file and the updates to send once it
            # exists, the way an agent reports a tool call ending mid-turn.
            late = turn.get("updates_when")
            while not os.path.exists(wait_for):
                if late and os.path.exists(late["file"]):
                    for update in late.get("updates", []):
                        send({"jsonrpc": "2.0", "method": "session/update",
                              "params": {"sessionId": sid, "update": update}})
                    late = None
                if cancelled_meanwhile() and not turn.get("ignore_cancel"):
                    stop_reason = "cancelled"
                    break
                time.sleep(0.01)
        if "exit" in turn:
            sys.exit(int(turn["exit"]))
        # A cancelled turn reports what it spent too, as ACP has it.
        response = {"stopReason": stop_reason}
        if "usage" in turn:
            response["usage"] = turn["usage"]
        if "quota" in turn:
            response["_meta"] = {"quota": {"token_count": turn["quota"]}}
        return response
    raise Failure(-32601, "method not supported: %s" % method)


while True:
    message = read()
    if "method" not in message or "id" not in message:
        continue  # a response to our own request, or a notification
    if message["method"] in script.get("silent_methods", []):
        continue  # an agent that stops answering, for the client's timeout
    try:
        send({"jsonrpc": "2.0", "id": message["id"], "result": respond(message)})
    except Failure as failure:
        send({"jsonrpc": "2.0", "id": message["id"],
              "error": {"code": failure.code, "message": failure.message}})
"#;
