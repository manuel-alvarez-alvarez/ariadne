//! The ACP runtime: the daemon's own client for agents that speak the Agent
//! Client Protocol.
//!
//! Every session is a child process of the daemon itself: spawned with piped
//! standard input and output, driven over newline-delimited JSON-RPC (ACP
//! version 1), reaped when it exits, and killed when its session is killed.
//!
//! What the agent does is reported through the one ingestion path
//! (`crate::http::events::ingest_event`), in the runtime's own event
//! vocabulary, which is what moves a session's status, attention and internal
//! id.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, oneshot};

use ariadne_api::events::{AgentEventDto, IngestEventRequest};
use ariadne_core::acp::LaunchConfig;
use ariadne_core::id::new_id;
use ariadne_core::{PermissionMode, TokenUsage};
use ariadne_store::Store;

use crate::acp_rpc::{Incoming, Outbound, RpcTransport};
use crate::http::classify::summarize;
use crate::http::events::ingest_event;
use crate::scheduler::SchedEvent;

/// Live console events buffered per subscriber before it is told to resync.
const CONSOLE_CAPACITY: usize = 1024;

/// Everything one launch of an ACP agent is made of. The launcher builds it
/// from the adapter's spawn plan: the registry command with the planned
/// flags behind it, the environment as planned, and the launch file as the
/// protocol's half.
pub struct AcpLaunch {
    pub session_id: String,
    /// The launch every event of this process reports under.
    pub launch_id: String,
    /// The executable to spawn: the head of the registry command of the
    /// agent the session's pin names.
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

/// The daemon-owned ACP agents, one child process per live session.
///
/// Cheap to clone: every clone shares the one registry, which is what lets a
/// driver task deregister itself and the launcher ask who is alive. There is
/// no "could not be asked" — the registry always answers, and a daemon
/// restart answers "no" for every child of the daemon that died.
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
    /// The live-only events — message and thought chunks, tool call
    /// progress — on their way to the console streams and nowhere else: none
    /// of them is stored, and none reaches the domain bus. One channel per
    /// session, so a chatty session never lags another session's console,
    /// kept for as long as an agent runs for it or somebody listens.
    consoles: Mutex<HashMap<String, broadcast::Sender<AgentEventDto>>>,
}

/// Where a prompt came from, as `user_prompt_submit` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptSource {
    /// Typed into the session's console.
    Console,
    /// Everything the daemon itself says: the briefing, a nudge, a message.
    Daemon,
}

impl PromptSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Daemon => "daemon",
        }
    }
}

/// One `session/prompt` waiting its turn.
struct Prompt {
    text: String,
    source: PromptSource,
}

/// The turn in flight, as far as the agent has told it: the text so far, and
/// every tool call still open, merged per `toolCallId` from the updates the
/// agent sent about it.
#[derive(Default)]
struct Turn {
    running: bool,
    /// The agent's own session id, as the turn's events name it.
    agent_session: Option<String>,
    thought: String,
    message: String,
    tools: HashMap<String, Value>,
}

impl Turn {
    /// The running turn's text so far as the two chunk events a console
    /// snapshot appends — none for a turn that has said nothing yet, and none
    /// between turns.
    fn so_far(&self, session_id: &str, task_id: &Option<String>) -> Vec<AgentEventDto> {
        if !self.running {
            return Vec::new();
        }
        let agent_session = self
            .agent_session
            .clone()
            .map_or(Value::Null, Value::String);
        [
            ("agent_thought_chunk", &self.thought),
            ("agent_message_chunk", &self.message),
        ]
        .into_iter()
        .filter(|(_, text)| !text.is_empty())
        .map(|(kind, text)| {
            live_event(
                session_id,
                task_id.clone(),
                kind,
                json!({"session_id": agent_session, "text": text}),
            )
        })
        .collect()
    }
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
    prompts: mpsc::UnboundedSender<Prompt>,
    /// The reply channel while the agent waits on one permission request.
    permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
    /// The turn in flight, shared with the driver that fills it.
    turn: Arc<tokio::sync::Mutex<Turn>>,
    /// The task the session works on, copied onto every live event.
    task_id: Option<String>,
    /// The notification path into the agent, for `session/cancel`.
    outbound: Outbound,
}

