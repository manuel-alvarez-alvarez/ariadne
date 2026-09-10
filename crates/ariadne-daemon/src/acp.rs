//! The ACP runtime: the daemon's own client for agents that speak the Agent
//! Client Protocol.
//!
//! Every other agent kind runs a CLI inside a tmux pane. An `acp` session is
//! a child process of the daemon itself: spawned with piped standard input
//! and output, driven over newline-delimited JSON-RPC (ACP version 1), reaped
//! when it exits, and killed when its session is killed. The protocol code is
//! ported from the CLI-side client (`ariadne-cli/src/commands/acp.rs`), which
//! `ariadne _spawn` still carries for launches outside the daemon.
//!
//! What the agent does is reported through the same ingestion the hooks use
//! (`crate::http::events::ingest_event`), in the ACP adapter's event
//! vocabulary, so an `acp` session's events, status, attention and internal
//! id read exactly like every other session's. Every permission request is
//! approved: permission modes are a later task's.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot};

use ariadne_api::events::IngestEventRequest;
use ariadne_core::AgentKind;
use ariadne_core::acp::LaunchConfig;
use ariadne_store::Store;

use crate::http::events::ingest_event;
use crate::scheduler::SchedEvent;

/// Everything one launch of an ACP agent is made of. The launcher builds it
/// from the adapter's spawn plan: the argv and environment as planned, and
/// the `acp.json` the adapter wrote, read back as the protocol's half.
pub struct AcpLaunch {
    pub session_id: String,
    /// The launch every event of this process reports under.
    pub launch_id: String,
    /// The executable to spawn — `Config::acp_bin`, which is the contract's
    /// `acp` outside a test.
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub config: LaunchConfig,
}

/// The daemon-owned ACP agents, one child process per live `acp` session.
///
/// Cheap to clone: every clone shares the one registry, which is what lets a
/// driver task deregister itself and the launcher ask who is alive. Unlike
/// tmux there is no "could not be asked" — the registry always answers, and a
/// daemon restart answers "no" for every child of the daemon that died.
#[derive(Clone)]
pub struct AcpRuntime {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    /// Live agents by Ariadne session id.
    running: Mutex<HashMap<String, RunningAgent>>,
    /// Wakes the scheduler after an event lands, the way the HTTP ingestion
    /// does — present once a scheduler is running.
    scheduler: OnceLock<mpsc::UnboundedSender<SchedEvent>>,
}

struct RunningAgent {
    /// The launch this agent runs under: what tells a driver's own entry from
    /// a successor's on the same seat. A relaunch replaces the entry while
    /// the old driver is still winding down, and the old driver's parting
    /// deregistration must not take the new agent's entry — dropping its stop
    /// sender is what kills it — so removal is gated on this id.
    launch_id: String,
    /// Dropping or firing this tells the driver to kill its child and end.
    stop: oneshot::Sender<()>,
    /// Console input for this agent: each send becomes a `session/prompt`,
    /// sent at once if the agent is between turns and queued, in order,
    /// behind whichever one is running.
    prompts: mpsc::UnboundedSender<String>,
}

