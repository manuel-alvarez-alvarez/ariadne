//! A scriptable stub ACP agent, as test support.
//!
//! [`stub_acp_agent`] writes a python3 program that speaks ACP version 1 over
//! its standard input and output, answering from a script the test wrote:
//! the capabilities it declares, the configuration options it offers, the
//! reply to each prompt — updates, a permission request, the stop reason, or
//! an exit mid-turn — and the stored sessions a load or resume finds. The
//! harness points `acp_bin` at it ([`super::HarnessBuilder::acp_bin`]), the
//! daemon spawns it as the agent, and the test reads everything the daemon
//! sent back out of its log.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// One stub agent on disk: the executable the harness points `acp_bin` at,
/// and the files it reports through.
pub struct StubAcpAgent {
    /// The wrapper script the daemon spawns.
    pub bin: String,
    log: PathBuf,
    pid_file: PathBuf,
}

impl StubAcpAgent {
    /// Every JSON-RPC message the daemon sent, in the order it arrived.
    pub fn messages(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("a logged JSON-RPC message"))
            .collect()
    }

    /// The methods of the requests among them.
    pub fn methods(&self) -> Vec<String> {
        self.messages()
            .iter()
            .filter_map(|m| m.get("method").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    /// The params of every request of `method`.
    pub fn calls_of(&self, method: &str) -> Vec<Value> {
        self.messages()
            .into_iter()
            .filter(|m| m.get("method").and_then(Value::as_str) == Some(method))
            .map(|m| m.get("params").cloned().unwrap_or_default())
            .collect()
    }

    /// The agent process's pid, once it has written it down.
    pub fn pid(&self) -> Option<u32> {
        std::fs::read_to_string(&self.pid_file)
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Whether that process still exists — a reaped child does not, where an
    /// unreaped zombie still would.
    pub fn process_is_alive(&self) -> bool {
        self.pid().is_some_and(pid_is_alive)
    }
}

/// Whether `pid` still exists. A relaunch starts a second agent process that
/// overwrites the pid file, so a test that watches the first one dies keeps
/// its pid and asks here.
pub fn pid_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

/// One configuration option in the ACP shape, as the stub offers it.
pub fn option(id: &str, category: &str, current: &str) -> Value {
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
pub fn script() -> Value {
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
pub fn stub_acp_agent(dir: &Path, mut script: Value) -> StubAcpAgent {
    let log = dir.join("acp-messages.jsonl");
    let pid_file = dir.join("acp-agent.pid");
    script["log"] = json!(log.display().to_string());
    script["pid_file"] = json!(pid_file.display().to_string());
    let script_file = dir.join("acp-script.json");
    std::fs::write(&script_file, serde_json::to_string_pretty(&script).unwrap()).unwrap();

    let program = dir.join("acp-stub.py");
    std::fs::write(&program, STUB).unwrap();
    let bin = dir.join("acp");
    super::write_script(
        &bin,
        &format!(
            "#!/bin/sh\nexec python3 '{}' '{}' \"$@\"\n",
            program.display(),
            script_file.display()
        ),
    );
    StubAcpAgent {
        bin: bin.display().to_string(),
        log,
        pid_file,
    }
}

/// The stub itself: single-threaded, line-oriented, and honest about order —
/// it answers exactly what the script says, logs every incoming message, and
/// exits on stdin closing, the way an ACP agent ends with its client.
const STUB: &str = r#"#!/usr/bin/env python3
import json, os, sys

script = json.load(open(sys.argv[1]))
with open(script["pid_file"], "w") as f:
    f.write(str(os.getpid()))

options = script.get("config_options", [])
prompts = list(script.get("prompts", []))
permissions = 0


class Failure(Exception):
    def __init__(self, code, message):
        self.code, self.message = code, message


def log(message):
    with open(script["log"], "a") as f:
        f.write(json.dumps(message) + "\n")


def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def read():
    line = sys.stdin.readline()
    if not line:
        sys.exit(0)
    message = json.loads(line)
    log(message)
    return message


def respond(request):
    global permissions
    method = request.get("method")
    sid = script.get("session_id", "stub-session")
    if method == "initialize":
        return {"protocolVersion": script.get("protocol_version", 1),
                "agentCapabilities": script.get("capabilities", {})}
    if method == "session/new":
        return {"sessionId": sid, "configOptions": options}
    if method in ("session/load", "session/resume"):
        wanted = request.get("params", {}).get("sessionId")
        if wanted in script.get("stored_sessions", []):
            return {"configOptions": options}
        raise Failure(-32001, "unknown session %s" % wanted)
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
        if "exit" in turn:
            sys.exit(int(turn["exit"]))
        return {"stopReason": turn.get("stop_reason", "end_turn")}
    raise Failure(-32601, "method not supported: %s" % method)


while True:
    message = read()
    if "method" not in message or "id" not in message:
        continue  # a response to our own request, or a notification
    try:
        send({"jsonrpc": "2.0", "id": message["id"], "result": respond(message)})
    except Failure as failure:
        send({"jsonrpc": "2.0", "id": message["id"],
              "error": {"code": failure.code, "message": failure.message}})
"#;
