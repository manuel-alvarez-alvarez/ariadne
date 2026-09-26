//! The ACP runtime: the daemon's own client for agents that speak the Agent
//! Client Protocol.
//!
//! Every session is a child process of the daemon itself: spawned with piped
//! standard input and output, driven over ACP version 1, reaped when it
//! exits, and killed when its session is killed.
//!
//! The wire is the protocol's own Rust SDK (`agent-client-protocol`). The
//! daemon keeps the process — it spawns, signals and reaps it — and the SDK
//! only ever sees its two pipes (`crate::acp_transport`).
//!
//! What the agent does is reported through the one ingestion path
//! (`crate::http::events::ingest_event`), in the runtime's own event
//! vocabulary, which is what moves a session's status, attention and internal
//! id.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use agent_client_protocol::schema::{ProtocolVersion, v1};
use agent_client_protocol::{Agent, Client, ConnectionTo, JsonRpcNotification, JsonRpcRequest};
use anyhow::{Context, Result, anyhow, bail};
use futures_util::FutureExt;
use futures_util::future::Shared;
use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Notify, broadcast, mpsc, oneshot};

use ariadne_api::events::{AgentEventDto, IngestEventRequest};
use ariadne_core::acp::LaunchConfig;
use ariadne_core::id::new_id;
use ariadne_core::{PermissionMode, TokenUsage};
use ariadne_store::Store;

use crate::acp_calls::PromptTurn;
use crate::acp_transport::pipes;
use crate::http::classify::summarize;
use crate::http::events::ingest_event;
use crate::laya::Laya;
use crate::laya::decide::{Decision, decide};
use crate::scheduler::SchedEvent;
use crate::timeouts::Timeouts;
use crate::transcript::{LaunchTranscript, TranscriptHomes};

/// Live console events buffered per subscriber before it is told to resync.
const CONSOLE_CAPACITY: usize = 1024;

/// What one launch reports of its turns as they go, to whoever waits on the
/// agent's own word — the daemon ending the turn an author asked for its
/// review in (004), once the agent says it holds the answer. Each launch
/// reports on channels of its own: a report never comes from the process
/// before, and none is ever dropped on the way — a follower's channel is
/// unbounded, and a follower lives only until it has what it waited for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnReport {
    /// A tool call ended, by the name the agent gave it — its title, else
    /// its id — as `post_tool_use` names it.
    ToolEnded(String),
    /// The turn ended, its `stop` recorded.
    TurnEnded,
}

/// Whoever follows one launch's turn reports, each on an unbounded channel
/// of their own; a follower that has gone is dropped at the next report.
type Followers = Arc<Mutex<Vec<mpsc::UnboundedSender<TurnReport>>>>;

/// Hand a report to every follower still listening.
fn report(followers: &Followers, report: TurnReport) {
    followers
        .lock()
        .expect("turn report followers lock")
        .retain(|follower| follower.send(report.clone()).is_ok());
}

const CODEX_AGENT_ID: &str = "codex-acp";
/// The codex-acp mode every session starts in. Its default mode, `agent`,
/// hands each approval to Codex's guardian sub-agent, a model call that
/// judges the action; `agent-full-access` never asks for approval at all.
/// The mode, not `features.guardian_approval`, is what picks the reviewer.
const CODEX_INITIAL_AGENT_MODE: &str = "agent-full-access";

/// Apply the environment Ariadne owns for one registry agent process.
pub(crate) fn apply_agent_launch_environment(command: &mut Command, agent_id: &str) {
    if agent_id == CODEX_AGENT_ID {
        command.env("INITIAL_AGENT_MODE", CODEX_INITIAL_AGENT_MODE);
    }
}

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
    /// The registry id of the agent being launched.
    pub agent_id: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub config: LaunchConfig,
    /// Repository this session works in. Learned approvals are scoped here.
    pub repository_id: String,
    /// The repository's permission mode, resolved before the launch.
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
    timeouts: Timeouts,
    /// Where the agents write the transcripts a launch reads its usage from.
    transcripts: TranscriptHomes,
    /// Live agents by Ariadne session id.
    running: Mutex<HashMap<String, RunningAgent>>,
    /// Sessions the user reopened after their work ended.
    user_resumed: Mutex<HashSet<String>>,
    /// Agents taken down whose driver has not reaped them yet, by Ariadne
    /// session id, each beside its launch id. A launch waits on these as
    /// well as on the agent it takes down itself: a kill followed by a
    /// relaunch finds no running entry left to take down, and the old agent
    /// still holds the conversation the relaunch resumes.
    ending: Mutex<HashMap<String, Vec<(String, Reaped)>>>,
    /// Wakes the scheduler after an event lands, the way the HTTP ingestion
    /// does — present once a scheduler is running.
    scheduler: OnceLock<mpsc::UnboundedSender<SchedEvent>>,
    /// The live-only events — message and thought chunks, tool call
    /// progress — on their way to the console streams and nowhere else: none
    /// of them is stored, and none reaches the domain bus. One channel per
    /// session, so a chatty session never lags another session's console,
    /// kept for as long as an agent runs for it or somebody listens.
    consoles: Mutex<HashMap<String, broadcast::Sender<AgentEventDto>>>,
    laya: Option<Laya>,
}

/// Resolves once a driver has killed and reaped its child. Shared, so that
/// every launch of the session waits on the same reap.
type Reaped = Shared<oneshot::Receiver<()>>;

/// Where a prompt came from, as `user_prompt_submit` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptSource {
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

/// One run of text: the chunks of one kind, and of one message where the
/// agent names its messages, in a row.
struct Run {
    /// `agent_thought` or `agent_message`: the kind the run is stored as.
    kind: &'static str,
    /// The ACP `messageId` its chunks carry, where they carry one.
    message_id: Option<String>,
    text: String,
}

impl Run {
    /// Whether a chunk of this kind and message goes on with this run. A
    /// chunk that names no message, or a run that named none, cannot be told
    /// apart, and goes on with it.
    fn continues(&self, kind: &str, message_id: Option<&str>) -> bool {
        self.kind == kind
            && match (self.message_id.as_deref(), message_id) {
                (Some(run), Some(chunk)) => run == chunk,
                _ => true,
            }
    }
}

/// The turn in flight, as far as the agent has told it: the run of text it
/// is writing, and every tool call still open, merged per `toolCallId` from
/// the updates the agent sent about it.
#[derive(Default)]
struct Turn {
    /// A load replay: keep the first transcript, skip later copies.
    replay: Option<bool>,
    running: bool,
    /// The agent's own session id, as the turn's events name it.
    agent_session: Option<String>,
    /// The run of chunks being written. The next thing the agent reports
    /// ends it — or a chunk of another kind or another message — and it is
    /// stored where it stands (021).
    text: Option<Run>,
    tools: HashMap<String, Value>,
}