impl AcpRuntime {
    pub fn new(store: Store) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                running: Mutex::new(HashMap::new()),
                scheduler: OnceLock::new(),
            }),
        }
    }

    /// Give the runtime the scheduler's waker. Called once, from the
    /// scheduler's own start.
    pub fn connect_scheduler(&self, tx: mpsc::UnboundedSender<SchedEvent>) {
        let _ = self.inner.scheduler.set(tx);
    }

    /// Whether this session's agent process is still owned and driven here.
    pub fn is_running(&self, session_id: &str) -> bool {
        self.inner
            .running
            .lock()
            .expect("acp registry lock")
            .contains_key(session_id)
    }

    /// Hand the running agent a console prompt: the daemon's own console has
    /// no pane to type into, so this is what a client's input becomes. Queued
    /// rather than sent while a turn is running — an agent mid-`session/prompt`
    /// cannot be asked for another one — and sent the moment it ends.
    ///
    /// Errs where there is nobody here to hear it: no agent runs for this
    /// session, which is the console's counterpart of a pane that is gone.
    pub fn send_prompt(&self, session_id: &str, text: String) -> Result<()> {
        self.inner
            .running
            .lock()
            .expect("acp registry lock")
            .get(session_id)
            .ok_or_else(|| anyhow!("no ACP agent is running for session {session_id}"))?
            .prompts
            .send(text)
            .map_err(|_| anyhow!("the ACP agent for session {session_id} is no longer listening"))
    }

    /// Spawn the agent and drive it until it exits or is killed. A driver
    /// still holding this session is stopped first: one seat, one agent.
    pub async fn launch(&self, launch: AcpLaunch) -> Result<()> {
        self.kill(&launch.session_id);
        let mut child = Command::new(&launch.program)
            .args(&launch.args)
            .envs(launch.env.iter().cloned())
            .current_dir(&launch.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "starting ACP agent {} in {}",
                    launch.program,
                    launch.cwd.display()
                )
            })?;
        let stdin = child
            .stdin
            .take()
            .context("opening the ACP agent's stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("opening the ACP agent's stdout")?;
        if let Some(stderr) = child.stderr.take() {
            let session = launch.session_id.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(session = %session, "acp agent stderr: {line}");
                }
            });
        }

        let (stop, stopped) = oneshot::channel();
        let (prompts, queued) = mpsc::unbounded_channel();
        self.inner
            .running
            .lock()
            .expect("acp registry lock")
            .insert(
                launch.session_id.clone(),
                RunningAgent {
                    launch_id: launch.launch_id.clone(),
                    stop,
                    prompts,
                },
            );
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime
                .drive(launch, child, stdout, stdin, stopped, queued)
                .await
        });
        Ok(())
    }

    /// Take a session's agent down. The entry goes at once — a spawn guard
    /// asking right after is told the seat is free — and the driver kills and
    /// reaps the child behind it. A session with no agent here is a no-op.
    pub fn kill(&self, session_id: &str) {
        let agent = self
            .inner
            .running
            .lock()
            .expect("acp registry lock")
            .remove(session_id);
        if let Some(agent) = agent {
            tracing::info!(session = %session_id, "killing the ACP agent");
            let _ = agent.stop.send(());
        }
    }

    /// Drop a driver's own entry, and only its own: by the time a replaced
    /// driver gets here, the seat's entry is its successor's, and removing
    /// that one would kill the very agent the relaunch just started.
    fn deregister(&self, session_id: &str, launch_id: &str) {
        let mut running = self.inner.running.lock().expect("acp registry lock");
        if running
            .get(session_id)
            .is_some_and(|agent| agent.launch_id == launch_id)
        {
            running.remove(session_id);
        }
    }

    /// One agent's whole life: the protocol until it ends, is killed, or
    /// fails; then the kill and the reap; then the session's last words.
    async fn drive(
        self,
        launch: AcpLaunch,
        mut child: Child,
        stdout: ChildStdout,
        stdin: ChildStdin,
        stopped: oneshot::Receiver<()>,
        prompts: mpsc::UnboundedReceiver<String>,
    ) {
        let sink = EventSink {
            runtime: self.clone(),
            session_id: launch.session_id.clone(),
            launch_id: launch.launch_id.clone(),
            agent_session: Arc::new(OnceLock::new()),
        };
        let mut rpc = Rpc::new(BufReader::new(stdout), stdin, sink.clone());
        let outcome = tokio::select! {
            result = run_protocol(&mut rpc, &launch.cwd, &launch.config, prompts) => Some(result),
            _ = stopped => None,
        };
        // Reap on exit, kill on a kill: the signal is a no-op on a child
        // already gone, and the wait is what collects it either way.
        let _ = child.start_kill();
        let _ = child.wait().await;
        if let Some(Err(error)) = &outcome {
            tracing::warn!(session = %launch.session_id, error = %format!("{error:#}"), "ACP agent failed");
            sink.emit(
                "session.error",
                json!({
                    "session_id": sink.agent_session_id(&launch.config),
                    "error": {"data": {"message": format!("{error:#}")}},
                }),
            )
            .await;
        }
        sink.emit(
            "session_end",
            json!({"session_id": sink.agent_session_id(&launch.config)}),
        )
        .await;
        self.deregister(&launch.session_id, &launch.launch_id);
    }
}

/// Where the driver reports what its agent does: the same ingestion the
/// hooks post to, minus the process and the socket.
#[derive(Clone)]
struct EventSink {
    runtime: AcpRuntime,
    session_id: String,
    launch_id: String,
    /// The agent's own session id, once setup has learned it: what every
    /// payload names as `session_id`, and what the ingestion records on the
    /// row.
    agent_session: Arc<OnceLock<String>>,
}

