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
use std::time::{Duration, Instant};

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

use ariadne_api::events::{AgentEventDto, IngestEventRequest, PermissionReplyDto};
use ariadne_core::acp::LaunchConfig;
use ariadne_core::id::new_id;
use ariadne_core::{PermissionMode, TokenUsage};
use ariadne_store::{PullRequestTold, Store};

use crate::acp_calls::PromptTurn;
use crate::acp_transport::{PromptWrites, witnessed_pipes};
use crate::ai_permissions::AiPermissions;
use crate::ai_permissions::decide::{Decision, decide, prepare};
use crate::ai_permissions::derive::derive;
use crate::http::classify::summarize;
use crate::http::events::ingest_event;
use crate::learned_key::{Facts, Key, normalize};
use crate::scheduler::SchedEvent;
use crate::stats::session_fact;
use crate::timeouts::Timeouts;
use crate::transcript::{LaunchTranscript, TranscriptHomes};

/// Live console events buffered per subscriber before it is told to resync.
const CONSOLE_CAPACITY: usize = 1024;

/// What one launch reports of its turns as they go, to whoever waits on the
/// agent's own word. Each launch
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
    /// That repository's checkout, empty where there is none: a learned
    /// key names it `<REPO>`.
    pub repository_path: String,
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
    /// The branch each session works on, by Ariadne session id: what a
    /// learned key names `<BRANCH>`. Shared with the session's runtime, so a
    /// session moved to another branch keys on that one.
    branches: Mutex<HashMap<String, SessionBranch>>,
    /// Wakes the scheduler after an event lands, the way the HTTP ingestion
    /// does — present once a scheduler is running.
    scheduler: OnceLock<mpsc::UnboundedSender<SchedEvent>>,
    /// The live-only events — message and thought chunks, tool call
    /// progress — on their way to the console streams and nowhere else: none
    /// of them is stored, and none reaches the domain bus. One channel per
    /// session, so a chatty session never lags another session's console,
    /// kept for as long as an agent runs for it or somebody listens.
    consoles: Mutex<HashMap<String, broadcast::Sender<AgentEventDto>>>,
    ai_permissions: Option<AiPermissions>,
    exhausted_patterns: Vec<String>,
    failure_diagnosis: Option<crate::failure_diagnosis::FailureDiagnosis>,
}

/// The branch one session works on, shared between the runtime and the
/// launcher that moves it.
type SessionBranch = Arc<Mutex<Option<String>>>;

/// Resolves once a driver has killed and reaped its child. Shared, so that
/// every launch of the session waits on the same reap.
type Reaped = Shared<oneshot::Receiver<()>>;

/// What holds a session's next launch back, from [`AcpRuntime::kill_gated`]
/// until it is dropped. It sits among the agents still ending, so a launch
/// waits on it as it waits on them.
pub(crate) struct LaunchGate {
    inner: Arc<Inner>,
    session_id: String,
    key: String,
    open: Option<oneshot::Sender<()>>,
}

impl Drop for LaunchGate {
    fn drop(&mut self) {
        let mut ending = self.inner.ending.lock().expect("acp ending lock");
        if let Some(launches) = ending.get_mut(&self.session_id) {
            launches.retain(|(key, _)| *key != self.key);
            if launches.is_empty() {
                ending.remove(&self.session_id);
            }
        }
        drop(ending);
        if let Some(open) = self.open.take() {
            let _ = open.send(());
        }
    }
}

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
    /// What this prompt delivers: an agent message (018) or a pull
    /// request's news (026). The driver claims it right before the prompt
    /// goes out. A nudge, a briefing and console input carry none.
    delivery: Option<Delivery>,
}

/// What a daemon prompt hands over that the store records as handed, claimed
/// by the driver right before the prompt goes out and given back where the
/// prompt was never written.
#[derive(Debug, Clone)]
pub(crate) enum Delivery {
    Step(String),
    /// An agent message, by id (018).
    Message(String),
    /// The news of a pull request (026). Boxed: it carries two marks and a
    /// list, where a message carries an id.
    PullRequestNews(Box<NewsDelivery>),
    /// A goal's failed and stalled tasks (031), named by id with the
    /// `updated_at` each failure carried at the moment this was queued —
    /// a later retry of the same task stamps a new one, so an old
    /// confirmation never answers for a new failure of it. Confirmed only
    /// once this exact prompt's own turn ends (`serve_with_input`), never
    /// by an unrelated turn landing on the same session: nothing here is
    /// written to the store until this delivery's own `session/prompt`
    /// call returns.
    GoalAttention {
        goal_id: String,
        failed_tasks: Vec<(String, String)>,
    },
}

/// One pull request's news, as its prompt delivers it: the comments it
/// names, the told mark it leaves, and the mark it found, which a prompt
/// that never went out puts back.
#[derive(Debug, Clone)]
pub(crate) struct NewsDelivery {
    pub(crate) pull_request_id: String,
    pub(crate) comment_ids: Vec<String>,
    pub(crate) told: PullRequestTold,
    pub(crate) before: PullRequestTold,
}

impl Delivery {
    /// What tells two queued deliveries apart: one per message, and one per
    /// pull request, whose next news waits until the queued one is claimed.
    fn key(&self) -> String {
        match self {
            Delivery::Step(id) => format!("step:{id}"),
            Delivery::Message(id) => format!("message:{id}"),
            Delivery::PullRequestNews(news) => format!("news:{}", news.pull_request_id),
            // Signed by its own content, not only the goal: a situation
            // that changes while the previous one is still queued is a
            // different delivery, queued beside it rather than dropped.
            Delivery::GoalAttention {
                goal_id,
                failed_tasks,
            } => format!("goal_attention:{goal_id}:{failed_tasks:?}"),
        }
    }
}

/// The commands the agent most recently offered for this session.
struct AvailableCommands {
    /// The agent session the update named.
    session_id: Value,
    /// Each command exactly as the agent sent it.
    commands: Vec<Value>,
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
    tools: HashMap<String, OpenToolCall>,
    available_commands: Option<AvailableCommands>,
}

/// An open tool call.
struct OpenToolCall {
    call: Value,
}

impl Turn {
    /// The live state a console snapshot appends after its stored events.
    fn so_far(&self, session_id: &str, task_id: &Option<String>) -> Vec<AgentEventDto> {
        let mut events = Vec::new();
        if let Some(run) = self.text.as_ref().filter(|_| self.running) {
            events.push(live_event(
                session_id,
                task_id.clone(),
                &format!("{}_chunk", run.kind),
                json!({"session_id": self.agent_session, "text": run.text}),
            ));
        }
        if let Some(commands) = &self.available_commands {
            events.push(live_event(
                session_id,
                task_id.clone(),
                "available_commands_update",
                json!({"session_id": commands.session_id, "available_commands": commands.commands}),
            ));
        }
        events
    }