impl Turn {
    /// The running turn's text so far as the chunk event a console snapshot
    /// appends: the run still being written, which comes after everything
    /// stored. None for a turn that is not writing text, and none between
    /// turns.
    fn so_far(&self, session_id: &str, task_id: &Option<String>) -> Vec<AgentEventDto> {
        let Some(run) = self.text.as_ref().filter(|_| self.running) else {
            return Vec::new();
        };
        vec![live_event(
            session_id,
            task_id.clone(),
            &format!("{}_chunk", run.kind),
            json!({"session_id": self.agent_session, "text": run.text}),
        )]
    }

    /// End the run of text being written, and hand it back as the event to
    /// store: `agent_thought` or `agent_message`, `{session_id, text}`. Text
    /// that came between turns belongs to no turn and is dropped.
    fn end_text(&mut self) -> Option<(&'static str, Value)> {
        let Run { kind, text, .. } = self.text.take()?;
        (self.running && !text.is_empty()).then(|| {
            (
                kind,
                json!({"session_id": self.agent_session, "text": text}),
            )
        })
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
    /// This launch's turn reports, for [`AcpRuntime::turn_reports`].
    reports: Followers,
    /// Closes once the driver has killed and reaped the child: what a
    /// relaunch waits on, so that two agents never serve one conversation.
    ended: Reaped,
}

/// The way into a running agent for the one notification sent from outside
/// its driver: `session/cancel`.
///
/// The connection is not there when the agent is registered — the driver
/// makes it — so it arrives in a cell the driver fills, and a cancel before
/// that simply finds no turn to cancel.
///
/// `sending` is what the agent's stdin lock used to be. A cancel must not be
/// written between the moment a turn is seen running and the moment it goes
/// out: the turn could end in that gap and a queued prompt start, and the
/// cancel would then land on a turn nobody asked to cancel. Every
/// `session/prompt` the driver sends takes the same lock, so no prompt can
/// start inside a cancel, and no cancel inside a prompt.
#[derive(Clone, Default)]
struct Outbound {
    connection: Arc<tokio::sync::OnceCell<ConnectionTo<Agent>>>,
    sending: Arc<tokio::sync::Mutex<()>>,
}

impl Outbound {
    /// Cancel whichever turn `turn` says is running. Answers whether a
    /// cancel went out, which is false when the agent is not connected yet
    /// or is between turns.
    async fn cancel(&self, turn: &tokio::sync::Mutex<Turn>) -> Result<bool> {
        let _sending = self.sending.lock().await;
        let Some(connection) = self.connection.get() else {
            return Ok(false);
        };
        let agent_session = {
            let turn = turn.lock().await;
            turn.running.then(|| turn.agent_session.clone()).flatten()
        };
        let Some(agent_session) = agent_session else {
            return Ok(false);
        };
        connection.send_notification(v1::CancelNotification::new(agent_session))?;
        Ok(true)
    }
}

/// The transport, the permission reply slot, the turn and the report channel
/// a driver owns for one child.
struct DriverIo {
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    outbound: Outbound,
    permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
    turn: Arc<tokio::sync::Mutex<Turn>>,
    task_id: Option<String>,
    reports: Followers,
    ready: Option<oneshot::Sender<()>>,
}

impl AcpRuntime {
    pub fn new(store: Store) -> Self {
        Self::with_timeouts(store, Timeouts::default())
    }

    /// A runtime that waits on its agents as long as `timeouts` says.
    pub(crate) fn with_timeouts(store: Store, timeouts: Timeouts) -> Self {
        Self::with_transcripts(store, timeouts, TranscriptHomes::from_env())
    }