impl EventSink {
    /// Event reporting is fail-safe, like every agent hook: an event that
    /// cannot be recorded costs the record, never the agent.
    async fn emit(&self, kind: &str, payload: Value) {
        let request = IngestEventRequest {
            session_id: self.session_id.clone(),
            launch: Some(self.launch_id.clone()),
            agent_kind: AgentKind::Acp,
            kind: kind.to_string(),
            payload,
        };
        if let Err(e) = ingest_event(&self.runtime.inner.store, &request).await {
            tracing::warn!(session = %self.session_id, kind, error = %e, "recording an ACP event failed");
            return;
        }
        if let Some(tx) = self.runtime.inner.scheduler.get() {
            let _ = tx.send(SchedEvent::SessionEvent(self.session_id.clone()));
        }
    }

    /// The ACP session id as a payload value: the one setup learned, else the
    /// one a resume was asked for, else null.
    fn agent_session_id(&self, config: &LaunchConfig) -> Value {
        self.agent_session
            .get()
            .cloned()
            .or_else(|| config.resume_session_id.clone())
            .map_or(Value::Null, Value::String)
    }
}

struct Rpc {
    reader: tokio::io::Lines<BufReader<ChildStdout>>,
    writer: BufWriter<ChildStdin>,
    next_id: u64,
    sink: EventSink,
    assistant_text: String,
}

impl Rpc {
    fn new(reader: BufReader<ChildStdout>, writer: ChildStdin, sink: EventSink) -> Self {
        Self {
            reader: reader.lines(),
            writer: BufWriter::new(writer),
            next_id: 1,
            sink,
            assistant_text: String::new(),
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;

        loop {
            let line = self
                .reader
                .next_line()
                .await
                .context("reading from the ACP agent")?
                .ok_or_else(|| anyhow!("ACP agent closed stdout during {method}"))?;
            let message: Value = serde_json::from_str(&line)
                .with_context(|| format!("reading ACP message `{line}`"))?;
            if message.get("id").and_then(Value::as_u64) == Some(id)
                && message.get("method").is_none()
            {
                if let Some(error) = message.get("error") {
                    bail!("ACP {method} failed: {error}");
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| anyhow!("ACP {method} response has no result"));
            }
            self.handle_incoming(message).await?;
        }
    }

    async fn write(&mut self, message: &Value) -> Result<()> {
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        self.writer
            .write_all(&line)
            .await
            .context("writing to the ACP agent")?;
        self.writer.flush().await.context("flushing ACP request")
    }

    async fn handle_incoming(&mut self, message: Value) -> Result<()> {
        match message.get("method").and_then(Value::as_str) {
            Some("session/update") => self.handle_update(&message["params"]).await,
            Some("session/request_permission") if message.get("id").is_some() => {
                self.handle_permission(&message).await
            }
            Some(_) if message.get("id").is_some() => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": message["id"],
                    "error": {"code": -32601, "message": "method not supported"},
                });
                self.write(&response).await
            }
            _ => Ok(()),
        }
    }

    async fn handle_update(&mut self, params: &Value) -> Result<()> {
        let session_id = params.get("sessionId").cloned().unwrap_or(Value::Null);
        let update = params.get("update").cloned().unwrap_or_default();
        let update_kind = update.get("sessionUpdate").and_then(Value::as_str);
        match update_kind {
            Some("agent_message_chunk") => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    self.assistant_text.push_str(text);
                }
            }
            Some("tool_call") => {
                self.sink
                    .emit("pre_tool_use", tool_payload(session_id, &update))
                    .await;
            }
            Some("tool_call_update") if terminal_tool_status(&update) => {
                self.sink
                    .emit("post_tool_use", tool_payload(session_id, &update))
                    .await;
            }
            Some("compaction_update")
                if update.get("status").and_then(Value::as_str) == Some("completed") =>
            {
                let mut payload = update;
                payload["session_id"] = session_id;
                self.sink.emit("compaction_update", payload).await;
            }
            _ => {}
        }
        Ok(())
    }

    /// Approve the permission request. There is nobody at a pane to choose:
    /// the daemon selects the first allowing option, and a later task adds
    /// the permission modes that decide otherwise.
    async fn handle_permission(&mut self, message: &Value) -> Result<()> {
        let params = &message["params"];
        let session_id = params.get("sessionId").cloned().unwrap_or(Value::Null);
        let mut payload = tool_payload(session_id.clone(), &params["toolCall"]);
        payload["options"] = params.get("options").cloned().unwrap_or_default();
        self.sink.emit("permission_request", payload).await;

        let selected = approved_option(params);
        let outcome = selected.as_ref().map_or_else(
            || json!({"outcome": "cancelled"}),
            |option_id| json!({"outcome": "selected", "optionId": option_id}),
        );
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {"outcome": outcome},
        }))
        .await?;
        self.sink
            .emit(
                "permission.replied",
                json!({"session_id": session_id, "option_id": selected}),
            )
            .await;
        Ok(())
    }
}