    /// Whether this prompt starts with one of the agent's offered commands.
    fn is_command(&self, text: &str) -> bool {
        let Some(word) = text
            .strip_prefix('/')
            .and_then(|text| text.split_whitespace().next())
        else {
            return false;
        };
        self.available_commands.as_ref().is_some_and(|commands| {
            commands
                .commands
                .iter()
                .any(|command| command.get("name").and_then(Value::as_str) == Some(word))
        })
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
    switches: mpsc::UnboundedSender<ConfigSwitch>,
    /// The deliveries in `prompts` that the driver has not taken off yet, by
    /// [`Delivery::key`]. A scheduler pass hands every unstamped message and
    /// every untold piece of news again, and one already queued here is not
    /// queued twice.
    queued_messages: HashSet<String>,
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

struct ConfigSwitch {
    model: String,
    effort: Option<String>,
    done: oneshot::Sender<Result<()>>,
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
                branches: Mutex::default(),
                scheduler: OnceLock::new(),
                consoles: Mutex::new(HashMap::new()),
                ai_permissions: None,
                exhausted_patterns: crate::config::default_exhausted_patterns(),
                failure_diagnosis: None,
            }),
        }
    }

    pub fn with_exhausted_patterns(mut self, patterns: Vec<String>) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("the runtime is not shared while it is configured")
            .exhausted_patterns = patterns;
        self
    }

    /// Set the branch a session works on: its task's. Its learned keys name
    /// it `<BRANCH>` from the next
    /// permission request on, a running agent's included.
    pub fn set_task_branch(&self, session_id: &str, branch: Option<String>) {
        *self.branch_of(session_id).lock().expect("ACP branch lock") = branch;
    }

    /// The branch a session works on, where one was set.
    pub fn task_branch(&self, session_id: &str) -> Option<String> {
        self.branch_of(session_id)
            .lock()
            .expect("ACP branch lock")
            .clone()
    }

    fn branch_of(&self, session_id: &str) -> SessionBranch {
        self.inner
            .branches
            .lock()
            .expect("ACP branches lock")
            .entry(session_id.to_string())
            .or_default()
            .clone()
    }

    /// Give this runtime the daemon's AI permission model before it is shared.
    pub fn with_ai_permissions(mut self, ai_permissions: AiPermissions) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("a new ACP runtime has one owner")
            .ai_permissions = Some(ai_permissions);
        self
    }

    /// Give this runtime the daemon's optional failure classifier (024)
    /// before it is shared.
    pub fn with_failure_diagnosis(
        mut self,
        failure_diagnosis: crate::failure_diagnosis::FailureDiagnosis,
    ) -> Self {
        Arc::get_mut(&mut self.inner)
            .expect("a new ACP runtime has one owner")
            .failure_diagnosis = Some(failure_diagnosis);
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

    /// Whether this session's agent is inside a turn: a `session/prompt`
    /// out and its response not yet in. `None` where no agent is driven here
    /// for the session, and there is nothing to ask.
    pub(crate) async fn in_turn(&self, session_id: &str) -> Option<bool> {
        let turn = self
            .inner
            .running
            .lock()
            .expect("acp registry lock")
            .get(session_id)?
            .turn
            .clone();
        Some(turn.lock().await.running)
    }

    /// Queue an option change ahead of the next prompt. A running turn does
    /// not hold up the caller; between turns the caller receives the result.
    pub(crate) async fn switch_pin(
        &self,
        session_id: &str,
        model: &str,
        effort: Option<&str>,
    ) -> Result<()> {
        let (switches, turn) = {
            let running = self.inner.running.lock().expect("acp registry lock");
            let agent = running
                .get(session_id)
                .ok_or_else(|| anyhow!("no ACP agent is running for session {session_id}"))?;
            (agent.switches.clone(), agent.turn.clone())
        };
        let (done, result) = oneshot::channel();
        let mid_turn = turn.lock().await.running;
        switches.send(ConfigSwitch {
            model: model.to_string(),
            effort: effort.map(str::to_string),
            done,
        })?;
        if mid_turn {
            return Ok(());
        }
        result
            .await
            .map_err(|_| anyhow!("the ACP agent ended before changing its pin"))?
    }

    /// Hand the running agent a prompt from the daemon itself — a scheduler
    /// nudge, a review briefing, an agent message. Always a `session/prompt`,
    /// queued in order behind whichever turn runs: unlike console input it
    /// answers no pending permission, so a delivery is never consumed as an
    /// option answer meant for a person.
    ///
    /// Errs where there is nobody here to hear it: no agent runs for this
    /// session.
    pub(crate) fn send_step(&self, session_id: &str, transition: &str, text: String) -> Result<()> {
        self.queue_daemon_prompt(session_id, text, Some(Delivery::Step(transition.into())))
    }

    pub fn send_prompt(&self, session_id: &str, text: String) -> Result<()> {
        self.queue_daemon_prompt(session_id, text, None)
    }

    /// Hand the running orchestrator a prompt naming its goal's failed and
    /// stalled tasks (031), tagged with exactly the failures named so the
    /// turn that answers it can confirm exactly those, and no other.
    pub(crate) fn send_goal_attention(
        &self,
        session_id: &str,
        text: String,
        goal_id: String,
        failed_tasks: Vec<(String, String)>,
    ) -> Result<()> {
        self.queue_daemon_prompt(
            session_id,
            text,
            Some(Delivery::GoalAttention {
                goal_id,
                failed_tasks,
            }),
        )
    }

    /// Hand the running agent a pull request's news as a prompt (026). The
    /// driver writes the told marks right before the prompt goes out, and
    /// gives them back where it never went out; news of a request still
    /// queued is not queued again.
    pub(crate) fn send_news(
        &self,
        session_id: &str,
        news: NewsDelivery,
        text: String,
    ) -> Result<()> {
        self.queue_daemon_prompt(
            session_id,
            text,
            Some(Delivery::PullRequestNews(Box::new(news))),
        )
    }

    /// Hand the running agent an agent message as a prompt, the way
    /// [`Self::send_prompt`] does. The driver claims the message right
    /// before the prompt goes out, and skips the prompt where a read took
    /// the message first (018).
    ///
    /// A message still queued for this agent is not queued again: the
    /// hand-over answers `Ok` and changes nothing.
    pub(crate) fn send_message(
        &self,
        session_id: &str,
        message_id: &str,
        text: String,
    ) -> Result<()> {
        self.queue_daemon_prompt(
            session_id,
            text,
            Some(Delivery::Message(message_id.to_string())),
        )
    }

    fn queue_daemon_prompt(
        &self,
        session_id: &str,
        text: String,
        delivery: Option<Delivery>,
    ) -> Result<()> {
        let mut running = self.inner.running.lock().expect("acp registry lock");
        let agent = running
            .get_mut(session_id)
            .ok_or_else(|| anyhow!("no ACP agent is running for session {session_id}"))?;
        let key = delivery.as_ref().map(Delivery::key);
        if let Some(key) = &key
            && !agent.queued_messages.insert(key.clone())
        {
            return Ok(());
        }
        let sent = agent.prompts.send(Prompt {
            text,
            source: PromptSource::Daemon,
            delivery,
        });
        if sent.is_err()
            && let Some(key) = &key
        {
            agent.queued_messages.remove(key);
        }
        sent.map_err(|_| anyhow!("the ACP agent for session {session_id} is no longer listening"))
    }

    /// Take a delivery off this launch's queue: the driver is about to claim
    /// it, and a later hand-over may queue it again. A relaunch has a queue
    /// of its own, which this leaves alone.
    fn dequeue_message(&self, session_id: &str, launch_id: &str, key: &str) {
        if let Some(agent) = self
            .inner
            .running
            .lock()
            .expect("acp registry lock")
            .get_mut(session_id)
            .filter(|agent| agent.launch_id == launch_id)
        {
            agent.queued_messages.remove(key);
        }
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
        let _ = self.close_prompt_channel_until_reopened_for_test(session_id);
    }

    /// Test support: [`Self::close_prompt_channel_for_test`], and a function
    /// that gives the same agent its working channel back. A test that wants
    /// the agent to hear again reopens it rather than launching another
    /// under the session: the launch takes the agent down first, and a pass
    /// landing in between finds the seat empty and fills it itself.
    pub fn close_prompt_channel_until_reopened_for_test(
        &self,
        session_id: &str,
    ) -> impl FnOnce() + '_ {
        let (closed, unread) = mpsc::unbounded_channel();
        drop(unread);
        let taken = self
            .inner
            .running
            .lock()
            .expect("acp registry lock")
            .get_mut(session_id)
            .map(|agent| {
                (
                    agent.launch_id.clone(),
                    std::mem::replace(&mut agent.prompts, closed),
                )
            });
        let session_id = session_id.to_string();
        move || {
            let Some((launch_id, prompts)) = taken else {
                return;
            };
            if let Some(agent) = self
                .inner
                .running
                .lock()
                .expect("acp registry lock")
                .get_mut(&session_id)
                .filter(|agent| agent.launch_id == launch_id)
            {
                agent.prompts = prompts;
            }
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
                delivery: None,
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
        let (switches, pending_switches) = mpsc::unbounded_channel();
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
                    switches,
                    queued_messages: HashSet::new(),
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
                    pending_switches,
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

    /// [`Self::kill`], and wait until the driver has cancelled the turn and
    /// reaped the child: for a caller that starts another agent on the same
    /// work under another session id, which no launch of this one waits for.
    pub(crate) async fn kill_and_wait(&self, session_id: &str) {
        self.take_down(session_id);
        for reaped in self.ending_for(session_id) {
            let _ = reaped.await;
        }
    }

    /// [`Self::kill`], and a future that resolves once the driver has
    /// cancelled the turn, reported what it spent and its end, and reaped the
    /// child: for a caller that must not wait in line, but has work to do
    /// once the agent's last words are in.
    ///
    /// The [`LaunchGate`] holds the session's next launch back until it is
    /// dropped, the way an agent still ending holds it back: the caller's
    /// work after the reap reads the session as this agent left it, and no
    /// agent launched after it can add to that.
    pub(crate) fn kill_gated(
        &self,
        session_id: &str,
    ) -> (impl Future<Output = ()> + use<>, LaunchGate) {
        self.take_down(session_id);
        let reaped = self.ending_for(session_id);
        let (open, opened) = oneshot::channel();
        let key = format!("gate:{}", ariadne_core::id::new_id());
        self.inner
            .ending
            .lock()
            .expect("acp ending lock")
            .entry(session_id.to_string())
            .or_default()
            .push((key.clone(), opened.shared()));
        let gate = LaunchGate {
            inner: self.inner.clone(),
            session_id: session_id.to_string(),
            key,
            open: Some(open),
        };
        let reaped = async move {
            for reaped in reaped {
                let _ = reaped.await;
            }
        };
        (reaped, gate)
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
    ///
    /// Answers whether the entry was still this driver's: an agent that
    /// ended by itself, rather than one a kill or a relaunch took down first.
    fn deregister(&self, session_id: &str, launch_id: &str) -> bool {
        let mut running = self.inner.running.lock().expect("acp registry lock");
        let own = running
            .get(session_id)
            .is_some_and(|agent| agent.launch_id == launch_id);
        if own {
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
        own
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
        switches: mpsc::UnboundedReceiver<ConfigSwitch>,
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
        let in_flight = MessageInFlight::default();
        let transport = witnessed_pipes(io.stdin, io.stdout, in_flight.writes.clone());
        // What the connection's handlers need. They run on its dispatch
        // loop, one message at a time, which is the order the transport's own
        // loop gave them.
        let incoming = RuntimeIncoming {
            sink: sink.clone(),
            turn: turn.clone(),
            repository_id: launch.repository_id.clone(),
            permission_mode: launch.permission_mode,
            workspace: launch.cwd.display().to_string(),
            key_facts: Facts {
                repository: launch.repository_path.clone(),
                worktree: launch.cwd.display().to_string(),
                branch: None,
                home: std::env::var("HOME").unwrap_or_default(),
            },
            branch: self.branch_of(&launch.session_id),
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
                .connect_with(transport, async |cx| {
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
                    rpc.in_flight = in_flight.clone();
                    protocol_outcome = Some(
                        run_protocol(
                            &mut rpc,
                            &launch.cwd,
                            &launch.config,
                            prompts,
                            switches,
                            io.ready,
                        )
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
        // A claimed delivery whose prompt never reached the agent's stdin —
        // the write failed, or a kill came first — waits for the next live
        // agent, or for a read, again (018, 026).
        if let Some(delivery) = in_flight.unwritten() {
            let released = match &delivery {
                Delivery::Step(id) => self.inner.store.release_step_briefing(id).await,
                Delivery::Message(id) => self.inner.store.unmark_message_delivered(id).await,
                Delivery::PullRequestNews(news) => {
                    self.inner
                        .store
                        .release_pull_request_news(
                            &news.pull_request_id,
                            &news.comment_ids,
                            &news.before,
                        )
                        .await
                }
                // Nothing was claimed ahead of the write (`claim_message`),
                // so there is nothing to release: the next pass over the
                // goal queues it again on its own.
                Delivery::GoalAttention { .. } => Ok(()),
            };
            if let Err(error) = released {
                tracing::warn!(session = %launch.session_id, delivery = %delivery.key(), error = %format!("{error:#}"), "releasing the delivery failed");
            }
        }
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
            let protocol = error
                .chain()
                .find_map(|cause| cause.downcast_ref::<agent_client_protocol::Error>());
            let mut reported = match protocol {
                Some(error) => json!({
                    "code": i32::from(error.code),
                    "message": error.message,
                    "data": error.data,
                }),
                None => json!({"data": {"message": format!("{error:#}")}}),
            };
            if let Some(protocol) = protocol
                && let Some(data) = protocol.data.as_ref()
                && reported
                    .pointer("/data/message")
                    .and_then(Value::as_str)
                    .is_none_or(|message| message.trim().is_empty())
            {
                let detail = acp_error_detail(data);
                let detail = detail.split_whitespace().collect::<Vec<_>>().join(" ");
                let line = if detail.is_empty() || detail == protocol.message.trim() {
                    protocol.message.clone()
                } else {
                    format!("{}: {detail}", protocol.message)
                };
                if let Some(fields) = reported["data"].as_object_mut() {
                    fields.insert("message".into(), json!(line));
                } else {
                    reported["data"] = json!({"message": line, "details": data});
                }
            }
            let message = protocol
                .map(|error| error.message.clone())
                .unwrap_or_else(|| format!("{error:#}"));
            if let Some(reason) = exhausted_reason(
                &message,
                protocol.and_then(|error| error.data.as_ref()),
                &self.inner.exhausted_patterns,
            ) {
                reported["exhausted"] = json!(true);
                reported["exhausted_reason"] = json!(reason);
            }
            let error_event_id = sink
                .emit(
                    "session.error",
                    json!({
                        "session_id": sink.agent_session_id(&launch.config),
                        "error": reported,
                    }),
                )
                .await;
            // The advisory diagnosis is considered only once the session's
            // own error is recorded and published (024): it never holds
            // this up, and runs as its own bounded task from here on.
            if let Some(failure_diagnosis) = &self.inner.failure_diagnosis {
                failure_diagnosis
                    .consider(
                        launch.session_id.clone(),
                        launch.launch_id.clone(),
                        error_event_id,
                        message,
                        protocol.and_then(|error| error.data.clone()),
                    )
                    .await;
            }
        }
        turn.lock().await.available_commands = None;
        sink.emit(
            "session_end",
            json!({"session_id": sink.agent_session_id(&launch.config)}),
        )
        .await;
        // Every wake the session's last words sent can be answered by a pass
        // that still finds this agent registered, and leaves the seat alone
        // as filled. Only from here does the runtime say nobody runs for it,
        // so the scheduler is told once more, past any window. An agent a
        // kill or a relaunch took down is the daemon's own doing, and owes
        // no pass.
        if self.deregister(&launch.session_id, &launch.launch_id)
            && let Some(tx) = self.inner.scheduler.get()
        {
            let _ = tx.send(SchedEvent::SessionEnded {
                session_id: launch.session_id.clone(),
                launch_id: launch.launch_id.clone(),
            });
        }
    }
}

fn acp_error_detail(data: &Value) -> String {
    const KEYS: [&str; 6] = [
        "details",
        "detail",
        "error",
        "reason",
        "description",
        "stderr",
    ];
    if let Some(detail) = data.as_str() {
        return detail.to_string();
    }
    if let Some(fields) = data.as_object() {
        for key in KEYS {
            let Some(value) = fields.get(key) else {
                continue;
            };
            if let Some(detail) = value.as_str().filter(|detail| !detail.trim().is_empty()) {
                return detail.to_string();
            }
            if let Some(nested) = value.as_object() {
                for nested_key in ["message"].into_iter().chain(KEYS) {
                    if let Some(detail) = nested
                        .get(nested_key)
                        .and_then(Value::as_str)
                        .filter(|detail| !detail.trim().is_empty())
                    {
                        return detail.to_string();
                    }
                }
            }
        }
    }
    let json = data.to_string();
    let mut chars = json.chars();
    let mut detail: String = chars.by_ref().take(200).collect();
    if chars.next().is_some() {
        detail.push('…');
    }
    detail
}

fn exhausted_reason(message: &str, data: Option<&Value>, patterns: &[String]) -> Option<String> {
    if let Some(reason) = data
        .and_then(|data| data.get("codexErrorInfo"))
        .and_then(Value::as_str)
        .filter(|reason| *reason == "usageLimitExceeded")
    {
        return Some(reason.to_string());
    }
    if data
        .and_then(|data| data.pointer("/_meta/jetbrains/air/sessionFailure/category"))
        .and_then(Value::as_str)
        == Some("limit")
    {
        return Some("limit".to_string());
    }
    if let Some(reason) = data
        .and_then(|data| data.get("errorKind"))
        .and_then(Value::as_str)
        .filter(|reason| *reason == "rate_limit")
    {
        return Some(reason.to_string());
    }
    // An adapter may say "Internal error" and keep the provider's own words
    // in its data — claude-acp's 429 on `session/set_config_option` does.
    let text = match data {
        Some(data) => format!("{message}\n{}", acp_error_detail(data)),
        None => message.to_string(),
    }
    .to_lowercase();
    patterns
        .iter()
        .find(|pattern| text.contains(&pattern.to_lowercase()))
        .cloned()
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
    /// the record, never the agent. Returns the id it was stored under,
    /// where it was stored at all — `None` on a store failure, and for the
    /// one case nothing is stored (the end of a launch this session has
    /// moved past) — for a caller that needs to correlate something with
    /// it (`crate::failure_diagnosis`, with a `session.error`).
    async fn emit(&self, kind: &str, payload: Value) -> Option<String> {
        let request = IngestEventRequest {
            session_id: self.session_id.clone(),
            launch: Some(self.launch_id.clone()),
            kind: kind.to_string(),
            payload,
        };
        let event = match ingest_event(&self.runtime.inner.store, &request).await {
            Ok(event) => event,
            Err(e) => {
                tracing::warn!(session = %self.session_id, kind, error = %e, "recording an ACP event failed");
                return None;
            }
        };
        if let Some(tx) = self.runtime.inner.scheduler.get() {
            let _ = tx.send(SchedEvent::SessionEvent(self.session_id.clone()));
        }
        event.map(|event| event.id)
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

    /// Append one session fact without making ACP wait on SQLite. Facts are
    /// observational: a failure is logged after the protocol reply has gone
    /// out, and does not change the turn.
    fn record_stat_fact(&self, kind: &'static str, data: Value) {
        let store = self.runtime.inner.store.clone();
        let session_id = self.session_id.clone();
        tokio::spawn(async move {
            let result = async {
                let fact = session_fact(&store, &session_id, kind, data).await?;
                store.record_fact(fact).await
            }
            .await;
            if let Err(error) = result {
                tracing::warn!(session = %session_id, kind, error = %error, "recording ACP stat fact failed");
            }
        });
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
    /// The agent message whose prompt is going out.
    in_flight: MessageInFlight,
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
            in_flight: MessageInFlight::default(),
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
    permission_mode: PermissionMode,
    workspace: String,
    /// What a learned key replaces with placeholders, but the branch.
    key_facts: Facts,
    /// The session's branch, which can change while it runs.
    branch: SessionBranch,
    pending_permission: Arc<Mutex<Option<oneshot::Sender<String>>>>,
    reports: Followers,
}

impl RuntimeIncoming {
    async fn handle_update(&mut self, params: &Value) -> Result<()> {
        let session_id = params.get("sessionId").cloned().unwrap_or(Value::Null);
        let update = params.get("update").cloned().unwrap_or_default();
        let update_kind = update.get("sessionUpdate").and_then(Value::as_str);
        if self.turn.lock().await.replay == Some(false)
            && update_kind != Some("available_commands_update")
        {
            return Ok(());
        }
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
                self.turn
                    .lock()
                    .await
                    .tools
                    .insert(id, OpenToolCall { call: call.clone() });
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
                        .or_insert_with(|| OpenToolCall {
                            call: json!({"toolCallId": id}),
                        });
                    merge_tool_call(&mut call.call, &update);
                    match terminal {
                        true => turn
                            .tools
                            .remove(&id)
                            .unwrap_or(OpenToolCall { call: Value::Null }),
                        false => OpenToolCall {
                            call: call.call.clone(),
                        },
                    }
                };
                if terminal {
                    let payload = completed_tool_payload(session_id, &merged.call);
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
                            json!({"session_id": session_id, "tool_call_id": id, "acp": merged.call}),
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
            Some("available_commands_update") => {
                let commands = update
                    .get("availableCommands")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut turn = self.turn.lock().await;
                turn.available_commands = Some(AvailableCommands {
                    session_id: session_id.clone(),
                    commands: commands.clone(),
                });
                self.sink
                    .emit_live(
                        "available_commands_update",
                        json!({"session_id": session_id, "available_commands": commands}),
                    )
                    .await;
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
        let requested_at = Instant::now();
        let session_id = params.get("sessionId").cloned().unwrap_or(Value::Null);
        let mut payload = tool_payload(session_id.clone(), &params["toolCall"]);
        payload["options"] = params.get("options").cloned().unwrap_or_default();
        let signature = permission_signature(&params["toolCall"]);
        // The learned row's key and the request's risk tags. The model reads
        // neither: it is given the raw input.
        let key = normalize(
            &signature.tool_name,
            signature.raw_input.as_ref().unwrap_or(&Value::Null),
            &Facts {
                branch: self.branch.lock().expect("ACP branch lock").clone(),
                ..self.key_facts.clone()
            },
        );
        let request_tags: Vec<String> = derive(
            &json!({"toolCall": params["toolCall"], "options": params["options"]}),
            Some(&self.workspace),
        )
        .risk_tags
        .into_iter()
        .map(str::to_string)
        .collect();
        let remembers = matches!(
            self.permission_mode,
            PermissionMode::Learn | PermissionMode::Ai
        );
        let ai_permissions_decision = if self.permission_mode == PermissionMode::Ai {
            let prepared = prepare(
                &params["toolCall"],
                &params["options"],
                Some(&self.workspace),
            );
            match self.sink.runtime.inner.ai_permissions.as_ref() {
                Some(ai_permissions) => match ai_permissions.live_once_started().await {
                    Some(live) => {
                        // A permission decision blocks the agent's turn; an
                        // advisory diagnosis blocks nothing. Abort one
                        // reaching this same model first, so it never makes
                        // this decision wait behind it (024).
                        ai_permissions.preempt_diagnosis();
                        Some(
                            decide(
                                &live,
                                &prepared,
                                self.sink.runtime.inner.timeouts.ai_permissions_decision,
                            )
                            .await,
                        )
                    }
                    None => {
                        tracing::warn!(
                            "AI permission model is unavailable for a permission decision"
                        );
                        Some(prepared.unanswered("unavailable"))
                    }
                },
                None => {
                    tracing::warn!("AI permission model is unavailable for a permission decision");
                    Some(prepared.unanswered("unavailable"))
                }
            }
        } else {
            None
        };
        let learned = if remembers && allowing_agent_option(params).is_some() {
            self.learned_allow(&signature, &key, &request_tags).await
        } else {
            None
        };
        let ai_permissions_selection = match &ai_permissions_decision {
            Some(Decision::Allow { .. }) => {
                approved_option(params).filter(|option| allowing_option(params, option))
            }
            Some(Decision::Deny { .. }) => rejecting_option(params),
            _ => None,
        };
        let deny_without_option = matches!(&ai_permissions_decision, Some(Decision::Deny { .. }))
            && ai_permissions_selection.is_none();
        let waiting = matches!(self.permission_mode, PermissionMode::Ask)
            || deny_without_option
            || (remembers && learned.is_none() && ai_permissions_selection.is_none());
        if waiting && remembers {
            payload["agent_options"] = payload["options"].clone();
            payload["options"] = learned_choices(params, &key.family);
        }
        // What the model made of the request, whoever answers it: the
        // question shows it while it waits, and the reply keeps it, so a
        // reader asking why the console was asked finds the score it fell
        // short with.
        let label = match &ai_permissions_decision {
            Some(Decision::Allow { .. }) => Some("allow"),
            Some(Decision::Ask { .. }) => Some("ask"),
            Some(Decision::Deny { .. }) => Some("deny"),
            Some(Decision::Unanswered { .. }) | None => None,
        };
        let score = ai_permissions_decision.as_ref().and_then(Decision::score);
        let danger = score.map(|score| score.danger);
        let allow_threshold = score.map(|score| score.allow_threshold);
        let deny_threshold = score.map(|score| score.deny_threshold);
        let probabilities = score.map(|score| score.probabilities.clone());
        let ai_error = match &ai_permissions_decision {
            Some(Decision::Unanswered { reason, .. }) => Some(*reason),
            _ => None,
        };
        let derived = ai_permissions_decision.as_ref().map(Decision::derived);
        let operation = derived.and_then(|derived| derived.operation);
        let risk_tags = derived.map(|derived| derived.risk_tags.clone());
        let cap = ai_permissions_decision.as_ref().and_then(Decision::cap);
        let request_decided_by = if waiting {
            None
        } else if ai_permissions_selection.is_some() {
            Some("ai")
        } else if learned.is_some() {
            Some("learned")
        } else {
            Some("auto")
        };
        for (key, value) in [
            ("decided_by", json!(request_decided_by)),
            ("label", json!(label)),
            ("danger", json!(danger)),
            ("allow_threshold", json!(allow_threshold)),
            ("deny_threshold", json!(deny_threshold)),
            ("ai_error", json!(ai_error)),
            ("operation", json!(operation)),
            ("risk_tags", json!(risk_tags)),
            ("cap", json!(cap)),
            ("probabilities", json!(probabilities)),
            (
                "learned_id",
                json!(
                    learned
                        .as_ref()
                        .filter(|_| request_decided_by == Some("learned"))
                        .map(|row| &row.id)
                ),
            ),
            (
                "learned_level",
                json!(
                    learned
                        .as_ref()
                        .filter(|_| request_decided_by == Some("learned"))
                        .map(|row| &row.level)
                ),
            ),
            (
                "learned_key",
                json!(
                    learned
                        .as_ref()
                        .filter(|_| request_decided_by == Some("learned"))
                        .map(|row| &row.key)
                ),
            ),
        ] {
            payload[key] = value;
        }
        let receiver = waiting.then(|| self.begin_permission());
        self.end_text().await;
        self.sink.emit("permission_request", payload.clone()).await;
        let (selected, decided_by, console_choice) = match receiver {
            Some(receiver) => {
                let choice = self.wait_for_permission(&payload, receiver).await?;
                let selected = if remembers {
                    match choice.as_deref() {
                        Some("once" | "command" | "family") => allowing_agent_option(params),
                        Some("reject") => console_rejecting_option(params),
                        _ => None,
                    }
                } else {
                    choice.clone()
                };
                (selected, "console", choice)
            }
            None if ai_permissions_selection.is_some() => (ai_permissions_selection, "ai", None),
            None if learned.is_some() => (allowing_agent_option(params), "learned", None),
            None => (approved_option(params), "auto", None),
        };
        if self.permission_mode == PermissionMode::Ai {
            tracing::info!(
                tool = %signature.tool_name,
                decided_by,
                label,
                danger,
                allow_threshold,
                deny_threshold,
                ai_error,
                operation,
                risk_tags = ?risk_tags,
                cap,
                probabilities = ?probabilities,
                "AI permission decision"
            );
        }
        // Every choice a person makes, and every denial, is training data
        // for the model: allows the daemon made itself are not.
        let recorded = match decided_by {
            "console" => true,
            "ai" => matches!(&ai_permissions_decision, Some(Decision::Deny { .. })),
            _ => false,
        };
        if let Some(selected_option) = selected.clone().filter(|_| recorded)
            && !self.repository_id.is_empty()
        {
            // An unavailable model has no output.
            let output = ai_permissions_decision
                .as_ref()
                .filter(|decision| {
                    !matches!(
                        decision,
                        Decision::Unanswered {
                            reason: "unavailable",
                            ..
                        }
                    )
                })
                .map(|_| {
                    json!({"label": label, "danger": danger,
                       "allow_threshold": allow_threshold, "deny_threshold": deny_threshold,
                       "probabilities": probabilities, "operation": operation,
                       "risk_tags": risk_tags, "cap": cap, "ai_error": ai_error})
                });
            let level = match (decided_by, console_choice.as_deref()) {
                ("console", Some("command")) => "command",
                ("console", Some("family")) => "family",
                ("console", _) => "once",
                _ => "command",
            };
            self.sink
                .runtime
                .inner
                .store
                .record_learned_permission(ariadne_store::NewLearnedPermission {
                    repository_id: self.repository_id.clone(),
                    tool_name: signature.tool_name.clone(),
                    key: if level == "family" {
                        key.family.clone()
                    } else {
                        key.key.clone()
                    },
                    level: level.into(),
                    family: key.family.clone(),
                    risk_tags: request_tags.clone(),
                    scope: "repository".into(),
                    tool_call: params["toolCall"].clone(),
                    options: params.get("options").cloned().unwrap_or_default(),
                    selected_option,
                    target: self.permission_mode.as_str().to_string(),
                    output,
                })
                .await
                .map_err(|error| anyhow!("recording ACP permission choice: {error}"))?;
        }
        let outcome = selected.as_ref().map_or_else(
            || json!({"outcome": "cancelled"}),
            |option_id| json!({"outcome": "selected", "optionId": option_id}),
        );
        let learned_reply = learned.as_ref().filter(|_| decided_by == "learned");
        let reply = PermissionReplyDto {
            session_id,
            option_id: selected,
            console_option_id: console_choice,
            decided_by: decided_by.into(),
            label: label.map(str::to_string),
            danger,
            allow_threshold,
            deny_threshold,
            ai_error: ai_error.map(str::to_string),
            operation: operation.map(str::to_string),
            risk_tags: risk_tags.map(|tags| tags.into_iter().map(str::to_string).collect()),
            cap: cap.map(str::to_string),
            probabilities,
            learned_id: learned_reply.map(|row| row.id.clone()),
            learned_level: learned_reply.map(|row| row.level.clone()),
            learned_key: learned_reply.map(|row| row.key.clone()),
        };
        self.sink.record_stat_fact("permission", json!({
            "tool_name": signature.tool_name,
            "decided_by": decided_by,
            "answer": permission_answer(reply.option_id.as_deref(), reply.label.as_deref(), params),
            "console_option_id": reply.console_option_id,
            "wait_ms": requested_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        }));
        self.sink.emit("permission.replied", json!(reply)).await;
        Ok(json!({"outcome": outcome}))
    }

    /// Find an allowing command row, then a family row, whose risk tags hold
    /// every tag of the request. A
    /// request with no `rawInput` never matches one.
    async fn learned_allow(
        &self,
        signature: &PermissionSignature,
        key: &Key,
        request_tags: &[String],
    ) -> Option<ariadne_store::LearnedPermission> {
        if signature.raw_input.is_none() || self.repository_id.is_empty() {
            return None;
        }
        for (level, row_key) in [("command", &key.key), ("family", &key.family)] {
            let row = self
                .sink
                .runtime
                .inner
                .store
                .find_learned_permission(&self.repository_id, &signature.tool_name, level, row_key)
                .await
                .ok()
                .flatten();
            if let Some(row) = row {
                let options = serde_json::from_str::<Value>(&row.options).unwrap_or_default();
                let row_tags =
                    serde_json::from_str::<Vec<String>>(&row.risk_tags).unwrap_or_default();
                if allowing_option(&json!({ "options": options }), &row.selected_option)
                    && request_tags.iter().all(|tag| row_tags.contains(tag))
                {
                    return Some(row);
                }
            }
        }
        None
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
        choices: &Value,
        receiver: oneshot::Receiver<String>,
    ) -> Result<Option<String>> {
        let answer = receiver
            .await
            .map_err(|_| anyhow!("ACP permission answer channel closed"))?;
        Ok(permission_option(choices, &answer))
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
    switches: mpsc::UnboundedReceiver<ConfigSwitch>,
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
    let mut options = if loose && resumed {
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
        pin_or_fall_back(
            rpc,
            &session_id,
            setup.config_options,
            &["model"],
            &["model"],
            "model",
            &config.model,
        )
        .await
    };
    if let Some(effort) = &config.effort {
        options = pin_or_fall_back(
            rpc,
            &session_id,
            options,
            &["thought_level"],
            &["effort", "reasoning", "thought_level"],
            "effort",
            effort,
        )
        .await;
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
            delivery: None,
        };
        prompt_once(rpc, &session_id, &config.system_prompt, &prompt).await?;
    }
    serve_with_input(
        rpc,
        &session_id,
        &config.system_prompt,
        prompts,
        switches,
        options,
    )
    .await
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
/// `session/prompt` is outstanding. A prompt that carries an agent message
/// is claimed before it goes out, and skipped where the claim fails.
async fn serve_with_input(
    rpc: &mut Rpc,
    session_id: &str,
    system_prompt: &str,
    mut prompts: mpsc::UnboundedReceiver<Prompt>,
    mut switches: mpsc::UnboundedReceiver<ConfigSwitch>,
    mut options: Vec<v1::SessionConfigOption>,
) -> Result<()> {
    // Once the console side is gone there is nothing left to queue, but the
    // agent may still have plenty to say — the branch is dropped rather than
    // polled into a busy loop of immediate `None`s. An agent being killed
    // starts nothing more either.
    let mut console_open = true;
    let mut switches_open = true;
    let closing = rpc.closing.clone();
    loop {
        tokio::select! {
            biased;
            change = switches.recv(), if switches_open && !closing.load(Ordering::SeqCst) => {
                match change {
                    Some(change) => {
                        let result = async {
                            options = set_pinned_option(rpc, session_id, options.clone(), &["model"], &["model"], "model", &change.model).await?;
                            if let Some(effort) = &change.effort {
                                options = set_pinned_option(rpc, session_id, options.clone(), &["thought_level"], &["effort", "reasoning", "thought_level"], "effort", effort).await?;
                            }
                            Ok(())
                        }.await;
                        let _ = change.done.send(result);
                    }
                    None => switches_open = false,
                }
            }
            prompt = prompts.recv(), if console_open && !closing.load(Ordering::SeqCst) => {
                match prompt {
                    Some(prompt) => {
                        if !claim_message(rpc, &prompt).await {
                            continue;
                        }
                        // A failed turn leaves its claim to the driver, which
                        // gives it back where the prompt was never written.
                        prompt_once(rpc, session_id, system_prompt, &prompt).await?;
                        rpc.in_flight.settle();
                        // This exact delivery's own turn just ended — not
                        // merely some turn on this session, which could be
                        // an unrelated one that landed first. Confirmed
                        // here, directly from what this prompt carried,
                        // rather than from any side record a later pass
                        // could race against.
                        if let Some(Delivery::GoalAttention {
                            goal_id,
                            failed_tasks,
                        }) = &prompt.delivery
                        {
                            let _ = rpc
                                .sink
                                .runtime
                                .inner
                                .store
                                .confirm_goal_orchestrator_answered(goal_id, failed_tasks)
                                .await;
                        }
                    }
                    None => console_open = false,
                }
            }
            () = rpc.connection.incoming_closed() => return Ok(()),
        }
    }
}

/// Claim the delivery a prompt carries, right before the prompt goes out
/// (018, 026). Answers whether the prompt goes out. A prompt that carries no
/// delivery always does. One whose message a read already took does not:
/// the agent has that text, and the prompt would say it twice. A pull
/// request's news is claimed by writing its told marks.
async fn claim_message(rpc: &Rpc, prompt: &Prompt) -> bool {
    let Some(delivery) = &prompt.delivery else {
        return true;
    };
    let sink = &rpc.sink;
    sink.runtime
        .dequeue_message(&sink.session_id, &sink.launch_id, &delivery.key());
    let store = &sink.runtime.inner.store;
    let claimed = match delivery {
        Delivery::Step(id) => store.claim_step_briefing(id).await,
        Delivery::Message(id) => store.mark_message_delivered(id).await,
        Delivery::PullRequestNews(news) => {
            store
                .claim_pull_request_news(
                    &news.pull_request_id,
                    &news.comment_ids,
                    &news.before,
                    &news.told,
                )
                .await
        }
        // Nothing to claim ahead of the send: `tell_orchestrator`'s own
        // `goal_told` map already keeps this situation from being queued
        // twice, and what this delivery confirms is written only once its
        // own turn ends (`serve_with_input`), never before.
        Delivery::GoalAttention { .. } => Ok(true),
    };
    match claimed {
        Ok(true) => {
            rpc.in_flight.claim(delivery.clone());
            true
        }
        // A message a read took first is routine. A column entry is not: it
        // is briefed once, and the agent whose briefing went elsewhere has
        // nothing else to start from.
        Ok(false) if matches!(delivery, Delivery::Step(_)) => {
            tracing::warn!(session = %sink.session_id, delivery = %delivery.key(), "the column entry was briefed first, or is no longer current; its briefing is skipped");
            false
        }
        Ok(false) => {
            tracing::debug!(session = %sink.session_id, delivery = %delivery.key(), "the delivery was taken first, or its news is stale; its prompt is skipped");
            false
        }
        // Unclaimed, so the next scheduler pass hands it again.
        Err(error) => {
            tracing::warn!(session = %sink.session_id, delivery = %delivery.key(), error = %format!("{error:#}"), "claiming the delivery failed");
            false
        }
    }
}

/// The delivery whose prompt is going out, and whether that prompt was
/// written. It lives outside the protocol, so the driver still reads it after
/// a failed write or a kill took the protocol down.
#[derive(Clone, Default)]
struct MessageInFlight {
    claimed: Arc<Mutex<Option<Delivery>>>,
    writes: PromptWrites,
}

impl MessageInFlight {
    /// The driver claimed this delivery, and its prompt goes out next.
    fn claim(&self, delivery: Delivery) {
        self.writes.reset();
        *self.claimed.lock().expect("message in flight lock") = Some(delivery);
    }

    /// The prompt's turn ended: the agent had the text.
    fn settle(&self) {
        self.claimed.lock().expect("message in flight lock").take();
    }

    /// The claimed delivery whose prompt the agent's stdin never took whole.
    /// A prompt that was written keeps its claim, whatever came after: the
    /// agent may have read it.
    fn unwritten(&self) -> Option<Delivery> {
        let claimed = self
            .claimed
            .lock()
            .expect("message in flight lock")
            .take()?;
        (!self.writes.written()).then_some(claimed)
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

/// Pin one option of a launch, or carry on without it.
///
/// A launch whose pin does not land — the agent offers no such option,
/// refuses the value, fails the call, or settles on another value — runs on
/// whatever the agent runs: its own default, or what the resumed
/// conversation ran on. A `session.pin_fallback` event names the pin and what
/// the agent runs instead. The row keeps the pin, so the next launch asks for
/// it again.
async fn pin_or_fall_back(
    rpc: &mut Rpc,
    session_id: &str,
    options: Vec<v1::SessionConfigOption>,
    categories: &[&str],
    names: &[&str],
    label: &str,
    value: &str,
) -> Vec<v1::SessionConfigOption> {
    let Unpinned { options, error } =
        match set_pinned_option(rpc, session_id, options, categories, names, label, value).await {
            Ok(options) => return options,
            Err(unpinned) => unpinned,
        };
    let running = find_config_option(&options, categories, names)
        .and_then(current_value)
        .map(str::to_string);
    let error = format!("{error:#}");
    tracing::warn!(session = %rpc.sink.session_id, label, value, ?running, %error, "a pin did not land, running on the agent's own");
    rpc.sink
        .emit(
            "session.pin_fallback",
            json!({
                "session_id": session_id,
                "option": label,
                "asked": value,
                "running": running,
                "error": error,
            }),
        )
        .await;
    options
}

/// A pin that did not land, and the options as the agent last reported
/// them: what a launch carries on from. Where the pin must land — a switch —
/// it is the error alone.
#[derive(Debug)]
struct Unpinned {
    options: Vec<v1::SessionConfigOption>,
    error: anyhow::Error,
}

impl std::fmt::Display for Unpinned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.error)
    }
}

impl std::error::Error for Unpinned {}

async fn set_pinned_option(
    rpc: &mut Rpc,
    session_id: &str,
    options: Vec<v1::SessionConfigOption>,
    categories: &[&str],
    names: &[&str],
    label: &str,
    value: &str,
) -> std::result::Result<Vec<v1::SessionConfigOption>, Unpinned> {
    let Some(option) = find_config_option(&options, categories, names) else {
        let error = anyhow!("ACP agent did not offer a {label} configuration option");
        return Err(Unpinned { options, error });
    };
    let config_id = option.id.clone();
    // The value rides flattened into the request, which is what puts a
    // select option's id at `value` and a boolean's `type` beside it.
    let request = v1::SetSessionConfigOptionRequest::new(
        session_id.to_string(),
        config_id,
        v1::SessionConfigOptionValue::value_id(value.to_string()),
    );
    let response = match rpc
        .call("session/set_config_option", request)
        .await
        .with_context(|| format!("setting ACP {label} to `{value}`"))
    {
        Ok(response) => response,
        Err(error) => return Err(Unpinned { options, error }),
    };
    let options = match response.config_options.is_empty() {
        true => options,
        false => response.config_options,
    };
    // An agent that took the option answers with it set. One that did not —
    // it read the value and made nothing of it — answers the same way, and
    // the session would run on the agent's own model at the agent's own
    // effort while Ariadne believed it was pinned. It is an error instead,
    // so a pin that does not land is heard about rather than paid for.
    let option = find_config_option(&options, categories, names);
    match option.and_then(current_value) {
        Some(settled) if settled == value => Ok(options),
        Some(settled) if option.is_some_and(|option| respelled(option, value, settled)) => {
            Ok(options)
        }
        Some(settled) => {
            let error = anyhow!("ACP agent kept {label} on `{settled}` when asked for `{value}`");
            Err(Unpinned { options, error })
        }
        // Nothing to check against: an agent that reports no current value
        // is taken at its word, as it was before this was checked at all.
        None => Ok(options),
    }
}

/// Whether `settled` is `asked` under the spelling the agent offers it by
/// now: the same id without its context hint (`claude-fable-5-1` for
/// `claude-fable-5-1[1m]`), on an option that no longer offers `asked` at all.
///
/// Claude's agent lists a model without the hint on a resumed conversation
/// that it lists with the hint on a new one — the same row of its picker,
/// under another id — and settles a pin of the hinted id on that row. Where
/// the hinted id is still offered beside the bare one, the bare one is a
/// smaller context window, and a pin that lands on it did not land.
fn respelled(option: &v1::SessionConfigOption, asked: &str, settled: &str) -> bool {
    let v1::SessionConfigKind::Select(select) = &option.kind else {
        return false;
    };
    let offered: Vec<&v1::SessionConfigSelectOption> = match &select.options {
        v1::SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect(),
        v1::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .collect(),
        _ => Vec::new(),
    };
    let unhinted = asked
        .strip_suffix(']')
        .and_then(|rest| rest.rsplit_once('['))
        .map(|(model, _)| model);
    unhinted == Some(settled)
        && !offered
            .iter()
            .any(|choice| choice.value.0.as_ref() == asked)
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
    let command = rpc.turn.lock().await.is_command(&prompt.text);
    let full = match command {
        true => prompt.text.clone(),
        false => format!("{system_prompt}\n\n{}", prompt.text),
    };
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
    if response
        .pointer("/_meta/jetbrains/air/sessionFailure/category")
        .and_then(Value::as_str)
        == Some("limit")
    {
        return Err(anyhow::Error::new(
            agent_client_protocol::Error::new(
                -32603,
                "the prompt failed because the model limit was reached",
            )
            .data(response),
        ))
        .context("ACP session/prompt failed");
    }
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

/// The agent's own name for the tool: `/name`, `/_meta/claudeCode/toolName`,
/// `/title`, `/toolCallId`, in order; `"ACP tool"` where the call names none.
/// Used by [`permission_signature`] to name a tool consistently.
fn tool_name_of(tool_call: &Value) -> String {
    [
        "/name",
        "/_meta/claudeCode/toolName",
        "/title",
        "/toolCallId",
    ]
    .into_iter()
    .find_map(|pointer| tool_call.pointer(pointer).and_then(Value::as_str))
    .unwrap_or("ACP tool")
    .to_string()
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

/// Prefer a one-time agent approval for a console or learned answer.
fn allowing_agent_option(params: &Value) -> Option<String> {
    let options = params.get("options").and_then(Value::as_array)?;
    for kind in ["allow_once", "allow_always"] {
        if let Some(id) = options
            .iter()
            .find(|option| option.get("kind").and_then(Value::as_str) == Some(kind))
            .and_then(|option| option.get("optionId"))
            .and_then(Value::as_str)
        {
            return Some(id.to_string());
        }
    }
    None
}

/// The choices Ariadne offers when a request in `learn` or `ai` reaches the console.
fn learned_choices(params: &Value, family: &str) -> Value {
    let mut choices = Vec::new();
    if allowing_agent_option(params).is_some() {
        choices.extend([
            json!({"optionId": "once", "name": "Allow once", "kind": "allow_once"}),
            json!({"optionId": "command", "name": "Allow this command", "kind": "allow_always"}),
            json!({"optionId": "family", "name": format!("Allow every {family} call"), "kind": "allow_always"}),
        ]);
    }
    if console_rejecting_option(params).is_some() {
        choices.push(json!({"optionId": "reject", "name": "Reject", "kind": "reject_once"}));
    }
    Value::Array(choices)
}

/// The first one-time rejection. A permanent rejection is never selected by
/// the model because it changes future requests too.
fn rejecting_option(params: &Value) -> Option<String> {
    params
        .get("options")
        .and_then(Value::as_array)?
        .iter()
        .find(|option| option.get("kind").and_then(Value::as_str) == Some("reject_once"))?
        .get("optionId")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Keep a rejection available to a person when the agent offers only a
/// permanent rejection. A model still uses [`rejecting_option`] alone.
fn console_rejecting_option(params: &Value) -> Option<String> {
    rejecting_option(params).or_else(|| {
        params
            .get("options")
            .and_then(Value::as_array)?
            .iter()
            .find(|option| option.get("kind").and_then(Value::as_str) == Some("reject_always"))?
            .get("optionId")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

/// The key of a learned permission, with the repository the request came
/// from: the tool name and the tool's input.
struct PermissionSignature {
    tool_name: String,
    raw_input: Option<Value>,
}

/// The tool name is the agent's own name for the tool where it sends one
/// (`Bash`, `Read`, `mcp__ariadne__create_task`), else the title.
fn permission_signature(tool_call: &Value) -> PermissionSignature {
    let tool_name = tool_name_of(tool_call);
    let raw_input = tool_call
        .get("rawInput")
        .filter(|raw_input| !raw_input.is_null())
        .cloned();
    PermissionSignature {
        tool_name,
        raw_input,
    }
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

/// Whether the selected option is an approval, rather than a denial: its
/// kind is `allow_once` or `allow_always`.
fn allowing_option(params: &Value, option_id: &str) -> bool {
    params
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|option| option.get("optionId").and_then(Value::as_str) == Some(option_id))
        .and_then(|option| option.get("kind").and_then(Value::as_str))
        .is_some_and(|kind| matches!(kind, "allow_once" | "allow_always"))
}

/// The normalized outcome of a permission answer. The option is authoritative
/// where it names an agent choice; a decision label fills the AI case.
fn permission_answer(option_id: Option<&str>, label: Option<&str>, params: &Value) -> &'static str {
    let kind = option_id.and_then(|id| {
        params
            .get("options")
            .and_then(Value::as_array)?
            .iter()
            .find(|option| option.get("optionId").and_then(Value::as_str) == Some(id))?
            .get("kind")
            .and_then(Value::as_str)
    });
    if kind.is_some_and(|kind| kind.starts_with("allow")) || label == Some("allow") {
        "allow"
    } else if option_id.is_none() {
        "cancelled"
    } else {
        "deny"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        approved_option, completed_tool_payload, permission_signature, usage_for_prompt_response,
    };

    use ariadne_core::TokenUsage;
    use serde_json::json;

    /// The tool name is the first string among `name`, the Claude tool name
    /// and the title: a null candidate does not hide the next one.
    #[test]
    fn the_tool_name_is_the_first_string_candidate() {
        let name = |tool_call| permission_signature(&tool_call).tool_name;
        assert_eq!(name(json!({"name": "Bash", "title": "ls"})), "Bash");
        assert_eq!(
            name(
                json!({"name": null, "_meta": {"claudeCode": {"toolName": "Read"}},
                        "title": "Read /a"})
            ),
            "Read"
        );
        assert_eq!(name(json!({"name": null, "title": "ls"})), "ls");
        assert_eq!(name(json!({"toolCallId": "call-1"})), "call-1");
        assert_eq!(name(json!({})), "ACP tool");
    }

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

    /// Every permission request is allowed: the allowing option wins
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

    /// A kill's gate holds the session's next launch back: the launch waits
    /// for it as it waits for an agent still ending, and goes on once the
    /// gate is dropped. Here the launch then fails, on a program that is not
    /// there, which is the proof that it got past the wait.
    #[tokio::test]
    async fn a_launch_waits_for_the_gate_of_a_kill() {
        let dir = tempfile::tempdir().unwrap();
        let store = ariadne_store::Store::open(dir.path().join("test.db"))
            .await
            .unwrap();
        let runtime = super::AcpRuntime::new(store);
        let (reaped, gate) = runtime.kill_gated("session");
        reaped.await;
        let launching = runtime.clone();
        let launch = tokio::spawn(async move {
            launching
                .launch(super::AcpLaunch {
                    session_id: "session".into(),
                    launch_id: "launch".into(),
                    program: dir.path().join("no-such-agent").display().to_string(),
                    agent_id: "stub".into(),
                    args: vec![],
                    env: vec![],
                    cwd: dir.path().to_path_buf(),
                    config: ariadne_core::acp::LaunchConfig {
                        version: 1,
                        system_prompt: String::new(),
                        initial_prompt: None,
                        model: "stub:test-model".into(),
                        effort: None,
                        resume_session_id: None,
                        mcp_servers: vec![],
                    },
                    repository_id: "repo".into(),
                    repository_path: String::new(),
                    permission_mode: ariadne_core::PermissionMode::Auto,
                })
                .await
        });
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert!(!launch.is_finished(), "the launch went past the gate");

        drop(gate);
        let launched = tokio::time::timeout(std::time::Duration::from_secs(10), launch)
            .await
            .expect("the launch goes on once the gate is dropped")
            .unwrap();
        assert!(launched.is_err(), "the program is not there");
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