    /// A runtime that also reads its agents' transcripts under `transcripts`
    /// rather than where the daemon's environment puts them.
    pub fn with_transcripts(
        store: Store,
        timeouts: Timeouts,
        transcripts: TranscriptHomes,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                timeouts,
                transcripts,
                running: Mutex::new(HashMap::new()),
                user_resumed: Mutex::default(),
                ending: Mutex::new(HashMap::new()),
                scheduler: OnceLock::new(),
                consoles: Mutex::new(HashMap::new()),
                laya: None,
            }),
        }
    }

    /// Give this runtime the daemon's Laya service before it is shared.
    pub fn with_laya(mut self, laya: Laya) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("a new ACP runtime has one owner")
            .laya = Some(laya);
        self
    }

    pub(crate) fn preserve_user_resume(&self, session_id: &str) {
        self.inner
            .user_resumed
            .lock()
            .expect("user resumes lock")
            .insert(session_id.to_string());
    }

    pub(crate) fn is_user_resumed(&self, session_id: &str) -> bool {
        self.inner
            .user_resumed
            .lock()
            .expect("user resumes lock")
            .contains(session_id)
    }

    /// Give the runtime the scheduler's waker. Called once, from the
    /// scheduler's own start.
    pub(crate) fn connect_scheduler(&self, tx: mpsc::UnboundedSender<SchedEvent>) {
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
    pub(crate) fn send_prompt(&self, session_id: &str, text: String) -> Result<()> {
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

    /// Test support: leave this session's registry entry exactly as it is —
    /// `is_running` still answers true — but replace its prompt channel with
    /// one whose receiving half is already dropped, so the next
    /// [`Self::send_prompt`] fails the way it does in the real window
    /// between a driver's own receiver dropping (its connection to the
    /// agent ending) and `deregister` removing the entry a few awaits
    /// later. A caller of `send_prompt` has no way to tell the two apart,
    /// which is the point: this reproduces the failure without needing the
    /// real window's timing.
    pub fn close_prompt_channel_for_test(&self, session_id: &str) {
        let (closed, unread) = mpsc::unbounded_channel();
        drop(unread);
        if let Some(agent) = self
            .inner
            .running
            .lock()
            .expect("acp registry lock")
            .get_mut(session_id)
        {
            agent.prompts = closed;
        }
    }

    /// Hand the running agent console input. A pending permission consumes it
    /// as an option answer; otherwise it becomes a prompt as before.
    ///
    /// Errs where there is nobody here to hear it: no agent runs for this
    /// session.
    pub(crate) fn send_input(&self, session_id: &str, text: String) -> Result<()> {
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
    pub(crate) async fn cancel(&self, session_id: &str) -> Result<()> {
        self.cancel_turn(session_id, None).await
    }

    /// [`Self::cancel`], but only while the session still runs under
    /// `launch_id`: a cancel decided on one launch never lands on the turn a
    /// relaunch since started.
    pub(crate) async fn cancel_launch(&self, session_id: &str, launch_id: &str) -> Result<()> {
        self.cancel_turn(session_id, Some(launch_id)).await
    }

    /// Follow the turn reports of the session's agent, as long as it still
    /// runs under `launch_id`: the reports of that launch and no other, from
    /// this moment on, every one of them — the channel is unbounded, so a
    /// follower that reads late reads them all.
    ///
    /// Errs where there is nothing to follow: no agent runs for this
    /// session, or the one that does is a relaunch.
    pub fn turn_reports(
        &self,
        session_id: &str,
        launch_id: &str,
    ) -> Result<mpsc::UnboundedReceiver<TurnReport>> {
        let running = self.inner.running.lock().expect("acp registry lock");
        let agent = running
            .get(session_id)
            .ok_or_else(|| anyhow!("no ACP agent is running for session {session_id}"))?;
        if agent.launch_id != launch_id {
            bail!("the ACP agent for session {session_id} has been relaunched since");
        }
        let (follower, reports) = mpsc::unbounded_channel();
        agent
            .reports
            .lock()
            .expect("turn report followers lock")
            .push(follower);
        Ok(reports)
    }

    async fn cancel_turn(&self, session_id: &str, of_launch: Option<&str>) -> Result<()> {
        let (turn, outbound) = {
            let running = self.inner.running.lock().expect("acp registry lock");
            let agent = running
                .get(session_id)
                .ok_or_else(|| anyhow!("no ACP agent is running for session {session_id}"))?;
            if of_launch.is_some_and(|launch| launch != agent.launch_id) {
                bail!("the ACP agent for session {session_id} has been relaunched since");
            }
            (agent.turn.clone(), agent.outbound.clone())
        };
        match outbound.cancel(&turn).await? {
            true => Ok(()),
            false => bail!("no turn is running for session {session_id}"),
        }
    }

    /// Follow the live console events, and read the running turn's text so
    /// far under the same lock the driver appends and publishes under: every
    /// chunk is then either in the text returned or on the subscription, and
    /// never in both.
    pub(crate) async fn subscribe_console(
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
    /// still holding this session is stopped first, and its child is gone
    /// before this one starts — whether this launch took it down or a kill
    /// before it did: one seat, one agent.
    ///
    /// The agent leads a process group of its own, which the kill takes
    /// whole (see [`Self::drive`]).
    pub async fn launch(&self, launch: AcpLaunch) -> Result<()> {
        self.take_down(&launch.session_id);
        for reaped in self.ending_for(&launch.session_id) {
            let _ = reaped.await;
        }
        let mut command = Command::new(&launch.program);
        command
            .args(&launch.args)
            .envs(launch.env.iter().cloned())
            .current_dir(&launch.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true);
        apply_agent_launch_environment(&mut command, &launch.agent_id);
        let mut child = command.spawn().with_context(|| {
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
        let (reaped, ended) = oneshot::channel();
        let (prompts, queued) = mpsc::unbounded_channel();
        let reports: Followers = Arc::default();
        let permission = Arc::new(Mutex::new(None));
        let turn = Arc::new(tokio::sync::Mutex::new(Turn::default()));
        let session = self.inner.store.get_session(&launch.session_id).await?;
        let loose = session.seat.is_none();
        let task_id = session.task_id;
        let outbound = Outbound::default();
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
                    outbound: outbound.clone(),
                    reports: reports.clone(),
                    ended: ended.shared(),
                },
            );
        let (ready, loaded) = oneshot::channel();
        let session_id = launch.session_id.clone();
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime
                .drive(
                    launch,
                    child,
                    DriverIo {
                        stdin,
                        stdout,
                        outbound,
                        permission,
                        turn,
                        task_id,
                        reports,
                        ready: loose.then_some(ready),
                    },
                    stopped,
                    queued,
                )
                .await;
            drop(reaped);
        });
        if loose {
            match tokio::time::timeout(self.inner.timeouts.session_load, loaded).await {
                Ok(Ok(())) => {}
                result => {
                    self.kill(&session_id);
                    bail!("loading the outside session failed: {result:?}");
                }
            }
        }
        Ok(())
    }

    /// Take a session's agent down. The entry goes at once — a spawn guard
    /// asking right after is told the seat is free — and the driver cancels
    /// the turn it is running, then kills and reaps the child behind it. A
    /// session with no agent here is a no-op.
    ///
    /// The kill does not wait for the reap; the next launch of the session
    /// does.
    pub(crate) fn kill(&self, session_id: &str) {
        self.take_down(session_id);
    }

    /// [`Self::kill`]: the agent moves from the running to the ending.
    fn take_down(&self, session_id: &str) {
        let Some(agent) = self
            .inner
            .running
            .lock()
            .expect("acp registry lock")
            .remove(session_id)
        else {
            return;
        };
        tracing::info!(session = %session_id, "killing the ACP agent");
        let _ = agent.stop.send(());
        self.inner
            .ending
            .lock()
            .expect("acp ending lock")
            .entry(session_id.to_string())
            .or_default()
            .push((agent.launch_id, agent.ended));
    }

    /// What resolves once every agent taken down for this session is reaped.
    fn ending_for(&self, session_id: &str) -> Vec<Reaped> {
        self.inner
            .ending
            .lock()
            .expect("acp ending lock")
            .get(session_id)
            .into_iter()
            .flatten()
            .map(|(_, reaped)| reaped.clone())
            .collect()
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
        // The driver has reaped its child by now: a launch that comes later
        // has nothing of this one's to wait on.
        {
            let mut ending = self.inner.ending.lock().expect("acp ending lock");
            if let Some(launches) = ending.get_mut(session_id) {
                launches.retain(|(ending_launch, _)| ending_launch != launch_id);
                if launches.is_empty() {
                    ending.remove(session_id);
                }
            }
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
    /// fails; on a kill, the running turn's cancel; then the kill and the
    /// reap; then the session's last words.
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
        let outbound = io.outbound.clone();
        let (turn, permission) = (io.turn.clone(), io.permission.clone());
        let closing = Arc::new(AtomicBool::new(false));
        let turn_ended = Arc::new(Notify::new());
        // What the connection's handlers need. They run on its dispatch
        // loop, one message at a time, which is the order the transport's own
        // loop gave them.
        let incoming = RuntimeIncoming {
            sink: sink.clone(),
            turn: turn.clone(),
            repository_id: launch.repository_id.clone(),
            repository: launch.cwd.clone(),
            permission_mode: launch.permission_mode,
            pending_permission: permission.clone(),
            reports: io.reports.clone(),
        };
        // The protocol's own outcome, set inside the connection: the
        // connection future answers for the link, not for the conversation.
        let mut protocol_outcome = None;
        let mut protocol = Box::pin(
            Client
                .builder()
                .name("ariadne")
                .on_receive_notification(
                    async |update: RawSessionUpdate, _cx| {
                        incoming.clone().handle_update(&update.0).await?;
                        Ok(())
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .on_receive_request(
                    async |request: RawPermissionRequest, responder, _cx| {
                        let answer = incoming.clone().handle_permission(&request.0).await?;
                        responder.respond(answer)
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .connect_with(pipes(io.stdin, io.stdout), async |cx| {
                    // Everything that sends from outside the driver waits on
                    // this: a cancel before it simply finds nothing running.
                    let _ = outbound.connection.set(cx.clone());
                    let mut rpc = Rpc::new(
                        cx,
                        outbound.clone(),
                        sink.clone(),
                        turn.clone(),
                        io.reports.clone(),
                        closing.clone(),
                        turn_ended.clone(),
                    );
                    protocol_outcome = Some(
                        run_protocol(&mut rpc, &launch.cwd, &launch.config, prompts, io.ready)
                            .await,
                    );
                    Ok(())
                }),
        );
        let mut ended = tokio::select! {
            result = &mut protocol => Some(result),
            _ = stopped => None,
        };
        if ended.is_none() {
            let ending = TurnEnding {
                turn: &turn,
                permission: &permission,
                outbound: &outbound,
                closing: &closing,
                turn_ended: &turn_ended,
                grace: self.inner.timeouts.cancel_grace,
            };
            ended = ending.settle(protocol.as_mut()).await;
        }
        // Whatever the protocol was in the middle of goes now, and every lock
        // it held with it — the store's event order among them, which the
        // session's last words below take again.
        drop(protocol);
        // The conversation's outcome where it reached one; otherwise the
        // link's, so a connection that failed before the protocol could say
        // anything is still reported.
        let outcome = match (protocol_outcome, ended) {
            (Some(outcome), _) => Some(outcome),
            (None, Some(Err(error))) => Some(Err(anyhow!("ACP connection failed: {error}"))),
            (None, _) => None,
        };
        // Reap on exit, kill on a kill: the signal is a no-op on a child
        // already gone, and the wait is what collects it either way. The
        // signal goes to the agent's whole process group: an adapter that
        // runs the agent in a process of its own — codex-acp's `codex
        // app-server` — otherwise leaves that process up past the reap, still
        // the writer of the conversation a relaunch is about to resume. The
        // child is not reaped yet, so its pid still names its group.
        if let Some(group) = child.id().and_then(|pid| Pid::from_raw(pid.cast_signed())) {
            let _ = kill_process_group(group, Signal::KILL);
        }
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

/// What a killed driver needs to let its running turn end the ACP way.
struct TurnEnding<'a> {
    turn: &'a tokio::sync::Mutex<Turn>,
    permission: &'a Mutex<Option<oneshot::Sender<String>>>,
    outbound: &'a Outbound,
    closing: &'a AtomicBool,
    turn_ended: &'a Notify,
    grace: Duration,
}

impl TurnEnding<'_> {
    /// Cancel the running turn and serve the agent until its response is in
    /// — the `stop` that carries what the turn spent — or the protocol ends,
    /// or `grace` ([`Timeouts::cancel_grace`]) runs out. The protocol's outcome where it
    /// ended here, `None` otherwise.
    ///
    /// No queued prompt starts meanwhile, and a turn waiting on a permission
    /// answer is not asked: nobody is left to answer it, and ACP has the
    /// client answer that request before the turn can end.
    async fn settle<T>(&self, mut protocol: Pin<&mut impl Future<Output = T>>) -> Option<T> {
        self.closing.store(true, Ordering::SeqCst);
        if self
            .permission
            .lock()
            .expect("ACP permission lock")
            .is_some()
        {
            return None;
        }
        let ended = self.turn_ended.notified();
        tokio::pin!(ended);
        ended.as_mut().enable();
        let cancelled = async { self.outbound.cancel(self.turn).await.unwrap_or(false) };
        let answered = async {
            if cancelled.await {
                ended.await;
            }
        };
        tokio::select! {
            result = &mut protocol => Some(result),
            _ = answered => None,
            _ = tokio::time::sleep(self.grace) => None,
        }
    }
}

/// One `session/update`, as the JSON the agent sent.
///
/// Not the SDK's typed `SessionUpdate`: that is a closed enum, so a kind it
/// does not know fails to parse, where the daemon has always read the kinds
/// it handles and let the rest by. What it stores is this JSON too — a tool
/// call reaches the console as the agent wrote it, extensions and all.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, JsonRpcNotification)]
#[notification(method = "session/update")]
struct RawSessionUpdate(Value);

/// One `session/request_permission`, as the JSON the agent sent, answered
/// with the JSON the daemon replies.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, JsonRpcRequest)]
#[request(method = "session/request_permission", response = Value)]
struct RawPermissionRequest(Value);

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
    async fn emit_live(&self, kind: &str, payload: Value) {
        // The id is taken and the event sent under the store's event lock,
        // the one a stored event holds from its id to its publication: a live
        // event cannot pass a stored event with a lower id, nor the other way
        // round, whichever driver of a relaunched session emits it.
        let _order = self.runtime.inner.store.event_order().lock().await;
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

    /// Keep the session's newest context window report. The report is live
    /// session state, not an agent event, and a storage failure cannot stop
    /// the agent's protocol conversation.
    async fn update_context_window(&self, used: i64, size: i64) {
        if let Err(error) = self
            .runtime
            .inner
            .store
            .set_session_context_window(&self.session_id, &self.launch_id, used, size)
            .await
        {
            tracing::warn!(
                session = %self.session_id,
                error = %error,
                "recording ACP context window failed"
            );
        }
    }

    /// Record what this launch has spent so far, as its `stop` would.
    async fn record_usage(&self, usage: TokenUsage) {
        let store = &self.runtime.inner.store;
        if let Err(e) = store
            .upsert_session_usage(&self.session_id, &self.launch_id, usage)
            .await
        {
            tracing::warn!(session = %self.session_id, error = %e, "recording a transcript's usage failed");
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
    /// The live connection to the agent, for everything this launch sends.
    connection: ConnectionTo<Agent>,
    /// The lock a `session/prompt` takes while it is written, so that a
    /// `session/cancel` cannot be sent between a turn being seen running and
    /// the cancel going out. See [`Outbound`].
    outbound: Outbound,
    sink: EventSink,
    turn: Arc<tokio::sync::Mutex<Turn>>,
    /// What this launch's turns have spent so far: each prompt response
    /// reports one turn (ACP), and the `stop` carries their running sum.
    /// Reported only where the launch has no transcript.
    launch_usage: TokenUsage,
    /// The session's transcript, read for what this launch spent: from
    /// session setup on, until a turn ends with none found.
    transcript: Option<Arc<Mutex<LaunchTranscript>>>,
    /// Set once the agent is being killed: no queued prompt starts after it.
    closing: Arc<AtomicBool>,
    /// Told each time a turn's `stop` has been recorded.
    turn_ended: Arc<Notify>,
    /// This launch's turn reports; nobody listening costs nothing.
    reports: Followers,
}

impl Rpc {
    fn new(
        connection: ConnectionTo<Agent>,
        outbound: Outbound,
        sink: EventSink,
        turn: Arc<tokio::sync::Mutex<Turn>>,
        reports: Followers,
        closing: Arc<AtomicBool>,
        turn_ended: Arc<Notify>,
    ) -> Self {
        Self {
            connection,
            outbound,
            sink,
            turn,
            launch_usage: TokenUsage::default(),
            transcript: None,
            closing,
            turn_ended,
            reports,
        }
    }

    /// Send one call to the agent and wait for its answer. Whatever the
    /// agent says meanwhile — an update, a permission request — is handled
    /// by the connection's own handlers, not here.
    async fn call<R>(&self, method: &str, request: R) -> Result<R::Response>
    where
        R: JsonRpcRequest + Send,
    {
        self.connection
            .send_request(request)
            .block_task()
            .await
            .with_context(|| format!("ACP {method} failed"))
    }
}

/// Everything the handling of one incoming ACP message needs, owned rather
/// than borrowed: the SDK's handlers outlive any one call, so this is cloned
/// into them once and shares what it holds with the driver.
#[derive(Clone)]
struct RuntimeIncoming {
    sink: EventSink,
    turn: Arc<tokio::sync::Mutex<Turn>>,
    repository_id: String,
    repository: PathBuf,
    permission_mode: PermissionMode,
    pending_permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
    reports: Followers,
}

impl RuntimeIncoming {
    async fn handle_update(&mut self, params: &Value) -> Result<()> {
        if self.turn.lock().await.replay == Some(false) {
            return Ok(());
        }
        let session_id = params.get("sessionId").cloned().unwrap_or(Value::Null);
        let update = params.get("update").cloned().unwrap_or_default();
        let update_kind = update.get("sessionUpdate").and_then(Value::as_str);
        match update_kind {
            // The text so far is appended and its chunk published under the
            // one lock, so a console opening mid-turn reads a text that ends
            // exactly where its subscription begins. A chunk of the other
            // kind, or of another message, ends the run before it; so does
            // every update below that reports something, which is stored
            // after the text before it.
            Some(kind @ ("agent_message_chunk" | "agent_thought_chunk" | "user_message_chunk")) => {
                if kind == "user_message_chunk" && self.turn.lock().await.replay != Some(true) {
                    return Ok(());
                }
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    let whole = match kind {
                        "agent_thought_chunk" => "agent_thought",
                        "user_message_chunk" => "user_prompt_submit",
                        _ => "agent_message",
                    };
                    let message_id = update.get("messageId").and_then(Value::as_str);
                    let mut turn = self.turn.lock().await;
                    if turn
                        .text
                        .as_ref()
                        .is_some_and(|run| !run.continues(whole, message_id))
                    {
                        self.store_text(&mut turn).await;
                    }
                    let run = turn.text.get_or_insert_with(|| Run {
                        kind: whole,
                        message_id: None,
                        text: String::new(),
                    });
                    run.message_id = run.message_id.take().or(message_id.map(str::to_string));
                    run.text.push_str(text);
                    self.sink
                        .emit_live(kind, json!({"session_id": session_id, "text": text}))
                        .await;
                }
            }
            Some("plan") => {
                self.end_text().await;
                self.sink
                    .emit(
                        "plan",
                        json!({"session_id": session_id, "entries": update.get("entries")}),
                    )
                    .await;
            }
            Some("tool_call") => {
                self.end_text().await;
                let (id, call) = tool_call_of(&update);
                self.turn.lock().await.tools.insert(id, call.clone());
                self.sink
                    .emit("pre_tool_use", tool_payload(session_id, &call))
                    .await;
            }
            Some("tool_call_update") => {
                self.end_text().await;
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
                    let payload = completed_tool_payload(session_id, &merged);
                    let name = payload["tool_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    self.sink.emit("post_tool_use", payload).await;
                    report(&self.reports, TurnReport::ToolEnded(name));
                } else {
                    self.sink
                        .emit_live(
                            "tool_call_update",
                            json!({"session_id": session_id, "tool_call_id": id, "acp": merged}),
                        )
                        .await;
                }
            }
            Some("usage_update") => {
                if let Some((used, size)) = update
                    .get("used")
                    .and_then(Value::as_u64)
                    .zip(update.get("size").and_then(Value::as_u64))
                    .and_then(|(used, size)| i64::try_from(used).ok().zip(i64::try_from(size).ok()))
                {
                    self.sink.update_context_window(used, size).await;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Store the run of text the agent was writing, before whatever it
    /// reports next.
    async fn end_text(&self) {
        let mut turn = self.turn.lock().await;
        self.store_text(&mut turn).await;
    }

    /// End the run of text and store it, under the turn lock the caller
    /// holds: a console opening meanwhile reads that text either as its text
    /// so far or stored, and never as neither.
    async fn store_text(&self, turn: &mut Turn) {
        if let Some((kind, payload)) = turn.end_text() {
            self.sink.emit(kind, payload).await;
        }
    }

    /// Answer one `session/request_permission`, with the params the agent
    /// sent and the result the daemon replies — the connection wraps it.
    async fn handle_permission(&mut self, params: &Value) -> Result<Value> {
        let session_id = params.get("sessionId").cloned().unwrap_or(Value::Null);
        let mut payload = tool_payload(session_id.clone(), &params["toolCall"]);
        payload["options"] = params.get("options").cloned().unwrap_or_default();
        let signature = permission_signature(&params["toolCall"]);
        let remembers = matches!(
            self.permission_mode,
            PermissionMode::Learn | PermissionMode::Ai
        );
        let laya_decision = if self.permission_mode == PermissionMode::Ai {
            match self.sink.runtime.inner.laya.as_ref() {
                Some(laya) => match laya.live().await {
                    Some(live) => Some(
                        decide(
                            &live,
                            &params["toolCall"],
                            &params["options"],
                            &self.repository,
                            self.sink.runtime.inner.timeouts.laya_decision,
                        )
                        .await,
                    ),
                    None => {
                        tracing::warn!("Laya is unavailable for a permission decision");
                        None
                    }
                },
                None => {
                    tracing::warn!("Laya is unavailable for a permission decision");
                    None
                }
            }
        } else {
            None
        };
        let learned = remembers
            && self
                .sink
                .runtime
                .inner
                .store
                .has_learned_permission(&self.repository_id, &signature.tool_name, &signature.kind)
                .await
                .unwrap_or(false);
        let laya_allow = matches!(laya_decision, Some(Decision::Allow { .. }))
            && approved_option(params)
                .as_deref()
                .is_some_and(|option| allowing_option(params, option));
        let waiting = matches!(self.permission_mode, PermissionMode::Ask)
            || (remembers && !learned && !laya_allow);
        let receiver = waiting.then(|| self.begin_permission());
        self.end_text().await;
        self.sink.emit("permission_request", payload).await;
        let (selected, decided_by, label, confidence) = match receiver {
            Some(receiver) => (
                self.wait_for_permission(params, receiver).await?,
                "console",
                None,
                None,
            ),
            None if laya_allow => {
                let Some(Decision::Allow { confidence }) = laya_decision else {
                    unreachable!("laya_allow requires an allowing Laya decision")
                };
                (
                    approved_option(params),
                    "laya",
                    Some("allow"),
                    Some(confidence),
                )
            }
            None if learned => (approved_option(params), "learned", None, None),
            None => (approved_option(params), "auto", None, None),
        };
        if remembers
            && decided_by == "console"
            && !self.repository_id.is_empty()
            && selected
                .as_deref()
                .is_some_and(|option| allowing_option(params, option))
        {
            self.sink
                .runtime
                .inner
                .store
                .learn_permission(&self.repository_id, &signature.tool_name, &signature.kind)
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
                json!({"session_id": session_id, "option_id": selected,
                       "decided_by": decided_by, "label": label, "confidence": confidence}),
            )
            .await;
        Ok(json!({"outcome": outcome}))
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
    ready: Option<oneshot::Sender<()>>,
) -> Result<()> {
    let initialized = rpc.call("initialize", initialize()).await?;
    if initialized.protocol_version != ProtocolVersion::V1 {
        bail!("ACP agent did not negotiate protocol version 1");
    }

    let loose = ready.is_some();
    if loose {
        let first_load = rpc
            .sink
            .runtime
            .inner
            .store
            .get_session(&rpc.sink.session_id)
            .await?
            .launched_at
            .is_none();
        let mut turn = rpc.turn.lock().await;
        turn.replay = Some(first_load);
        turn.running = first_load;
        turn.agent_session = config.resume_session_id.clone();
    }
    let setup = session_setup(rpc, cwd, config, &initialized, loose).await?;
    if loose {
        let mut turn = rpc.turn.lock().await;
        if let Some((kind, payload)) = turn.end_text() {
            rpc.sink.emit(kind, payload).await;
        }
        turn.running = false;
        turn.replay = None;
    }
    let session_id = setup.session_id;
    let _ = rpc.sink.agent_session.set(session_id.clone());
    let (homes, internal, cwd_owned) = (
        rpc.sink.runtime.inner.transcripts.clone(),
        session_id.clone(),
        cwd.to_path_buf(),
    );
    let resumed = config.resume_session_id.is_some();
    rpc.transcript = tokio::task::spawn_blocking(move || {
        LaunchTranscript::open(homes, &internal, &cwd_owned, resumed)
    })
    .await
    .ok()
    .map(|transcript| Arc::new(Mutex::new(transcript)));
    // A loaded conversation runs on whatever model it ran on, and the row
    // records that one. Every new conversation — a loose one started from
    // scratch included — is put on the model it was pinned to.
    let options = if loose && resumed {
        let model = find_config_option(&setup.config_options, &["model"], &["model"])
            .and_then(current_value)
            .unwrap_or(&config.model);
        let store = &rpc.sink.runtime.inner.store;
        let row = store.get_session(&rpc.sink.session_id).await?;
        let agent = ariadne_core::models::agent_of(&row.model);
        store
            .set_loose_session_model(&row.id, &format!("{agent}:{model}"))
            .await?;
        setup.config_options
    } else {
        set_pinned_option(
            rpc,
            &session_id,
            setup.config_options,
            &["model"],
            &["model"],
            "model",
            &config.model,
        )
        .await?
    };
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
    if let Some(ready) = ready {
        let _ = ready.send(());
    }
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
    // polled into a busy loop of immediate `None`s. An agent being killed
    // starts nothing more either.
    let mut console_open = true;
    let closing = rpc.closing.clone();
    loop {
        tokio::select! {
            prompt = prompts.recv(), if console_open && !closing.load(Ordering::SeqCst) => {
                match prompt {
                    Some(prompt) => prompt_once(rpc, session_id, system_prompt, &prompt).await?,
                    None => console_open = false,
                }
            }
            () = rpc.connection.incoming_closed() => return Ok(()),
        }
    }
}

/// What the daemon tells an agent it is, on `initialize`.
///
/// It reads and writes the worktree itself rather than through the agent, and
/// runs no terminal for it. The one session extension it uses is the config
/// options that carry the model and effort pins.
///
/// Compaction is not among them. An agent compacts its own conversation near
/// the context limit and carries on, and the daemon asks for none — so there
/// is nothing it would do with the report that the agent's own next event
/// does not already do.
fn initialize() -> v1::InitializeRequest {
    let capabilities = v1::ClientCapabilities::new().terminal(false).session(
        v1::ClientSessionCapabilities::new()
            .config_options(v1::SessionConfigOptionsCapabilities::new()),
    );
    v1::InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(capabilities)
        .client_info(v1::Implementation::new(
            "ariadne",
            env!("CARGO_PKG_VERSION"),
        ))
}

/// One typed ACP request as the `params` object the transport sends.
///
/// The SDK's request types serialize to exactly the protocol's shape, so this
/// is where a typed value becomes wire JSON and the only place the two meet.
pub(crate) fn to_params<T: serde::Serialize>(request: &T) -> Result<Value> {
    serde_json::to_value(request).context("building an ACP request")
}

/// A session the agent opened, however it was opened: the id it runs under
/// and the options it offers, which is all a launch reads off the answer.
struct OpenedSession {
    session_id: String,
    config_options: Vec<v1::SessionConfigOption>,
}

async fn session_setup(
    rpc: &mut Rpc,
    cwd: &Path,
    config: &LaunchConfig,
    initialized: &v1::InitializeResponse,
    force_load: bool,
) -> Result<OpenedSession> {
    let mcp_servers = crate::acp_schema::mcp_servers(config);
    let Some(session_id) = &config.resume_session_id else {
        let request = v1::NewSessionRequest::new(cwd).mcp_servers(mcp_servers);
        let opened = rpc.call("session/new", request).await?;
        return Ok(OpenedSession {
            session_id: opened.session_id.to_string(),
            config_options: opened.config_options.unwrap_or_default(),
        });
    };
    // A resumed conversation is reopened the way the agent says it can be.
    // `session/resume` is the one to ask for: it puts the agent back at its
    // prompt. `session/load` replays the whole conversation on the way in,
    // which every update of costs an event, so it is the fallback and not
    // the choice.
    let capabilities = &initialized.agent_capabilities;
    // Neither answer carries the id it reopened: it is the one that was
    // asked for, and every caller reads one off the result.
    let config_options = if !force_load && capabilities.session_capabilities.resume.is_some() {
        let request =
            v1::ResumeSessionRequest::new(session_id.clone(), cwd).mcp_servers(mcp_servers);
        rpc.call("session/resume", request).await?.config_options
    } else if capabilities.load_session {
        let request = v1::LoadSessionRequest::new(session_id.clone(), cwd).mcp_servers(mcp_servers);
        rpc.call("session/load", request).await?.config_options
    } else {
        bail!("ACP agent supports neither session/resume nor session/load")
    };
    Ok(OpenedSession {
        session_id: session_id.clone(),
        config_options: config_options.unwrap_or_default(),
    })
}

async fn set_pinned_option(
    rpc: &mut Rpc,
    session_id: &str,
    options: Vec<v1::SessionConfigOption>,
    categories: &[&str],
    names: &[&str],
    label: &str,
    value: &str,
) -> Result<Vec<v1::SessionConfigOption>> {
    let config_id = find_config_option(&options, categories, names)
        .ok_or_else(|| anyhow!("ACP agent did not offer a {label} configuration option"))?
        .id
        .clone();
    // The value rides flattened into the request, which is what puts a
    // select option's id at `value` and a boolean's `type` beside it.
    let request = v1::SetSessionConfigOptionRequest::new(
        session_id.to_string(),
        config_id,
        v1::SessionConfigOptionValue::value_id(value.to_string()),
    );
    let response = rpc
        .call("session/set_config_option", request)
        .await
        .with_context(|| format!("setting ACP {label} to `{value}`"))?;
    let options = match response.config_options.is_empty() {
        true => options,
        false => response.config_options,
    };
    // An agent that took the option answers with it set. One that did not —
    // it read the value and made nothing of it — answers the same way, and
    // the session would run on the agent's own model at the agent's own
    // effort while Ariadne believed it was pinned. The launch fails instead,
    // so a pin that does not land is heard about rather than paid for.
    let settled = find_config_option(&options, categories, names).and_then(current_value);
    match settled {
        Some(settled) if settled == value => Ok(options),
        Some(settled) => bail!("ACP agent kept {label} on `{settled}` when asked for `{value}`"),
        // Nothing to check against: an agent that reports no current value
        // is taken at its word, as it was before this was checked at all.
        None => Ok(options),
    }
}

/// What a select option is set to. A boolean one has no id to compare, and
/// neither pin Ariadne sets is one.
pub(crate) fn current_value(option: &v1::SessionConfigOption) -> Option<&str> {
    match &option.kind {
        v1::SessionConfigKind::Select(select) => Some(select.current_value.0.as_ref()),
        _ => None,
    }
}

/// Find a session configuration option by its ACP category, then by the
/// identifying names agents used before categories were consistently set.
pub(crate) fn find_config_option<'a>(
    options: &'a [v1::SessionConfigOption],
    categories: &[&str],
    names: &[&str],
) -> Option<&'a v1::SessionConfigOption> {
    options
        .iter()
        .find(|option| {
            option
                .category
                .as_ref()
                .is_some_and(|category| categories.contains(&category_name(category)))
        })
        .or_else(|| {
            options.iter().find(|option| {
                [option.id.0.as_ref(), option.name.as_str()]
                    .into_iter()
                    .any(|candidate| {
                        names
                            .iter()
                            .any(|name| candidate.eq_ignore_ascii_case(name))
                    })
            })
        })
}

/// A category as the protocol spells it. One the SDK does not name is an
/// `Other` carrying the agent's own word for it, which is compared as it
/// came.
fn category_name(category: &v1::SessionConfigOptionCategory) -> &str {
    use v1::SessionConfigOptionCategory as C;
    match category {
        C::Mode => "mode",
        C::Model => "model",
        C::ModelConfig => "model_config",
        C::ThoughtLevel => "thought_level",
        C::Other(name) => name,
        _ => "",
    }
}

/// One turn: the prompt out, the turn's chunks as they come — each run of
/// text stored once the next thing the agent reports arrives — and, once the
/// response is in, the last run stored, then the `stop`. A turn that fails
/// stores its last run too, before the error ends the driver.
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
        turn.text = None;
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
    // The transcript is read again while the turn runs, so a long turn's
    // figure moves before its `stop`.
    let (reading, sink) = (rpc.transcript.clone(), rpc.sink.clone());
    let every = sink.runtime.inner.timeouts.transcript_poll;
    let response = {
        let prompt = v1::PromptRequest::new(
            session_id.to_string(),
            vec![v1::ContentBlock::Text(v1::TextContent::new(full.clone()))],
        );
        // The enqueue happens under the lock a cancel also takes, so a
        // `session/cancel` cannot be written between a turn being seen
        // running and this prompt starting one. The wait below does not hold
        // it: a turn runs for minutes, and a cancel has to reach it.
        let sent = {
            let _sending = rpc.outbound.sending.lock().await;
            rpc.connection.send_request(PromptTurn(to_params(&prompt)?))
        };
        let request = async { sent.block_task().await.context("ACP session/prompt failed") };
        tokio::pin!(request);
        let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                response = &mut request => break response,
                _ = ticks.tick(), if reading.is_some() => {
                    if let Some(usage) = transcript_usage(reading.as_ref()).await {
                        sink.record_usage(usage).await;
                    }
                }
            }
        }
    };
    {
        let mut turn = rpc.turn.lock().await;
        if let Some((kind, payload)) = turn.end_text() {
            rpc.sink.emit(kind, payload).await;
        }
        turn.running = false;
    }
    let response = response?;
    let mut stop = json!({
        "session_id": session_id,
        "stop_reason": response.get("stopReason"),
    });
    // The transcript's figure where there is one, else the prompt
    // responses': both report under this launch, so only one of them may.
    let from_transcript = transcript_usage(rpc.transcript.as_ref()).await;
    if from_transcript.is_none() {
        rpc.transcript = None;
    }
    let from_response = usage_for_prompt_response(&response).map(|usage| {
        rpc.launch_usage += usage;
        rpc.launch_usage
    });
    if let Some(usage) = from_transcript.or(from_response) {
        stop["ariadne_usage"] = json!({
            "source": rpc.sink.launch_id,
            "input_tokens": usage.input_tokens,
            "cached_input_tokens": usage.cached_input_tokens,
            "output_tokens": usage.output_tokens,
        });
    }
    rpc.sink.emit("stop", stop).await;
    rpc.turn_ended.notify_waiters();
    report(&rpc.reports, TurnReport::TurnEnded);
    Ok(())
}

/// What the launch has spent by its transcript, read off the runtime's
/// threads; `None` where it has no transcript, or none is found.
async fn transcript_usage(transcript: Option<&Arc<Mutex<LaunchTranscript>>>) -> Option<TokenUsage> {
    let transcript = transcript?.clone();
    tokio::task::spawn_blocking(move || transcript.lock().expect("transcript lock").usage())
        .await
        .ok()
        .flatten()
}

/// What one turn spent, as its ACP prompt response reports it — a cancelled
/// turn's included. Adapter quota totals include subagents, so use them where
/// their shape is complete.
fn usage_for_prompt_response(response: &Value) -> Option<TokenUsage> {
    response
        .pointer("/_meta/quota/token_count")
        .and_then(|usage| adapter_usage(usage, "cachedInputTokens"))
        .or_else(|| {
            response
                .get("usage")
                .and_then(|usage| adapter_usage(usage, "cachedReadTokens"))
        })
}

/// Map one ACP usage object into Ariadne's counters. Input holds every prompt
/// token: fresh, cache read and cache written. Cached input holds only the
/// cache reads, named by `read_field`: a cache write is a token the model read
/// for the first time, not a cache hit.
fn adapter_usage(usage: &Value, read_field: &str) -> Option<TokenUsage> {
    let input_tokens = usage.get("inputTokens").and_then(Value::as_u64)?;
    let output_tokens = usage.get("outputTokens").and_then(Value::as_u64)?;
    let cached_input_tokens = usage.get(read_field).and_then(Value::as_u64).unwrap_or(0);
    let cached_write_tokens = usage
        .get("cachedWriteTokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(TokenUsage {
        input_tokens: input_tokens
            .checked_add(cached_input_tokens)?
            .checked_add(cached_write_tokens)?,
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

/// The terminal event follows its opener in storage, so it keeps only the
/// output a transcript reads. The opener already has the input.
fn completed_tool_payload(session_id: Value, call: &Value) -> Value {
    let mut payload = tool_payload(session_id, call);
    if let Some(payload) = payload.as_object_mut() {
        payload.remove("tool_input");
    }
    let Some(call) = payload.get_mut("acp").and_then(Value::as_object_mut) else {
        return payload;
    };
    call.remove("rawInput");
    if call
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|content| {
            content.iter().any(|entry| {
                entry.get("type").and_then(Value::as_str) == Some("content")
                    && entry
                        .pointer("/content/text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.is_empty())
            })
        })
    {
        call.remove("rawOutput");
    }
    payload
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
    use super::{approved_option, completed_tool_payload, usage_for_prompt_response};

    use ariadne_core::TokenUsage;
    use serde_json::json;

    /// Empty content does not replace the raw output a finished call needs.
    #[test]
    fn empty_content_text_keeps_the_raw_output() {
        let payload = completed_tool_payload(
            json!("session"),
            &json!({
                "title": "Bash", "rawInput": {"command": "make"},
                "content": [{"type": "content", "content": {"type": "text", "text": ""}}],
                "rawOutput": {"stdout": "built"},
            }),
        );

        assert_eq!(payload["acp"]["rawOutput"], json!({"stdout": "built"}));
    }

    /// Every permission request is approved: the allowing option wins
    /// wherever the agent put it, an unmarked list falls back to its first
    /// option, and only an empty one is answered with nothing to select.
    /// A live event takes its id and goes out under the store's event lock,
    /// the one a stored event holds from its id to its publication. A live
    /// event that has to wait for it takes its id after the one that held
    /// it, so it cannot pass a stored event with a lower id.
    #[tokio::test]
    async fn a_live_event_that_waits_for_the_event_lock_takes_its_id_after_the_one_that_held_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = ariadne_store::Store::open(dir.path().join("test.db"))
            .await
            .unwrap();
        let runtime = super::AcpRuntime::new(store.clone());
        let (console, mut rx) = tokio::sync::broadcast::channel(8);
        let sink = super::EventSink {
            runtime,
            session_id: "session".into(),
            launch_id: "launch".into(),
            task_id: None,
            agent_session: std::sync::Arc::new(std::sync::OnceLock::new()),
            console,
        };
        let held = store.event_order().lock().await;
        let emitter = tokio::spawn(async move {
            sink.emit_live("agent_message_chunk", json!({"text": "late"}))
                .await;
        });
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        let meanwhile = ariadne_core::id::new_id();
        drop(held);
        emitter.await.unwrap();

        let event = rx.recv().await.unwrap();
        assert!(
            event.id > meanwhile,
            "the waiting live event took its id after the lock was released: {} > {meanwhile}",
            event.id
        );
    }

    /// The SDK builds `initialize`, so what goes on the pipe is the SDK's
    /// shape and not one this crate writes. An agent reads it once, and what
    /// it reads decides whether the session can be pinned or compacted at
    /// all — so the shape is asserted here rather than left to the stub,
    /// which echoes whatever it is sent.
    #[test]
    fn initialize_tells_the_agent_what_the_daemon_supports() {
        let params = super::to_params(&super::initialize()).unwrap();
        assert_eq!(params["protocolVersion"], json!(1));
        assert_eq!(params["clientInfo"]["name"], json!("ariadne"));

        let capabilities = &params["clientCapabilities"];
        assert_eq!(
            capabilities["fs"],
            json!({"readTextFile": false, "writeTextFile": false}),
            "the daemon reads and writes the worktree itself: {capabilities}"
        );
        assert_eq!(capabilities["terminal"], json!(false));
        assert!(
            capabilities["session"]["configOptions"].is_object(),
            "config options carry the model and effort pins: {capabilities}"
        );
        assert!(
            capabilities["session"]["compaction"].is_null(),
            "compaction is not advertised: the daemon asks for none and reads none"
        );
    }

    /// What a `session/set_config_option` puts on the wire.
    ///
    /// The value is flattened into the request, so a select option's id is
    /// `value` itself and a boolean carries a `type` beside it. Read off the
    /// value type alone it looks like an object, `{"value": "<id>"}`, and it
    /// is not: an agent sent that answers `Invalid params` and refuses the
    /// option, as claude-agent-acp, codex-acp and `opencode acp` all three
    /// did when asked on 2026-09-20. The shapes are asserted here so the
    /// difference is a failing test rather than a launch that pins nothing.
    #[test]
    fn a_config_option_is_set_by_its_value_flattened_into_the_request() {
        use agent_client_protocol::schema::v1::{
            SessionConfigOptionValue as Value, SetSessionConfigOptionRequest as Request,
        };

        let select = Request::new("sess", "model", Value::value_id("gpt-5.6"));
        assert_eq!(
            super::to_params(&select).unwrap(),
            json!({"sessionId": "sess", "configId": "model", "value": "gpt-5.6"}),
            "a select option's value is the id itself"
        );

        let boolean = Request::new("sess", "brave_mode", Value::boolean(true));
        assert_eq!(
            super::to_params(&boolean).unwrap(),
            json!({
                "sessionId": "sess", "configId": "brave_mode",
                "type": "boolean", "value": true
            }),
            "a boolean option carries its type beside the value"
        );
    }

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

    /// The standard and quota shapes name cache reads differently, and the
    /// quota figure wins because it includes the adapter's subagents. Both
    /// keep cache writes in input and out of cached input.
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
                cached_input_tokens: 20,
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
                cached_input_tokens: 5,
                output_tokens: 7,
            })
        );

        // A turn measured on claude-agent-acp 0.79.0: its transcript agrees.
        let measured = json!({"_meta": {"quota": {"token_count": {
            "inputTokens": 10,
            "cachedInputTokens": 90232,
            "cachedWriteTokens": 10189,
            "outputTokens": 291,
        }}}});
        assert_eq!(
            usage_for_prompt_response(&measured),
            Some(TokenUsage {
                input_tokens: 100431,
                cached_input_tokens: 90232,
                output_tokens: 291,
            })
        );

        // Cache reads alone are cached input; a write stays in input only.
        let written = json!({"usage": {
            "inputTokens": 1,
            "cachedWriteTokens": 4,
            "outputTokens": 2,
        }});
        assert_eq!(
            usage_for_prompt_response(&written),
            Some(TokenUsage {
                input_tokens: 5,
                cached_input_tokens: 0,
                output_tokens: 2,
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