/// Initialize, set the session up, pin the model and the effort, run the
/// initial prompt, then serve the agent — and whatever the console sends it —
/// until it exits.
async fn run_protocol(
    rpc: &mut Rpc,
    cwd: &Path,
    config: &LaunchConfig,
    prompts: mpsc::UnboundedReceiver<String>,
) -> Result<()> {
    let initialized = rpc
        .request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": {"readTextFile": false, "writeTextFile": false},
                    "terminal": false,
                    "session": {"configOptions": {}, "compaction": {}},
                    "auth": {},
                },
                "clientInfo": {"name": "ariadne", "version": env!("CARGO_PKG_VERSION")},
            }),
        )
        .await?;
    if initialized.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
        bail!("ACP agent did not negotiate protocol version 1");
    }

    let setup = session_setup(rpc, cwd, config, &initialized).await?;
    let session_id = setup
        .get("sessionId")
        .and_then(Value::as_str)
        .or(config.resume_session_id.as_deref())
        .ok_or_else(|| anyhow!("ACP session setup returned no session id"))?
        .to_string();
    let _ = rpc.sink.agent_session.set(session_id.clone());
    let options = setup
        .get("configOptions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let options = set_pinned_option(
        rpc,
        &session_id,
        options,
        &["model"],
        &["model"],
        "model",
        &config.model,
    )
    .await?;
    if let Some(effort) = &config.effort {
        set_pinned_option(
            rpc,
            &session_id,
            options,
            &["thought_level"],
            &["effort", "reasoning", "thought_level"],
            "effort",
            effort,
        )
        .await?;
    }

    rpc.sink
        .emit("session_start", json!({"session_id": session_id}))
        .await;
    if let Some(prompt) = config.initial_prompt.as_deref() {
        prompt_once(rpc, &session_id, &config.system_prompt, prompt).await?;
    }
    serve_with_input(rpc, &session_id, &config.system_prompt, prompts).await
}

/// Serve the agent until it exits: the turn is over, and the agent stays up
/// for the next one the way a TUI stays at its prompt. What ends this is the
/// agent closing its stdout — its own exit, or the kill.
///
/// Console input is the other thing that can start a turn here, alongside the
/// agent's own notifications: a prompt queued while one was running waits in
/// `prompts` for this loop to come back around, and is sent the moment it
/// does. Only one turn is ever in flight — `prompt_once` does not return
/// until the agent's response does — so nothing here is sent while another
/// `session/prompt` is outstanding.
async fn serve_with_input(
    rpc: &mut Rpc,
    session_id: &str,
    system_prompt: &str,
    mut prompts: mpsc::UnboundedReceiver<String>,
) -> Result<()> {
    // Once the console side is gone there is nothing left to queue, but the
    // agent may still have plenty to say — the branch is dropped rather than
    // polled into a busy loop of immediate `None`s.
    let mut console_open = true;
    loop {
        tokio::select! {
            prompt = prompts.recv(), if console_open => {
                match prompt {
                    Some(prompt) => prompt_once(rpc, session_id, system_prompt, &prompt).await?,
                    None => console_open = false,
                }
            }
            line = rpc.reader.next_line() => {
                let Some(line) = line.context("reading from the ACP agent")? else {
                    return Ok(());
                };
                let message: Value = serde_json::from_str(&line)
                    .with_context(|| format!("reading ACP message `{line}`"))?;
                rpc.handle_incoming(message).await?;
            }
        }
    }
}

async fn session_setup(
    rpc: &mut Rpc,
    cwd: &Path,
    config: &LaunchConfig,
    initialized: &Value,
) -> Result<Value> {
    let cwd = cwd.display().to_string();
    let mcp_servers = serde_json::to_value(&config.mcp_servers)?;
    let Some(session_id) = &config.resume_session_id else {
        return rpc
            .request(
                "session/new",
                json!({"cwd": cwd, "mcpServers": mcp_servers}),
            )
            .await;
    };
    let capabilities = &initialized["agentCapabilities"];
    let resume = &capabilities["sessionCapabilities"]["resume"];
    if resume.is_object() || resume.as_bool() == Some(true) {
        let mut result = rpc
            .request(
                "session/resume",
                json!({"sessionId": session_id, "cwd": cwd, "mcpServers": mcp_servers}),
            )
            .await?;
        result["sessionId"] = Value::String(session_id.clone());
        return Ok(result);
    }
    if capabilities.get("loadSession").and_then(Value::as_bool) == Some(true) {
        let mut result = rpc
            .request(
                "session/load",
                json!({"sessionId": session_id, "cwd": cwd, "mcpServers": mcp_servers}),
            )
            .await?;
        result["sessionId"] = Value::String(session_id.clone());
        return Ok(result);
    }
    bail!("ACP agent supports neither session/resume nor session/load")
}

