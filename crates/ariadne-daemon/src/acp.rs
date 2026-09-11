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
//! id read exactly like every other session's.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot};

use ariadne_api::events::IngestEventRequest;
use ariadne_core::acp::LaunchConfig;
use ariadne_core::{AgentKind, PermissionMode};
use ariadne_store::Store;

use crate::acp_rpc::{Incoming, RpcTransport};
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
    /// Repository this session works in. Learned approvals are scoped here.
    pub repository_id: String,
    /// The task override, or daemon default, resolved before the launch.
    pub permission_mode: PermissionMode,
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
    /// The reply channel while the agent waits on one permission request.
    permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

/// The pipe endpoints and permission reply slot a driver owns for one child.
struct DriverIo {
    stdout: ChildStdout,
    stdin: ChildStdin,
    permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
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

    /// Hand the running agent a prompt from the daemon itself — a scheduler
    /// nudge, a review briefing, an agent message. Always a `session/prompt`,
    /// queued in order behind whichever turn runs: unlike console input it
    /// answers no pending permission, so a delivery is never consumed as an
    /// option answer meant for a person.
    ///
    /// Errs where there is nobody here to hear it: no agent runs for this
    /// session, which is the prompt's counterpart of a pane that is gone.
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

    /// Hand the running agent console input. A pending permission consumes it
    /// as an option answer; otherwise it becomes a prompt as before.
    ///
    /// Errs where there is nobody here to hear it: no agent runs for this
    /// session, which is the console's counterpart of a pane that is gone.
    pub fn send_input(&self, session_id: &str, text: String) -> Result<()> {
        let (prompts, permission) = {
            let running = self.inner.running.lock().expect("acp registry lock");
            let agent = running
                .get(session_id)
                .ok_or_else(|| anyhow!("no ACP agent is running for session {session_id}"))?;
            (agent.prompts.clone(), agent.permission.clone())
        };
        if let Some(reply) = permission.lock().expect("ACP permission lock").take() {
            return reply.send(text).map_err(|_| {
                anyhow!("the ACP agent for session {session_id} is no longer waiting")
            });
        }
        prompts
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
        let permission = Arc::new(Mutex::new(None));
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
                    permission: permission.clone(),
                },
            );
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime
                .drive(
                    launch,
                    child,
                    DriverIo {
                        stdout,
                        stdin,
                        permission,
                    },
                    stopped,
                    queued,
                )
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
        io: DriverIo,
        stopped: oneshot::Receiver<()>,
        prompts: mpsc::UnboundedReceiver<String>,
    ) {
        let sink = EventSink {
            runtime: self.clone(),
            session_id: launch.session_id.clone(),
            launch_id: launch.launch_id.clone(),
            agent_session: Arc::new(OnceLock::new()),
        };
        let mut rpc = Rpc::new(io.stdout, io.stdin, sink.clone(), io.permission, &launch);
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
    transport: RpcTransport,
    sink: EventSink,
    assistant_text: String,
    repository_id: String,
    permission_mode: PermissionMode,
    pending_permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

impl Rpc {
    fn new(
        stdout: ChildStdout,
        stdin: ChildStdin,
        sink: EventSink,
        pending_permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
        launch: &AcpLaunch,
    ) -> Self {
        Self {
            transport: RpcTransport::new(stdout, stdin),
            sink,
            assistant_text: String::new(),
            repository_id: launch.repository_id.clone(),
            permission_mode: launch.permission_mode,
            pending_permission,
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let mut incoming = RuntimeIncoming {
            sink: &self.sink,
            assistant_text: &mut self.assistant_text,
            repository_id: &self.repository_id,
            permission_mode: self.permission_mode,
            pending_permission: &self.pending_permission,
        };
        self.transport.request(method, params, &mut incoming).await
    }

    async fn receive(&mut self) -> Result<bool> {
        let mut incoming = RuntimeIncoming {
            sink: &self.sink,
            assistant_text: &mut self.assistant_text,
            repository_id: &self.repository_id,
            permission_mode: self.permission_mode,
            pending_permission: &self.pending_permission,
        };
        self.transport.receive(&mut incoming).await
    }
}

struct RuntimeIncoming<'a> {
    sink: &'a EventSink,
    assistant_text: &'a mut String,
    repository_id: &'a str,
    permission_mode: PermissionMode,
    pending_permission: &'a Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

impl Incoming for RuntimeIncoming<'_> {
    async fn handle(&mut self, message: Value) -> Result<Option<Value>> {
        match message.get("method").and_then(Value::as_str) {
            Some("session/update") => {
                self.handle_update(&message["params"]).await?;
                Ok(None)
            }
            Some("session/request_permission") if message.get("id").is_some() => {
                self.handle_permission(&message).await.map(Some)
            }
            Some(_) if message.get("id").is_some() => Ok(Some(json!({
                "jsonrpc": "2.0",
                "id": message["id"],
                "error": {"code": -32601, "message": "method not supported"},
            }))),
            _ => Ok(None),
        }
    }
}

impl RuntimeIncoming<'_> {
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

    async fn handle_permission(&mut self, message: &Value) -> Result<Value> {
        let params = &message["params"];
        let session_id = params.get("sessionId").cloned().unwrap_or(Value::Null);
        let mut payload = tool_payload(session_id.clone(), &params["toolCall"]);
        payload["options"] = params.get("options").cloned().unwrap_or_default();
        let signature = permission_signature(&params["toolCall"]);
        let learned = self.permission_mode == PermissionMode::Learn
            && self
                .sink
                .runtime
                .inner
                .store
                .has_learned_permission(self.repository_id, &signature.tool_name, &signature.kind)
                .await
                .unwrap_or(false);
        // The input path must see a waiting receiver as soon as the request
        // reaches the console stream. Register it before emitting the event,
        // rather than leaving a gap where input would become a new prompt.
        let waiting = matches!(self.permission_mode, PermissionMode::Ask)
            || (self.permission_mode == PermissionMode::Learn && !learned);
        let receiver = waiting.then(|| self.begin_permission());
        self.sink.emit("permission_request", payload).await;
        let selected = match receiver {
            Some(receiver) => self.wait_for_permission(params, receiver).await?,
            None => approved_option(params),
        };
        if self.permission_mode == PermissionMode::Learn
            && selected
                .as_deref()
                .is_some_and(|option| allowing_option(params, option))
        {
            self.sink
                .runtime
                .inner
                .store
                .learn_permission(self.repository_id, &signature.tool_name, &signature.kind)
                .await
                .map_err(|error| anyhow!("remembering ACP permission approval: {error}"))?;
        }
        let outcome = selected.as_ref().map_or_else(
            || json!({"outcome": "cancelled"}),
            |option_id| json!({"outcome": "selected", "optionId": option_id}),
        );
        self.sink
            .emit(
                "permission.replied",
                json!({"session_id": session_id, "option_id": selected}),
            )
            .await;
        Ok(json!({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {"outcome": outcome},
        }))
    }

    /// Make the input path answer the permission request now visible to the
    /// console. There is one outstanding ACP request per session.
    fn begin_permission(&self) -> oneshot::Receiver<String> {
        let (sender, receiver) = oneshot::channel();
        *self.pending_permission.lock().expect("ACP permission lock") = Some(sender);
        receiver
    }

    /// Wait for the console answer after its request was published.
    async fn wait_for_permission(
        &self,
        params: &Value,
        receiver: oneshot::Receiver<String>,
    ) -> Result<Option<String>> {
        let answer = receiver
            .await
            .map_err(|_| anyhow!("ACP permission answer channel closed"))?;
        Ok(permission_option(params, &answer))
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
            received = rpc.receive() => {
                if !received? {
                    return Ok(());
                }
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
    let option = find_config_option(&options, categories, names)
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

/// Find a session configuration option by its ACP category, then by the
/// identifying names agents used before categories were consistently set.
pub(crate) fn find_config_option<'a>(
    options: &'a [Value],
    categories: &[&str],
    names: &[&str],
) -> Option<&'a Value> {
    options
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

/// A remembered permission is deliberately narrow: the ACP tool's human
/// name and kind, and the repository the request came from.
struct PermissionSignature {
    tool_name: String,
    kind: String,
}

fn permission_signature(tool_call: &Value) -> PermissionSignature {
    let tool_name = tool_call
        .get("title")
        .or_else(|| tool_call.get("toolCallId"))
        .and_then(Value::as_str)
        .unwrap_or("ACP tool")
        .to_string();
    let kind = tool_call
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    PermissionSignature { tool_name, kind }
}

/// Resolve a console answer to an option. Option ids are the stable answer
/// the API exposes; names make a terminal reply readable too. An unknown
/// answer cancels the request, which is a denial and is never learned.
fn permission_option(params: &Value, answer: &str) -> Option<String> {
    params
        .get("options")
        .and_then(Value::as_array)?
        .iter()
        .find(|option| {
            ["optionId", "name"].into_iter().any(|key| {
                option
                    .get(key)
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.eq_ignore_ascii_case(answer))
            })
        })?
        .get("optionId")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Whether the selected option is an approval, rather than a denial.
fn allowing_option(params: &Value, option_id: &str) -> bool {
    params
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|option| option.get("optionId").and_then(Value::as_str) == Some(option_id))
        .and_then(|option| option.get("kind").and_then(Value::as_str))
        .is_some_and(|kind| kind.starts_with("allow"))
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