/// The transport, the permission reply slot and the turn a driver owns for
/// one child.
struct DriverIo {
    transport: RpcTransport,
    permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
    turn: Arc<tokio::sync::Mutex<Turn>>,
    task_id: Option<String>,
}

impl AcpRuntime {
    pub fn new(store: Store) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                running: Mutex::new(HashMap::new()),
                scheduler: OnceLock::new(),
                consoles: Mutex::new(HashMap::new()),
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
    /// session.
    pub fn send_prompt(&self, session_id: &str, text: String) -> Result<()> {
        self.inner
            .running
            .lock()
            .expect("acp registry lock")
            .get(session_id)
            .ok_or_else(|| anyhow!("no ACP agent is running for session {session_id}"))?
            .prompts
            .send(Prompt {
                text,
                source: PromptSource::Daemon,
            })
            .map_err(|_| anyhow!("the ACP agent for session {session_id} is no longer listening"))
    }

    /// Hand the running agent console input. A pending permission consumes it
    /// as an option answer; otherwise it becomes a prompt as before.
    ///
    /// Errs where there is nobody here to hear it: no agent runs for this
    /// session.
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
            .send(Prompt {
                text,
                source: PromptSource::Console,
            })
            .map_err(|_| anyhow!("the ACP agent for session {session_id} is no longer listening"))
    }

    /// Cancel the turn in flight: ACP `session/cancel`, sent while the
    /// `session/prompt` it interrupts is still outstanding, whose response
    /// then ends the turn with `stopReason: cancelled`.
    ///
    /// Errs where there is nothing to cancel: no agent runs for this session,
    /// or the agent is between turns. The agent's stdin is held from the
    /// check to the send: a turn that ends meanwhile cannot have its
    /// successor's prompt written first, so the cancel never lands on the
    /// next turn. The turn lock itself is taken only for the check, so a
    /// child that stops reading its stdin stalls this call and not the
    /// driver.
    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        let (turn, outbound) = {
            let running = self.inner.running.lock().expect("acp registry lock");
            let agent = running
                .get(session_id)
                .ok_or_else(|| anyhow!("no ACP agent is running for session {session_id}"))?;
            (agent.turn.clone(), agent.outbound.clone())
        };
        let mut pipe = outbound.lock().await;
        let agent_session = {
            let turn = turn.lock().await;
            turn.running.then(|| turn.agent_session.clone()).flatten()
        };
        let Some(agent_session) = agent_session else {
            bail!("no turn is running for session {session_id}");
        };
        pipe.notify("session/cancel", json!({"sessionId": agent_session}))
            .await
    }

    /// The running turn's text so far, as the chunk events a console snapshot
    /// appends after the stored ones; empty between turns, and for a session
    /// with no agent here.
    pub async fn turn_so_far(&self, session_id: &str) -> Vec<AgentEventDto> {
        let Some((turn, task_id)) = self.turn_of(session_id) else {
            return Vec::new();
        };
        turn.lock().await.so_far(session_id, &task_id)
    }

    /// Follow the live console events, and read the running turn's text so
    /// far under the same lock the driver appends and publishes under: every
    /// chunk is then either in the text returned or on the subscription, and
    /// never in both.
    pub async fn subscribe_console(
        &self,
        session_id: &str,
    ) -> (broadcast::Receiver<AgentEventDto>, Vec<AgentEventDto>) {
        let Some((turn, task_id)) = self.turn_of(session_id) else {
            return (self.console_of(session_id).subscribe(), Vec::new());
        };
        let turn = turn.lock().await;
        let rx = self.console_of(session_id).subscribe();
        (rx, turn.so_far(session_id, &task_id))
    }

    /// The session's live console channel, made on first use — by a launch
    /// or by a subscriber, whichever comes first, so a console opened before
    /// the agent is up still hears its first turn.
    ///
    /// Each channel holds its whole buffer from the start, so the ones
    /// nobody needs any more — no agent running, no console listening — are
    /// pruned here, on the way to making the next one.
    fn console_of(&self, session_id: &str) -> broadcast::Sender<AgentEventDto> {
        let running = self.inner.running.lock().expect("acp registry lock");
        let mut consoles = self.inner.consoles.lock().expect("acp console lock");
        consoles.retain(|session, console| {
            console.receiver_count() > 0 || running.contains_key(session)
        });
        consoles
            .entry(session_id.to_string())
            .or_insert_with(|| broadcast::Sender::new(CONSOLE_CAPACITY))
            .clone()
    }

    fn turn_of(&self, session_id: &str) -> Option<(Arc<tokio::sync::Mutex<Turn>>, Option<String>)> {
        self.inner
            .running
            .lock()
            .expect("acp registry lock")
            .get(session_id)
            .map(|agent| (agent.turn.clone(), agent.task_id.clone()))
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
        let turn = Arc::new(tokio::sync::Mutex::new(Turn::default()));
        let task_id = self
            .inner
            .store
            .get_session(&launch.session_id)
            .await
            .ok()
            .and_then(|session| session.task_id);
        let transport = RpcTransport::new(stdout, stdin);
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
                    turn: turn.clone(),
                    task_id: task_id.clone(),
                    outbound: transport.outbound(),
                },
            );
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime
                .drive(
                    launch,
                    child,
                    DriverIo {
                        transport,
                        permission,
                        turn,
                        task_id,
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
        // The console channel goes with the last agent, once nobody listens;
        // a console still open keeps it for the agent that comes next.
        let mut consoles = self.inner.consoles.lock().expect("acp console lock");
        if consoles
            .get(session_id)
            .is_some_and(|console| console.receiver_count() == 0)
        {
            consoles.remove(session_id);
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
        prompts: mpsc::UnboundedReceiver<Prompt>,
    ) {
        let sink = EventSink {
            runtime: self.clone(),
            session_id: launch.session_id.clone(),
            launch_id: launch.launch_id.clone(),
            task_id: io.task_id,
            agent_session: Arc::new(OnceLock::new()),
            console: self.console_of(&launch.session_id),
        };
        let mut rpc = Rpc::new(io.transport, sink.clone(), io.permission, io.turn, &launch);
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

/// Where the driver reports what its agent does: the one ingestion path
/// every event takes into the store, and the live path the chunks take to
/// the console streams alone.
#[derive(Clone)]
struct EventSink {
    runtime: AcpRuntime,
    session_id: String,
    launch_id: String,
    task_id: Option<String>,
    /// The agent's own session id, once setup has learned it: what every
    /// payload names as `session_id`, and what the ingestion records on the
    /// row.
    agent_session: Arc<OnceLock<String>>,
    /// The session's live console channel.
    console: broadcast::Sender<AgentEventDto>,
}

/// A live-only event as the console streams frame it: an id and a timestamp
/// of its own, and the same summary a stored event gets, so a client reads
/// both the same way.
fn live_event(
    session_id: &str,
    task_id: Option<String>,
    kind: &str,
    payload: Value,
) -> AgentEventDto {
    AgentEventDto {
        id: new_id(),
        session_id: Some(session_id.to_string()),
        task_id,
        kind: kind.to_string(),
        summary: summarize(kind, &payload),
        payload,
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    }
}

impl EventSink {
    /// Publish a live-only event to the console streams. Nothing is stored,
    /// nothing wakes the scheduler, and nobody listening costs nothing.
    fn emit_live(&self, kind: &str, payload: Value) {
        let _ = self.console.send(live_event(
            &self.session_id,
            self.task_id.clone(),
            kind,
            payload,
        ));
    }

    /// Event reporting is fail-safe: an event that cannot be recorded costs
    /// the record, never the agent.
    async fn emit(&self, kind: &str, payload: Value) {
        let request = IngestEventRequest {
            session_id: self.session_id.clone(),
            launch: Some(self.launch_id.clone()),
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
    turn: Arc<tokio::sync::Mutex<Turn>>,
    repository_id: String,
    permission_mode: PermissionMode,
    pending_permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

impl Rpc {
    fn new(
        transport: RpcTransport,
        sink: EventSink,
        pending_permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
        turn: Arc<tokio::sync::Mutex<Turn>>,
        launch: &AcpLaunch,
    ) -> Self {
        Self {
            transport,
            sink,
            turn,
            repository_id: launch.repository_id.clone(),
            permission_mode: launch.permission_mode,
            pending_permission,
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let mut incoming = RuntimeIncoming {
            sink: &self.sink,
            turn: &self.turn,
            repository_id: &self.repository_id,
            permission_mode: self.permission_mode,
            pending_permission: &self.pending_permission,
        };
        self.transport.request(method, params, &mut incoming).await
    }

    async fn receive(&mut self) -> Result<bool> {
        let mut incoming = RuntimeIncoming {
            sink: &self.sink,
            turn: &self.turn,
            repository_id: &self.repository_id,
            permission_mode: self.permission_mode,
            pending_permission: &self.pending_permission,
        };
        self.transport.receive(&mut incoming).await
    }
}

struct RuntimeIncoming<'a> {
    sink: &'a EventSink,
    turn: &'a Arc<tokio::sync::Mutex<Turn>>,
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
            // The text so far is appended and its chunk published under the
            // one lock, so a console opening mid-turn reads a text that ends
            // exactly where its subscription begins.
            Some(kind @ ("agent_message_chunk" | "agent_thought_chunk")) => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    let mut turn = self.turn.lock().await;
                    match kind {
                        "agent_thought_chunk" => turn.thought.push_str(text),
                        _ => turn.message.push_str(text),
                    }
                    self.sink
                        .emit_live(kind, json!({"session_id": session_id, "text": text}));
                }
            }
            Some("plan") => {
                self.sink
                    .emit(
                        "plan",
                        json!({"session_id": session_id, "entries": update.get("entries")}),
                    )
                    .await;
            }
            Some("tool_call") => {
                let (id, call) = tool_call_of(&update);
                self.turn.lock().await.tools.insert(id, call.clone());
                self.sink
                    .emit("pre_tool_use", tool_payload(session_id, &call))
                    .await;
            }
            Some("tool_call_update") => {
                let (id, update) = tool_call_of(&update);
                let terminal = terminal_tool_status(&update);
                let merged = {
                    let mut turn = self.turn.lock().await;
                    let call = turn
                        .tools
                        .entry(id.clone())
                        .or_insert_with(|| json!({"toolCallId": id}));
                    merge_tool_call(call, &update);
                    match terminal {
                        true => turn.tools.remove(&id).unwrap_or_default(),
                        false => call.clone(),
                    }
                };
                if terminal {
                    self.sink
                        .emit("post_tool_use", tool_payload(session_id, &merged))
                        .await;
                } else {
                    self.sink.emit_live(
                        "tool_call_update",
                        json!({"session_id": session_id, "tool_call_id": id, "acp": merged}),
                    );
                }
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
    prompts: mpsc::UnboundedReceiver<Prompt>,
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
        let prompt = Prompt {
            text: prompt.to_string(),
            source: PromptSource::Daemon,
        };
        prompt_once(rpc, &session_id, &config.system_prompt, &prompt).await?;
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
    mut prompts: mpsc::UnboundedReceiver<Prompt>,
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

/// One turn: the prompt out, the turn's chunks as they come, and — once the
/// response is in — the whole thought and message text, stored once each,
/// then the `stop`.
async fn prompt_once(
    rpc: &mut Rpc,
    session_id: &str,
    system_prompt: &str,
    prompt: &Prompt,
) -> Result<()> {
    let full = format!("{system_prompt}\n\n{}", prompt.text);
    {
        let mut turn = rpc.turn.lock().await;
        turn.running = true;
        turn.agent_session = Some(session_id.to_string());
        turn.thought.clear();
        turn.message.clear();
        // A call the last turn left open is not this turn's to finish.
        turn.tools.clear();
    }
    rpc.sink
        .emit(
            "user_prompt_submit",
            json!({
                "session_id": session_id,
                "prompt": full,
                "text": prompt.text,
                "source": prompt.source.as_str(),
            }),
        )
        .await;
    let response = rpc
        .request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": full}],
            }),
        )
        .await?;
    let (thought, message) = {
        let mut turn = rpc.turn.lock().await;
        turn.running = false;
        (
            std::mem::take(&mut turn.thought),
            std::mem::take(&mut turn.message),
        )
    };
    for (kind, text) in [("agent_thought", thought), ("agent_message", message)] {
        if !text.is_empty() {
            rpc.sink
                .emit(kind, json!({"session_id": session_id, "text": text}))
                .await;
        }
    }
    let mut stop = json!({
        "session_id": session_id,
        "stop_reason": response.get("stopReason"),
    });
    if let Some(usage) = usage_for_prompt_response(&response) {
        stop["ariadne_usage"] = json!({
            "source": rpc.sink.launch_id,
            "input_tokens": usage.input_tokens,
            "cached_input_tokens": usage.cached_input_tokens,
            "output_tokens": usage.output_tokens,
        });
    }
    rpc.sink.emit("stop", stop).await;
    Ok(())
}

/// The cumulative token totals an ACP prompt response reports. Adapter quota
/// totals include subagents, so use them where their shape is complete.
fn usage_for_prompt_response(response: &Value) -> Option<TokenUsage> {
    response
        .pointer("/_meta/quota/token_count")
        .and_then(|usage| adapter_usage(usage, &["cachedInputTokens", "cachedWriteTokens"]))
        .or_else(|| {
            response
                .get("usage")
                .and_then(|usage| adapter_usage(usage, &["cachedReadTokens", "cachedWriteTokens"]))
        })
}

/// Map one ACP usage object into Ariadne's cache-inclusive counters.
fn adapter_usage(usage: &Value, cached_fields: &[&str]) -> Option<TokenUsage> {
    let input_tokens = usage.get("inputTokens").and_then(Value::as_u64)?;
    let output_tokens = usage.get("outputTokens").and_then(Value::as_u64)?;
    let mut cached_input_tokens = 0_u64;
    for field in cached_fields {
        cached_input_tokens = cached_input_tokens
            .checked_add(usage.get(*field).and_then(Value::as_u64).unwrap_or(0))?;
    }
    Some(TokenUsage {
        input_tokens: input_tokens.checked_add(cached_input_tokens)?,
        cached_input_tokens,
        output_tokens,
    })
}

/// A tool call update's id, and the update as a record of the call: what it
/// says about the call, without the `sessionUpdate` that said which kind of
/// message it came in.
fn tool_call_of(update: &Value) -> (String, Value) {
    let id = update
        .get("toolCallId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut call = update.clone();
    if let Some(fields) = call.as_object_mut() {
        fields.remove("sessionUpdate");
    }
    (id, call)
}

/// Fold one update into the call it is about: every field the update sets
/// replaces the one on record, `content` included — an ACP update carries the
/// whole collection, never a delta of it.
fn merge_tool_call(call: &mut Value, update: &Value) {
    let (Some(call), Some(update)) = (call.as_object_mut(), update.as_object()) else {
        return;
    };
    for (key, value) in update {
        if !value.is_null() {
            call.insert(key.clone(), value.clone());
        }
    }
}

fn tool_payload(session_id: Value, call: &Value) -> Value {
    let name = call
        .get("title")
        .or_else(|| call.get("toolCallId"))
        .cloned()
        .unwrap_or_else(|| Value::String("ACP tool".into()));
    json!({
        "session_id": session_id,
        "tool_name": name,
        "tool_input": call.get("rawInput").cloned().unwrap_or_default(),
        "acp": call,
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
    use super::{approved_option, usage_for_prompt_response};

    use ariadne_core::TokenUsage;
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

    /// The standard and quota shapes name cached input differently, and the
    /// quota figure wins because it includes the adapter's subagents.
    #[test]
    fn prompt_usage_maps_each_adapter_shape_and_prefers_quota() {
        let standard = json!({"usage": {
            "inputTokens": 10,
            "cachedReadTokens": 20,
            "cachedWriteTokens": 30,
            "outputTokens": 40,
        }});
        assert_eq!(
            usage_for_prompt_response(&standard),
            Some(TokenUsage {
                input_tokens: 60,
                cached_input_tokens: 50,
                output_tokens: 40,
            })
        );

        let quota = json!({
            "usage": {"inputTokens": 100, "outputTokens": 200},
            "_meta": {"quota": {"token_count": {
                "inputTokens": 4,
                "cachedInputTokens": 5,
                "cachedWriteTokens": 6,
                "outputTokens": 7,
            }}},
        });
        assert_eq!(
            usage_for_prompt_response(&quota),
            Some(TokenUsage {
                input_tokens: 15,
                cached_input_tokens: 11,
                output_tokens: 7,
            })
        );

        let malformed_quota = json!({
            "usage": {"inputTokens": 8, "outputTokens": 9},
            "_meta": {"quota": {"token_count": {"inputTokens": "bad"}}},
        });
        assert_eq!(
            usage_for_prompt_response(&malformed_quota),
            Some(TokenUsage {
                input_tokens: 8,
                cached_input_tokens: 0,
                output_tokens: 9,
            })
        );
        assert_eq!(usage_for_prompt_response(&json!({})), None);
    }
}