async fn set_pinned_option(
    rpc: &mut Rpc,
    session_id: &str,
    options: Vec<Value>,
    categories: &[&str],
    names: &[&str],
    label: &str,
    value: &str,
) -> Result<Vec<Value>> {
    let option = options
        .iter()
        .find(|option| {
            option
                .get("category")
                .and_then(Value::as_str)
                .is_some_and(|category| categories.contains(&category))
        })
        .or_else(|| {
            options.iter().find(|option| {
                [option.get("id"), option.get("name")]
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .any(|candidate| {
                        names
                            .iter()
                            .any(|name| candidate.eq_ignore_ascii_case(name))
                    })
            })
        })
        .ok_or_else(|| anyhow!("ACP agent did not offer a {label} configuration option"))?;
    let config_id = option
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("ACP {label} configuration option has no id"))?;
    let response = rpc
        .request(
            "session/set_config_option",
            json!({"sessionId": session_id, "configId": config_id, "value": value}),
        )
        .await
        .with_context(|| format!("setting ACP {label} to `{value}`"))?;
    Ok(response
        .get("configOptions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or(options))
}

async fn prompt_once(
    rpc: &mut Rpc,
    session_id: &str,
    system_prompt: &str,
    prompt: &str,
) -> Result<()> {
    let prompt = format!("{system_prompt}\n\n{prompt}");
    rpc.assistant_text.clear();
    rpc.sink
        .emit(
            "user_prompt_submit",
            json!({"session_id": session_id, "prompt": prompt}),
        )
        .await;
    let response = rpc
        .request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": prompt}],
            }),
        )
        .await?;
    rpc.sink
        .emit(
            "stop",
            json!({
                "session_id": session_id,
                "stop_reason": response.get("stopReason"),
                "last_assistant_message": rpc.assistant_text.clone(),
            }),
        )
        .await;
    Ok(())
}

fn tool_payload(session_id: Value, update: &Value) -> Value {
    let name = update
        .get("title")
        .or_else(|| update.get("toolCallId"))
        .cloned()
        .unwrap_or_else(|| Value::String("ACP tool".into()));
    json!({
        "session_id": session_id,
        "tool_name": name,
        "tool_input": update.get("rawInput").cloned().unwrap_or_default(),
        "acp": update,
    })
}

fn terminal_tool_status(update: &Value) -> bool {
    matches!(
        update.get("status").and_then(Value::as_str),
        Some("completed" | "failed")
    )
}

/// The option that approves a permission request: the first the agent marks
/// as allowing, and the first of any kind where it marks none. `None` only
/// where there is nothing to select, which the reply spells as cancelled.
fn approved_option(params: &Value) -> Option<String> {
    let options = params.get("options").and_then(Value::as_array)?;
    let option_id = |option: &Value| {
        option
            .get("optionId")
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    options
        .iter()
        .find(|option| {
            option
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.starts_with("allow"))
        })
        .and_then(option_id)
        .or_else(|| options.first().and_then(option_id))
}

#[cfg(test)]
mod tests {
    use super::approved_option;

    use serde_json::json;

    /// Every permission request is approved: the allowing option wins
    /// wherever the agent put it, an unmarked list falls back to its first
    /// option, and only an empty one is answered with nothing to select.
    #[test]
    fn the_allowing_option_is_selected_wherever_it_stands() {
        let request = json!({"options": [
            {"optionId": "no", "name": "Reject", "kind": "reject_once"},
            {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
        ]});
        assert_eq!(approved_option(&request).as_deref(), Some("yes"));

        let unmarked = json!({"options": [
            {"optionId": "first", "name": "Only"},
        ]});
        assert_eq!(approved_option(&unmarked).as_deref(), Some("first"));

        assert_eq!(approved_option(&json!({"options": []})), None);
        assert_eq!(approved_option(&json!({})), None);
    }
}
