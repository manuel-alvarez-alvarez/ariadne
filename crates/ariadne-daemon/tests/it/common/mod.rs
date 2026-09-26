//! One daemon in a temporary directory, and the scaffolding every integration
//! test in this crate drives it with.
//!
//! A harness is a real store, a real launcher, the axum router the daemon
//! serves and — where a test asks for one — a real scheduler, all pointed at a
//! `TempDir` that goes when the test does. The agents are the scriptable stub
//! ACP agent of [`acp`]: unless a test hands it a home of its own
//! ([`HarnessBuilder::home`]), the harness registers one as the registry
//! agent `stub`, so every session the daemon spawns runs a real stub process
//! driven over the protocol, and [`Harness::agent_runs`] puts one under a
//! session a test seeded itself.
//!

pub(crate) mod acp;

use std::future::IntoFuture;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::UnboundedSender;
use tower::ServiceExt;

use ariadne_api::SESSION_HEADER;
use ariadne_api::error::ErrorBody;
use ariadne_api::events::IngestEventRequest;
use ariadne_api::stream::DomainEvent;
use ariadne_core::acp::LaunchConfig;
use ariadne_core::models::agent_of;
use ariadne_core::{
    Actor, AttentionReason, GoalStatus, MessageKind, PermissionMode, Seat, SessionStatus,
    TaskStatus,
};
use ariadne_daemon::acp::AcpLaunch;
use ariadne_daemon::branch::BranchWatchers;
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::log::LogBuffer;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::timeouts::Timeouts;
use ariadne_daemon::transcript::TranscriptHomes;
use ariadne_store::{
    AgentPin, AgentSession, Goal, NewAgentEvent, NewGoal, NewMessage, NewRepository, NewSession,
    NewTask, NewTaskAgent, Repository, RepositoryUpdate, SessionFilter, Store, Task, TaskAgent,
};

/// How long a test waits for something the daemon does off the request path —
/// a reconciliation, an event, a delivery — before giving up.
///
/// Generous because some of what is waited on is not the daemon thinking: a
/// stub agent is a python process the daemon starts and talks to, and every
/// test in the crate runs beside the others.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);

/// How long a test listens for something that must not happen.
///
/// Short, because it cannot fail a test that is right: a longer listen only
/// catches a wrong event that comes later still. Everything it listens for is
/// the daemon reacting inside its own process, and the branch watch, the
/// slowest of those, debounces for 50 ms.
pub(crate) const QUIET: Duration = Duration::from_millis(500);

/// A daemon timeout for a test that is about that timeout running out. The
/// test waits it out, so it is short, and nothing the test needs done in time
/// runs under it.
pub(crate) const RUNS_OUT: Duration = Duration::from_millis(500);

/// The registry id the harness registers its stub agent under.
pub(crate) const STUB: &str = "stub";

pub(crate) struct Harness {
    pub store: Store,
    pub launcher: Arc<Launcher>,
    pub router: Router,
    pub state: AppState,
    pub bus: EventBus,
    pub logs: LogBuffer,
    /// Present when the harness was built with [`HarnessBuilder::scheduler`].
    pub sched: Option<UnboundedSender<SchedEvent>>,
    /// The stub agent the harness registers as [`STUB`] — whatever the home,
    /// and the one [`Harness::agent_runs`] starts.
    pub agent: acp::StubAcpAgent,
    pub dir: tempfile::TempDir,
    /// How long this harness waits on an agent, as the builder was given it:
    /// what a test starting a scheduler of its own hands that scheduler.
    pub timeouts: Timeouts,
    /// One connection of this test's own to the database the store is on, for
    /// the columns a test writes behind the store's back. One, and kept: a
    /// pool per write would be a handful of file descriptors opened and closed
    /// for every clock a test moves, and thirty tests doing that at once run a
    /// machine out of them.
    db: sqlx::SqlitePool,
}

pub(crate) struct HarnessBuilder {
    home: Option<PathBuf>,
    scheduler: bool,
    spawns: bool,
    dies: bool,
    logs: Option<LogBuffer>,
    discover_agents: bool,
    timeouts: Timeouts,
    path: std::ffi::OsString,
    index: String,
    ai_permissions_installer: Option<Vec<String>>,
    ai_permissions_serve_command: Option<Vec<String>>,
    ai_permissions_endpoint: Option<String>,
    python_bin: Option<String>,
    ai_permissions_release_url: Option<String>,
}

/// The pin the fixtures staff an agent on: a model of the registry agent the
/// harness registers. A model is required everywhere, so every seeded row
/// names one, and a test that cares which model it is names its own.
pub(crate) fn test_pin() -> AgentPin {
    AgentPin {
        model: format!("{STUB}:test-model"),
        effort: None,
    }
}

/// A daemon in a temporary directory, its registry holding the stub agent,
/// and no scheduler.
///
/// The `PATH` its registry searches is empty, so no agent of the ACP registry
/// index is found on the machine running the tests: the agents of a harness
/// are the ones it registers itself, unless a test hands it a `PATH` of its
/// own ([`HarnessBuilder::agents_on_path`]).
pub(crate) fn harness() -> HarnessBuilder {
    HarnessBuilder {
        home: None,
        scheduler: false,
        spawns: true,
        dies: false,
        logs: None,
        discover_agents: false,
        timeouts: Timeouts::default(),
        path: std::ffi::OsString::new(),
        index: ariadne_daemon::acp_discovery::SHIPPED_INDEX.to_string(),
        ai_permissions_installer: None,
        ai_permissions_serve_command: None,
        ai_permissions_endpoint: None,
        python_bin: None,
        ai_permissions_release_url: None,
    }
}

impl HarnessBuilder {
    /// Build the daemon around an already prepared home directory — a
    /// `config.toml` in it is read as `ariadned` would read it, its registry
    /// included.
    pub(crate) fn home(mut self, home: PathBuf) -> Self {
        self.home = Some(home);
        self
    }

    /// Run a real scheduler behind the router, as the daemon does. No sleep
    /// inhibition: nothing in a test runs long enough to matter.
    pub(crate) fn scheduler(mut self) -> Self {
        self.scheduler = true;
        self
    }

    /// A daemon that cannot start anything: the registry's stub agent names
    /// no executable, so every fresh session dies at the launch.
    pub(crate) fn cannot_spawn(mut self) -> Self {
        self.spawns = false;
        self
    }

    /// A daemon whose agent starts and exits at once: every launch works,
    /// and not one agent is ever heard from.
    pub(crate) fn dying_agent(mut self) -> Self {
        self.dies = true;
        self
    }

    /// Serve `/v1/logs` from a buffer the test already holds.
    pub(crate) fn logs(mut self, logs: LogBuffer) -> Self {
        self.logs = Some(logs);
        self
    }

    /// Run ACP registry discovery while the harness starts, on a home of the
    /// test's own ([`Self::home`]). The harness's own home is always
    /// discovered — the daemon discovers its registry at every start, and a
    /// resume is gated on what discovery measured — with the stub probed
    /// until discovery accepts it and the probes' traffic dropped from its
    /// log.
    pub(crate) fn discover_agents(mut self) -> Self {
        self.discover_agents = true;
        self
    }

    /// Wait on agents as long as `timeouts` says, rather than as long as a
    /// daemon does: for a test about one of them running out ([`RUNS_OUT`]).
    pub(crate) fn timeouts(mut self, timeouts: Timeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Search `path` for the agents of the registry index, as a daemon
    /// searches its own: what the machine running the daemon holds.
    /// [`acp::StubAcpAgent::path_with`] makes one out of the stub.
    pub(crate) fn agents_on_path(mut self, path: impl Into<std::ffi::OsString>) -> Self {
        self.path = path.into();
        self
    }

    /// Read `index` as the registry index, rather than the snapshot Ariadne
    /// ships: for a test about what the mapping makes of an entry.
    pub(crate) fn acp_index(mut self, index: impl Into<String>) -> Self {
        self.index = index.into();
        self
    }

    /// Run `cmd` in place of the whole model install — the venv, the wheel
    /// and the checkpoints (022). Its exit status decides `ready` or
    /// `failed`, and its stderr is `last_error`.
    pub(crate) fn ai_permissions_installer(mut self, cmd: Vec<String>) -> Self {
        self.ai_permissions_installer = Some(cmd);
        self
    }

    /// Run `cmd` as the model server instead of the installed `laya-serve`.
    pub(crate) fn ai_permissions_serve_command(mut self, cmd: Vec<String>) -> Self {
        self.ai_permissions_serve_command = Some(cmd);
        self
    }

    /// Answer `AiPermissions::endpoint` with `url`, in place of a server the daemon
    /// started.
    pub(crate) fn ai_permissions_endpoint(mut self, url: impl Into<String>) -> Self {
        self.ai_permissions_endpoint = Some(url.into());
        self
    }

    /// Install into `path` rather than whatever `python3` the machine
    /// running the tests has: a script that prints a version is a Python as
    /// far as the check is concerned.
    pub(crate) fn python_bin(mut self, path: impl Into<String>) -> Self {
        self.python_bin = Some(path.into());
        self
    }

    /// Read the AI permission model release document from `url`, rather than from GitHub.
    pub(crate) fn ai_permissions_release_url(mut self, url: impl Into<String>) -> Self {
        self.ai_permissions_release_url = Some(url.into());
        self
    }

    async fn build(self) -> Harness {
        raise_open_file_limit();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = Store::open(&db_path).await.unwrap();
        let agent_dir = dir.path().join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        let agent = acp::stub_acp_agent(&agent_dir, default_script());
        let own_home = self.home.is_none();
        let home = match self.home {
            Some(home) => home,
            None => {
                let home = dir.path().join("home");
                std::fs::create_dir_all(&home).unwrap();
                let command = match (self.spawns, self.dies) {
                    (false, _) => dir.path().join("no-such-agent").display().to_string(),
                    (true, true) => shared_script("#!/bin/sh\nexit 0\n").display().to_string(),
                    (true, false) => agent.bin.clone(),
                };
                std::fs::write(
                    home.join("config.toml"),
                    format!("[[acp_agents]]\nid = \"{STUB}\"\ncommand = [{command:?}]\n"),
                )
                .unwrap();
                home
            }
        };
        let mut config = Config::load(Some(home)).unwrap();
        // The model seams are the daemon's own settings rather than keys of
        // `config.toml`, so the harness writes them onto the config the way
        // the daemon would have read them.
        config.ai_permissions_installer = self.ai_permissions_installer;
        config.ai_permissions_serve_command = self.ai_permissions_serve_command;
        config.ai_permissions_endpoint = self.ai_permissions_endpoint;
        if let Some(python_bin) = self.python_bin {
            config.python_bin = Some(python_bin);
        }
        if let Some(url) = self.ai_permissions_release_url {
            config.ai_permissions_release_url = url;
        }
        let agent_registry = ariadne_daemon::acp_discovery::AgentRegistry::test_registry(
            &config.acp_agents,
            config.root.clone(),
            store.clone(),
            &self.path,
            &self.index,
        )
        .with_timeouts(self.timeouts);
        // Installed before anything writes, exactly as the daemon does at
        // startup.
        let bus = ariadne_daemon::bus::start(store.clone());
        let settle = own_home && self.spawns && !self.dies;
        let discover = self.discover_agents || settle;
        if discover {
            agent_registry.discover().await;
        }
        let ai_permissions = ariadne_daemon::ai_permissions::AiPermissions::new(
            store.clone(),
            bus.clone(),
            &config,
            self.timeouts,
        );
        ariadne_daemon::ai_permissions::schedule::start(
            ai_permissions.clone(),
            self.timeouts.ai_permissions_schedule_poll,
        );
        let launcher = Arc::new(Launcher {
            cfg: Arc::new(config),
            store: store.clone(),
            git: GitManager,
            acp: ariadne_daemon::acp::AcpRuntime::with_transcripts(
                store.clone(),
                self.timeouts,
                transcript_homes(dir.path()),
            )
            .with_ai_permissions(ai_permissions.clone()),
            registry: agent_registry.clone(),
            branches: BranchWatchers::new(bus.clone()),
        });
        let sched = self
            .scheduler
            .then(|| scheduler::start(store.clone(), launcher.clone(), false, self.timeouts));
        let logs = self.logs.unwrap_or_default();
        let state = AppState {
            store: store.clone(),
            started_at: Instant::now(),
            started_at_utc: chrono::Utc::now(),
            launcher: launcher.clone(),
            sched_tx: sched.clone(),
            events: bus.clone(),
            logs: logs.clone(),
            agent_registry,
            outside_sessions: ariadne_daemon::acp_sessions::OutsideSessions::with_transcripts(
                &transcript_homes(dir.path()),
            ),
            ai_permissions,
        };
        // Lazy: most tests never write behind the store's back, and a
        // connection opened for every harness in every binary is a hundred
        // file descriptors a full run has no use for.
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_lazy(&format!("sqlite://{}", db_path.display()))
            .unwrap();
        let h = Harness {
            router: http::router(state.clone()),
            state,
            store,
            launcher,
            bus,
            logs,
            sched,
            agent,
            dir,
            timeouts: self.timeouts,
            db,
        };
        // A probe under full-suite load can run out its timeout: probe again
        // until the stub is accepted, so no test reads a timed-out snapshot.
        if settle {
            let deadline = Instant::now() + TIMEOUT;
            while !h.launcher.registry.agents().await.iter().any(|agent| {
                agent.id == STUB && agent.status == ariadne_api::agents::AcpAgentStatus::Ready
            }) {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for discovery to accept the stub"
                );
                h.launcher.registry.refresh().await;
            }
            h.agent.clear_messages();
        }
        h
    }
}

/// Where a harness's agents keep their transcripts: under its own
/// directory, so no test reads the transcripts of the machine it runs on.
fn transcript_homes(dir: &Path) -> TranscriptHomes {
    TranscriptHomes {
        codex: dir.join("codex"),
        claude: dir.join("claude"),
        opencode: dir.join("opencode"),
    }
}

impl IntoFuture for HarnessBuilder {
    type Output = Harness;
    type IntoFuture = Pin<Box<dyn Future<Output = Harness> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.build())
    }
}

/// The script the harness's own stub runs: [`acp::script`], with the stored
/// conversations the fixtures resume ([`Harness::make_resumable`]) and the
/// one its own sessions start under, so a resume of either loads.
fn default_script() -> serde_json::Value {
    let mut script = acp::script();
    script["stored_sessions"] = serde_json::json!(["uuid-1234", "stub-session"]);
    script
}

/// The open files this binary needs, asked for once before the first daemon
/// starts.
///
/// Every test here runs a daemon of its own — a store with its pools, stub
/// agents, a scheduler — and libtest runs as many at once as the machine has
/// cores. Sixteen of them want around three hundred descriptors between them,
/// where a shell's default soft limit is two hundred and fifty-six, and what
/// that shortfall looks like is not "too many open files" on the test that
/// happened to ask last: it is a store that cannot be opened, a connection
/// that spends its busy timeout retrying, and a suite that fails somewhere
/// else entirely.
///
/// So the process raises its own soft limit towards its hard one. A machine
/// that will not have it is left exactly as it was — this makes a run
/// reliable, it does not make one possible.
fn raise_open_file_limit() {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

    // Not the hard limit itself, which is "unlimited" on macOS where the
    // kernel refuses anything over `kern.maxfilesperproc`: a few thousand is
    // under every such cap and many times what a full run holds at once.
    const WANTED: u64 = 4_096;
    static ONCE: std::sync::Once = std::sync::Once::new();

    ONCE.call_once(|| {
        let limit = getrlimit(Resource::Nofile);
        if limit.current.is_some_and(|current| current >= WANTED) {
            return;
        }
        let _ = setrlimit(
            Resource::Nofile,
            Rlimit {
                current: Some(limit.maximum.map_or(WANTED, |max| max.min(WANTED))),
                maximum: limit.maximum,
            },
        );
    });
}

/// An executable holding `script`, written once for every test that asks for
/// the same text.
///
/// macOS checks a new executable before its first launch, one file at a
/// time, and each check takes about a quarter of a second. A script written
/// for each test made every test in a parallel run wait for the check of
/// every other test. One file for each text is checked once, and a symlink to
/// it is not checked again.
pub(crate) fn shared_script(script: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    use std::os::unix::fs::PermissionsExt;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    script.hash(&mut hasher);
    let name = format!("{:016x}", hasher.finish());
    let dir = std::env::temp_dir().join("ariadne-test-scripts");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(&name);
    if !path.exists() {
        // Tests run in processes of their own: write aside and rename, so no
        // test ever launches a file another test is still writing.
        let partial = dir.join(format!("{name}.{}", std::process::id()));
        std::fs::write(&partial, script).unwrap();
        std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&partial, &path).unwrap();
    }
    path
}

// -- the stub agent, from the test's side -------------------------------------

impl Harness {
    /// Where this harness's agents keep their transcripts.
    pub(crate) fn transcript_homes(&self) -> TranscriptHomes {
        transcript_homes(self.dir.path())
    }

    pub(crate) fn at(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Start the harness's stub agent under a session the test seeded, the
    /// way a launch would have: a live agent process the runtime owns, with
    /// no briefing to answer. Returns once the agent's session start has
    /// landed all the way through — not merely recorded as an event row, but
    /// read back with the status `ingest_event` writes last — so what the
    /// test writes to the row afterwards is not written over by the rest of
    /// the handshake still draining behind it.
    pub(crate) async fn agent_runs(&self, session: &AgentSession) {
        let agent_id = agent_of(&session.model).to_string();
        self.agent_runs_as(session, &agent_id).await;
    }

    /// Start the harness's stub as the registry agent named by `agent_id`,
    /// and wait for its `session_start` to land.
    ///
    /// `session_start`'s ingestion (`ingest_event`) makes up to six writes,
    /// in this order:
    ///
    /// 1. `create_event` — publishes `AgentEventCreated`. A different
    ///    variant; a `SessionUpdated` waiter never sees it.
    /// 2. `upsert_session_usage`, only where the payload carries
    ///    `ariadne_usage` — publishes `SessionUpdated`, but the row exactly
    ///    as it stood before this event: neither the clock (5) nor the
    ///    status (6) below have run yet.
    /// 3. `set_session_internal_id`, only the first time this session
    ///    reports an agent-internal id — same: published ahead of (5)
    ///    and (6).
    /// 4. `set_session_attention` / `clear_agent_attention` /
    ///    `clear_attention_after_idle`, only where this event's kind
    ///    raises or clears a reason — same again.
    /// 5. `touch_session` — moves the clock, and publishes with it; the
    ///    status has not written yet, so this still carries whatever the
    ///    row's status already was, live or not.
    /// 6. `set_session_status_if_live` — the status goes last; for
    ///    `session_start` that is always `Running` (`status_for_event`).
    ///    This is the first publish carrying both the clock (5) moved and
    ///    the status (6) decided.
    ///
    /// Waiting on `status == Running && last_activity_at != before` rejects
    /// every publish through (5): (1) is a different event kind outright,
    /// and (2)-(4) all still carry the clock unmoved, since `before` is
    /// read immediately ahead of anything this launch writes — including
    /// `set_session_launch` below, whose own publish is stamped `before`
    /// for the same reason. (5) does carry the moved clock, but only reads
    /// `Running` if the row already said so ahead of this launch, which is
    /// exactly what the assert below forecloses: without it, a session
    /// left `Running` from an earlier call would make (5) indistinguishable
    /// from (6), because (6) would then match no row (already `Running`)
    /// and never publish at all. Every caller already either starts a
    /// session fresh or calls `set_status(.., Idle)` before reusing one;
    /// this only turns that into something enforced rather than assumed.
    pub(crate) async fn agent_runs_as(&self, session: &AgentSession, agent_id: &str) {
        let repository_id = match &session.task_id {
            Some(task) => self.store.get_task(task).await.unwrap().repo_id,
            None => {
                self.store
                    .list_goal_repositories(session.goal_id.as_deref().unwrap())
                    .await
                    .unwrap()
                    .remove(0)
                    .id
            }
        };
        let cwd = session
            .worktree_path
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| self.dir.path().to_path_buf());
        let row = self.store.get_session(&session.id).await.unwrap();
        assert_ne!(
            row.status(),
            SessionStatus::Running,
            "agent_runs_as waits for session_start's own status write; \
             called on a session already Running, touch_session's publish \
             would carry that same status and be indistinguishable from it"
        );
        let before = row.last_activity_at;
        let mut rx = self.bus.subscribe();
        // Stamped on the row before the process starts, the way a real
        // launch stamps it (`Launcher::launch`): a session run a second time
        // this way is a relaunch in every way that matters, launch id
        // included, not just a fresh process under an id the row never
        // heard of.
        let launch_id = ariadne_core::id::new_id();
        self.store
            .set_session_launch(&session.id, &launch_id)
            .await
            .unwrap();
        self.launcher
            .acp
            .launch(AcpLaunch {
                session_id: session.id.clone(),
                launch_id,
                program: self.agent.bin.clone(),
                agent_id: agent_id.to_string(),
                args: Vec::new(),
                env: vec![("ARIADNE_SESSION_ID".into(), session.id.clone())],
                cwd,
                config: LaunchConfig {
                    version: ariadne_core::acp::VERSION,
                    system_prompt: String::new(),
                    initial_prompt: None,
                    model: "test-model".into(),
                    effort: None,
                    resume_session_id: None,
                    mcp_servers: Vec::new(),
                },
                repository_id,
                permission_mode: PermissionMode::Auto,
            })
            .await
            .unwrap();
        // Waited for on the stream rather than polled off the row: a session
        // that runs its whole scripted turn before this is ever polled is
        // not missed the way it would be by a poll of the row, since every
        // publish in between is queued for this subscription regardless of
        // how fast the next one follows. See the doc comment above for why
        // this predicate specifically, and not a looser one, is what it
        // waits for.
        next_event(&mut rx, |e| {
            matches!(&e.event, DomainEvent::SessionUpdated(s)
                if s.id == session.id
                    && s.status == SessionStatus::Running
                    && s.last_activity_at != before)
        })
        .await;
    }

    /// Whether the runtime still owns an agent process for this session.
    pub(crate) fn agent_is_running(&self, session: &AgentSession) -> bool {
        self.launcher.acp.is_running(&session.id)
    }

    /// Every prompt the harness's stub agent was handed for this session, in
    /// order, as the agent read it.
    pub(crate) fn prompts_to(&self, session: &AgentSession) -> Vec<String> {
        self.agent.prompts_for(&session.id)
    }

    /// Whether this session's agent has been nudged, and has already turned
    /// that nudge's own turn over: the prompt recorded is only what the stub
    /// was handed, and a test that writes over the session right after can
    /// still be caught by that turn's own later reports landing — the same
    /// gap `agent_runs` closes for the first turn a session ever runs.
    pub(crate) async fn nudged(&self, session: &AgentSession) -> bool {
        !self.prompts_to(session).is_empty()
            && self.session_status(session).await == SessionStatus::Idle
    }

    /// Every prompt this session's agent was handed since its launch, as one
    /// text: what a delivery to it is read back from.
    pub(crate) fn prompted(&self, session: &AgentSession) -> String {
        self.prompts_to(session).join("\n\n")
    }

    /// Everything this session's agent was told: the system prompt and the
    /// briefing of its last launch, and every prompt the harness's stub was
    /// sent for it since — whichever way a briefing travelled, a relaunch or
    /// a prompt to an agent already up.
    pub(crate) fn told(&self, session_id: &str) -> String {
        let prompts = self.agent.prompts_for(session_id);
        let mut told = Vec::new();
        if let Some(launch) = self.launch_file(session_id) {
            told.push(launch.system_prompt);
            // The launch file's instruction and the first `session/prompt`
            // are one delivery, not two: the runtime sends the instruction
            // as the turn that opens the session. Counted from both, a
            // briefing reads as having reached the agent twice — and only
            // once the agent has got as far as recording it, which is a race
            // on the agent rather than on what it was told.
            if let Some(initial) = launch.initial_prompt
                && !prompts.iter().any(|prompt| prompt.contains(&initial))
            {
                told.push(initial);
            }
        }
        told.extend(prompts);
        told.join("\n\n")
    }

    /// The launch file the adapter last wrote for this session: what its
    /// agent was told, pinned to and connected to.
    pub(crate) fn launch_file(&self, session_id: &str) -> Option<LaunchConfig> {
        let path = self.launcher.cfg.run_dir.join(session_id).join("acp.json");
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }
}

// -- seeding ----------------------------------------------------------------

/// The agents of one goal with one task, and the repository behind it: every
/// agent that can be spawned for it.
///
/// The orchestrator is not among them: a goal has exactly one, it is the one
/// agent type Ariadne defines, and nothing staffs it.
pub(crate) struct Cast {
    pub goal: Goal,
    pub task: Task,
    pub repo: Repository,
    pub author: TaskAgent,
    pub reviewer: TaskAgent,
}

impl Harness {
    /// A toy git repo under the harness directory: `main` at one commit, and a
    /// `next` branch one commit ahead of it, checked out on `main`.
    pub(crate) fn git_repo(&self, name: &str) -> PathBuf {
        let repo = self.at(name);
        std::fs::create_dir_all(&repo).unwrap();
        sh(
            &repo,
            "git init -q -b main && echo v1 > file.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm init && \
             git checkout -q -b next && echo v2 > file.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm ahead && \
             git checkout -q main",
        );
        repo
    }

    /// A registered repository at `path`: a directory that exists, since an
    /// orchestrator is started in it, but only the tests that spawn an author
    /// ever have git look at it.
    pub(crate) async fn repository(&self, path: &Path) -> Repository {
        std::fs::create_dir_all(path).unwrap();
        self.store
            .create_repository(NewRepository {
                path: path.display().to_string(),
                base_branch: "main".into(),
                description: None,
                permission_mode: None,
            })
            .await
            .unwrap()
    }

    /// Set how a repository's sessions answer their permission requests.
    pub(crate) async fn set_permission_mode(&self, repo: &Repository, mode: PermissionMode) {
        self.store
            .update_repository(
                &repo.id,
                RepositoryUpdate {
                    permission_mode: Some(mode),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    /// One reviewer's verdict on the round a task stands in, as the reviewer
    /// itself would send it: a message to the author, of the kind that closes
    /// a round.
    pub(crate) async fn verdict(
        &self,
        task: &Task,
        reviewer_agent_id: &str,
        kind: MessageKind,
        body: &str,
    ) -> ariadne_store::Message {
        self.write_verdict(task, reviewer_agent_id, None, kind, body)
            .await
    }

    /// The same, sent from a live reviewer session, so the verdict names the
    /// session it came from.
    pub(crate) async fn verdict_from(
        &self,
        task: &Task,
        session: &AgentSession,
        kind: MessageKind,
        body: &str,
    ) -> ariadne_store::Message {
        let agent_id = session.task_agent_id.clone().unwrap_or_default();
        self.write_verdict(task, &agent_id, Some(&session.id), kind, body)
            .await
    }

    /// A verdict on the review that is open now, whichever that is: what a
    /// verdict belongs to is the request it answers, and the store reads that
    /// off the channel rather than off anything the caller holds.
    async fn write_verdict(
        &self,
        task: &Task,
        reviewer_agent_id: &str,
        session_id: Option<&str>,
        kind: MessageKind,
        body: &str,
    ) -> ariadne_store::Message {
        let task = self.store.get_task(&task.id).await.unwrap();
        let author = self.store.task_author(&task.id).await.unwrap();
        self.store
            .send_message(NewMessage {
                goal_id: task.goal_id.clone(),
                task_id: Some(task.id.clone()),
                kind,
                from_actor: Actor::Reviewer,
                from_agent_id: Some(reviewer_agent_id.to_string()),
                from_session: session_id.map(str::to_string),
                to_actor: Actor::Author,
                to_agent_id: Some(author.id),
                body: body.to_string(),
            })
            .await
            .unwrap()
    }

    /// A goal still in planning, on a repository of its own, pinned to the
    /// stub as [`Self::cast_reviewed_by`] pins its own.
    pub(crate) async fn goal(&self) -> (Goal, Repository) {
        let repo = self.repository(&self.at("repo")).await;
        let goal = self.goal_on(&repo, test_pin()).await;
        (goal, repo)
    }

    pub(crate) async fn goal_on(&self, repo: &Repository, pin: AgentPin) -> Goal {
        self.store
            .create_goal(NewGoal {
                title: "Ship the UI".into(),
                description: "desc".into(),
                repository_ids: vec![repo.id.clone()],
                pin,
            })
            .await
            .unwrap()
    }

    /// A task on a goal, staffed with one author and the reviewers given, all
    /// on the same pin.
    pub(crate) async fn task_on(
        &self,
        goal: &Goal,
        repo: &Repository,
        title: &str,
        reviewers: usize,
        pin: AgentPin,
    ) -> Task {
        let agent =
            |seat: Seat, skills: &[&str]| NewTaskAgent::new(seat, skills.to_vec(), pin.clone());
        let mut agents = vec![agent(Seat::Author, &["coding"])];
        agents.extend((0..reviewers).map(|_| agent(Seat::Reviewer, &["code-review"])));
        self.store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo.id.clone(),
                title: title.into(),
                description: "do things".into(),
                agents,
                depends_on: vec![],
                landing: None,
            })
            .await
            .unwrap()
    }

    /// A goal still in planning, with a repository behind it and nothing else:
    /// no task, so nothing but the orchestrator is under reconciliation.
    pub(crate) async fn planning_goal(&self) -> Goal {
        let (goal, _repo) = self.goal().await;
        goal
    }

    /// A goal in planning with one task on it, and the agents staffed on that
    /// task: the shape most tests start from.
    pub(crate) async fn cast(&self) -> Cast {
        self.cast_reviewed_by(1).await
    }

    /// The same, with `reviewers` reviewers on the task. A task is approved
    /// when every one of them has approved, so two of them is where a round
    /// one verdict does not close — a reviewer sitting with its work done.
    pub(crate) async fn cast_reviewed_by(&self, reviewers: usize) -> Cast {
        self.cast_pinned(&test_pin().model, reviewers).await
    }

    /// The same on another model: what a goal and a task's agents run on is
    /// what they were pinned to when they were created.
    pub(crate) async fn cast_pinned(&self, model: &str, reviewers: usize) -> Cast {
        let pin = AgentPin {
            model: model.to_string(),
            effort: None,
        };
        let repo = self.repository(&self.at("repo")).await;
        let goal = self.goal_on(&repo, pin.clone()).await;
        let task = self.task_on(&goal, &repo, "task", reviewers, pin).await;
        let author = self.store.task_author(&task.id).await.unwrap();
        let reviewer = self
            .store
            .list_task_reviewers(&task.id)
            .await
            .unwrap()
            .remove(0);
        Cast {
            goal,
            task,
            repo,
            author,
            reviewer,
        }
    }

    /// Move a staffed agent onto another model, which is what a `PATCH
    /// /v1/tasks/{id}` from the UI amounts to.
    pub(crate) async fn move_agent(&self, agent_id: &str, model: &str) {
        let pin = AgentPin {
            model: model.to_string(),
            effort: None,
        };
        self.store.set_agent_pin(agent_id, &pin).await.unwrap();
    }

    /// The same, with the goal out of planning: reconciliation only acts on an
    /// active goal.
    ///
    /// Returns only once the bus has published every seeding change — the pump
    /// preserves commit order, so seeing the last one means the earlier ones
    /// are out too — so a stream opened afterwards sees nothing but what the
    /// test itself does.
    pub(crate) async fn active_cast(&self) -> Cast {
        let mut rx = self.bus.subscribe();
        let mut cast = self.cast().await;
        cast.goal = self.activate(&cast.goal).await;
        next_event(
            &mut rx,
            |e| matches!(&e.event, DomainEvent::GoalUpdated(g) if g.status == GoalStatus::Active),
        )
        .await;
        cast
    }

    pub(crate) async fn activate(&self, goal: &Goal) -> Goal {
        self.store
            .set_goal_status(&goal.id, GoalStatus::Active)
            .await
            .unwrap()
    }

    /// A session of `seat`, as the launcher would have created it — a row,
    /// with no agent process under it until [`Self::agent_runs`] starts one.
    pub(crate) async fn session(
        &self,
        goal: &Goal,
        task: Option<&Task>,
        seat: Seat,
        agent_id: &str,
    ) -> AgentSession {
        self.new_session(goal, task, seat, Some(agent_id)).await
    }

    /// An orchestrator session on a goal of its own, on a repository named
    /// after `name`: the least a test that only cares about one session
    /// needs, and two names for two of them.
    pub(crate) async fn lone_session(&self, name: &str) -> AgentSession {
        let repo = self.repository(&self.at(&format!("repo-{name}"))).await;
        let goal = self.goal_on(&repo, test_pin()).await;
        // An orchestrator is staffed on no task, so its session carries no
        // agent.
        self.orchestrator_session(&goal).await
    }

    /// An orchestrator session on `goal`.
    pub(crate) async fn orchestrator_session(&self, goal: &Goal) -> AgentSession {
        self.new_session(goal, None, Seat::Orchestrator, None).await
    }

    async fn new_session(
        &self,
        goal: &Goal,
        task: Option<&Task>,
        seat: Seat,
        agent_id: Option<&str>,
    ) -> AgentSession {
        // A tree of its own per session, really there: what a resume comes
        // back in, and what a test can take away to see what happens when it
        // is not.
        let worktree = self.worktree_of(agent_id.unwrap_or(&goal.id));
        std::fs::create_dir_all(&worktree).unwrap();
        self.store
            .create_session(NewSession {
                goal_id: Some(goal.id.clone()),
                task_id: task.map(|t| t.id.clone()),
                seat: Some(seat),
                task_agent_id: agent_id.map(str::to_string),
                model: test_pin().model,
                effort: None,
                worktree_path: Some(worktree.display().to_string()),
            })
            .await
            .unwrap()
    }

    fn worktree_of(&self, id: &str) -> PathBuf {
        self.at(&format!("wt-{}", &id[id.len() - 4..]))
    }

    /// Take a session's row out from under the daemon, the way deleting the
    /// goal it belonged to would. Straight SQL: nothing an agent can call does
    /// this, which is the point — it is the state the daemon has to cope with,
    /// not one it is asked to produce.
    pub(crate) async fn forget_session(&self, session: &AgentSession) {
        sqlx::query("DELETE FROM agent_sessions WHERE id = ?")
            .bind(&session.id)
            .execute(&self.db)
            .await
            .unwrap();
    }

    /// A task whose author session has already run once: a worktree on disk,
    /// an agent conversation to resume, and no agent left running.
    /// What the launcher relaunches when the reviewers bounce a task back.
    pub(crate) async fn resumable_author(&self) -> (Cast, AgentSession) {
        let cast = self.cast().await;
        let session = self
            .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
            .await;
        self.make_resumable(&cast.task, &session).await;
        self.set_status(&session, SessionStatus::Exited).await;
        (cast, session)
    }

    /// What a relaunch needs to find: an agent conversation to resume and a
    /// tree to resume it in.
    pub(crate) async fn make_resumable(&self, task: &Task, session: &AgentSession) {
        let worktree = session.worktree_path.clone().expect("a session worktree");
        std::fs::create_dir_all(&worktree).unwrap();
        self.store
            .set_task_worktree(&task.id, Some(&worktree))
            .await
            .unwrap();
        self.store
            .set_session_internal_id(&session.id, "uuid-1234")
            .await
            .unwrap();
    }

    /// Walk a fresh task up to the status a test wants to watch it in, from
    /// wherever it stands: a scheduler woken by a live agent may already
    /// have taken it part of the way, and may take a step of the walk
    /// between this read of the status and this write of it.
    pub(crate) async fn advance(&self, task: &Task, to: TaskStatus) {
        let steps = [
            (TaskStatus::Ready, Actor::Daemon),
            (TaskStatus::InProgress, Actor::Daemon),
            (TaskStatus::UnderReview, Actor::Author),
        ];
        // How far up the walk a status stands. A status off the walk,
        // `pending` included, stands below every step of it.
        let reached = |status: TaskStatus| {
            steps
                .iter()
                .position(|(step, _)| *step == status)
                .map_or(0, |at| at + 1)
        };
        for (at, (status, actor)) in steps.into_iter().enumerate() {
            if reached(self.status(&task.id).await) <= at {
                let stepped = self
                    .store
                    .transition_task(&task.id, status, actor, None, None)
                    .await;
                if let Err(refused) = stepped {
                    // The scheduler took the step first, which is the step
                    // the test wanted. Anything else is a real failure.
                    let now = self.status(&task.id).await;
                    assert!(
                        reached(now) > at,
                        "the walk to {to:?} stopped at {now:?}: {refused}"
                    );
                }
            }
            if status == to {
                return;
            }
        }
    }

    /// One event recorded for an agent, straight into the store.
    pub(crate) async fn reports(&self, session: &AgentSession, kind: &str) {
        self.store
            .create_event(NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: session.task_id.clone(),
                kind: kind.into(),
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();
    }

    /// One event reported by an agent, down the ingestion path the ACP
    /// runtime reports on — the whole of it, rather than the store write at
    /// the end of it — and the scheduler woken, as the runtime wakes it.
    pub(crate) async fn ingest(
        &self,
        session: &AgentSession,
        kind: &str,
        payload: serde_json::Value,
    ) {
        self.ingest_as(session, None, kind, payload).await;
    }

    /// The same event, reported by a named launch of that session: what
    /// every agent the daemon starts reports under, and the only thing that
    /// tells the agent running from the one it replaced.
    pub(crate) async fn ingest_from(
        &self,
        session: &AgentSession,
        launch: &str,
        kind: &str,
        payload: serde_json::Value,
    ) {
        self.ingest_as(session, Some(launch), kind, payload).await;
    }

    async fn ingest_as(
        &self,
        session: &AgentSession,
        launch: Option<&str>,
        kind: &str,
        payload: serde_json::Value,
    ) {
        http::ingest_event(
            &self.store,
            &IngestEventRequest {
                session_id: session.id.clone(),
                launch: launch.map(str::to_string),
                kind: kind.to_string(),
                payload,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{kind}: {e}"));
        if let Some(sched) = &self.sched {
            let _ = sched.send(SchedEvent::SessionEvent(session.id.clone()));
        }
    }

    /// The launch this session's row is currently answering for.
    pub(crate) async fn launch_id(&self, session: &AgentSession) -> Option<String> {
        self.store.get_session(&session.id).await.unwrap().launch_id
    }

    /// Raise a flag on a session, the way the ingestion or a sweep would.
    pub(crate) async fn raise(&self, session: &AgentSession, reason: AttentionReason) {
        self.store
            .set_session_attention(&session.id, reason)
            .await
            .unwrap();
    }

    /// Move a session's lifecycle status, the way its agent reporting would.
    pub(crate) async fn set_status(&self, session: &AgentSession, status: SessionStatus) {
        self.store
            .set_session_status(&session.id, status)
            .await
            .unwrap();
    }

    pub(crate) async fn session_status(&self, session: &AgentSession) -> SessionStatus {
        self.store.get_session(&session.id).await.unwrap().status()
    }

    pub(crate) async fn attention(&self, session: &AgentSession) -> Option<AttentionReason> {
        self.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason()
    }

    /// Poke the scheduler about a task, the way an HTTP handler does after a
    /// write. Only for a harness built with [`HarnessBuilder::scheduler`].
    pub(crate) fn notify(&self, task_id: &str) {
        self.wake(SchedEvent::TaskChanged(task_id.to_string()));
    }

    /// Reconcile a task directly, then wait for its expected state.
    ///
    /// One notification is one complete reconciliation. Sending another on
    /// every poll can build a queue behind a slow agent launch, leaving the
    /// later state change waiting behind stale passes over the same task.
    pub(crate) async fn reconcile_task_until(
        &self,
        task_id: &str,
        patience: Duration,
        what: &str,
        check: impl AsyncFnMut() -> bool,
    ) {
        self.notify(task_id);
        eventually(patience, what, check).await;
    }

    /// The same about a goal: what a status change sends.
    pub(crate) fn notify_goal(&self, goal_id: &str) {
        self.wake(SchedEvent::GoalChanged(goal_id.to_string()));
    }

    /// Block until every event sent to the scheduler before this call has
    /// been reconciled to completion. A test that forces one pass to fail —
    /// a closed prompt channel, a branch that does not exist yet — sends
    /// this right after, so it knows the failing pass actually ran before it
    /// heals the failure and looks for the retry: a fixed sleep only bets
    /// that the pass was fast enough, and loses that bet under load.
    pub(crate) async fn flush_scheduler(&self) {
        ariadne_daemon::scheduler::flush_for_test(
            self.sched.as_ref().expect("this harness has no scheduler"),
        )
        .await;
    }

    fn wake(&self, event: SchedEvent) {
        self.sched
            .as_ref()
            .expect("this harness has no scheduler")
            .send(event)
            .unwrap();
    }

    pub(crate) async fn status(&self, task_id: &str) -> TaskStatus {
        self.store.get_task(task_id).await.unwrap().status()
    }

    /// Every session a goal has ever had, live or not — an orchestrator's
    /// included, which is the one no task lists.
    pub(crate) async fn sessions_of_goal(&self, goal_id: &str) -> Vec<AgentSession> {
        self.store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                ..Default::default()
            })
            .await
            .unwrap()
    }

    /// Every session a task has ever had, live or not.
    pub(crate) async fn sessions_of(&self, task_id: &str) -> Vec<AgentSession> {
        self.store
            .list_sessions(SessionFilter {
                task_id: Some(task_id.to_string()),
                ..Default::default()
            })
            .await
            .unwrap()
    }

    /// The session of `seat` that is up on the task, if there is one.
    ///
    /// Launched rather than merely live: a row is created before its agent is
    /// launched, and a test that reads what an agent was started with has to
    /// wait for the launch that wrote it down. Running or idle alike — a stub
    /// agent answers its briefing at once and sits at its prompt.
    pub(crate) async fn running_session(&self, task_id: &str, seat: Seat) -> Option<AgentSession> {
        self.sessions_of(task_id).await.into_iter().find(|s| {
            s.seat() == Some(seat)
                && matches!(s.status(), SessionStatus::Running | SessionStatus::Idle)
                && s.launched_at.is_some()
        })
    }

    /// Whether this session has been heard from, and settled idle, since
    /// `before` — a launch stamp read earlier, or `None` for one never
    /// launched.
    ///
    /// `launched_at` alone is not enough: `Launcher::launch` writes it, and
    /// then writes `running` itself the moment the process spawns, before the
    /// agent has said a word — a read taken on either alone can catch a
    /// relaunch on its way up rather than the agent actually reporting.
    /// Idle is not enough either until `heard_from` too: a resume carries a
    /// briefing, so the agent's own turn on it — `session_start`, the
    /// prompt, whatever it does with it, `stop` — keeps landing writes for a
    /// while after the launch, and a test that moved the clock before that
    /// turn settled would have it landed over. Idle, heard from, is the turn
    /// over and every write it made done: what a test can safely write over.
    pub(crate) async fn relaunched(&self, session: &AgentSession, before: &Option<String>) -> bool {
        let row = self.store.get_session(&session.id).await.unwrap();
        row.launched_at.is_some()
            && &row.launched_at != before
            && heard_from(&row)
            && row.status() == SessionStatus::Idle
    }

    // -- the clock ----------------------------------------------------------

    /// An agent that has been sitting there doing nothing for `secs`.
    pub(crate) async fn idle_for(&self, session: &AgentSession, secs: i64) {
        self.store
            .set_session_status(&session.id, SessionStatus::Idle)
            .await
            .unwrap();
        self.backdate(&["last_activity_at"], session, secs).await;
    }

    /// An agent launched `secs` ago, running ever since and silent all the
    /// while: what a turn that never ends looks like from outside the agent.
    pub(crate) async fn launched_ago(&self, session: &AgentSession, secs: i64) {
        self.store
            .set_session_status(&session.id, SessionStatus::Running)
            .await
            .unwrap();
        self.backdate(&["launched_at", "last_activity_at"], session, secs)
            .await;
    }

    /// A session that has been in `starting` for `secs`: the liveness sweep
    /// leaves a start younger than its grace window alone, so a test about
    /// what it concludes has to date the start. `created_at` is the column
    /// that holds it for a row nothing has launched yet, and the only one of
    /// the three the sweep reads that such a row has at all.
    pub(crate) async fn starting_for(&self, session: &AgentSession, secs: i64) {
        self.backdate(&["created_at"], session, secs).await;
    }

    /// When this session's agent process was last started, which is what a
    /// relaunch moves.
    pub(crate) async fn launched_at(&self, session: &AgentSession) -> Option<String> {
        self.store
            .get_session(&session.id)
            .await
            .unwrap()
            .launched_at
    }

    /// Write an attention flag straight into the database, the way a daemon
    /// that did not know better left one behind. It has to go around the
    /// store, which now refuses to raise a prompt on a session that has
    /// ended — which is why there are rows like this to heal at all.
    pub(crate) async fn stale_attention(&self, session: &AgentSession, reason: AttentionReason) {
        sqlx::query(
            "UPDATE agent_sessions SET attention_reason = ?, attention_since = ? WHERE id = ?",
        )
        .bind(reason.as_str())
        .bind(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .bind(&session.id)
        .execute(&self.db)
        .await
        .unwrap();
    }

    /// Move the columns the watchdog's clock is read from back, since the
    /// store only ever stamps them "now" and every threshold is minutes away.
    async fn backdate(&self, columns: &[&str], session: &AgentSession, secs: i64) {
        let when = (chrono::Utc::now() - chrono::Duration::seconds(secs))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let set = columns
            .iter()
            .map(|column| format!("{column} = ?"))
            .collect::<Vec<_>>()
            .join(", ");
        // Safe: only the column names vary, and they are this module's own
        // literals; every value is bound.
        let mut query = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE agent_sessions SET {set} WHERE id = ?"
        )));
        for _ in columns {
            query = query.bind(when.clone());
        }
        query.bind(&session.id).execute(&self.db).await.unwrap();
    }

    // -- HTTP ---------------------------------------------------------------

    /// The whole response, for the handful of tests that assert on a header.
    pub(crate) async fn response(&self, request: Request<Body>) -> axum::response::Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    pub(crate) async fn send(&self, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = self.response(request).await;
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    /// Send a request expected to answer `expected` and decode its JSON body.
    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        request: Request<Body>,
        expected: StatusCode,
    ) -> T {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// The same for `200 OK`, which is what most reads answer.
    pub(crate) async fn get<T: DeserializeOwned>(&self, uri: &str) -> T {
        self.json(get(uri), StatusCode::OK).await
    }

    /// Send a request expected to fail and decode the error envelope.
    pub(crate) async fn error(&self, request: Request<Body>, expected: StatusCode) -> ErrorBody {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// The body of a streaming response, to be read message by message.
    pub(crate) async fn stream(&self, request: Request<Body>) -> Body {
        let response = self.response(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        response.into_body()
    }
}

// -- requests ---------------------------------------------------------------

pub(crate) fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

pub(crate) fn post(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

pub(crate) fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn json_request(method: Method, uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub(crate) fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    json_request(Method::POST, uri, body)
}

pub(crate) fn put_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    json_request(Method::PUT, uri, body)
}

pub(crate) fn patch_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    json_request(Method::PATCH, uri, body)
}

/// A request an agent makes as itself, carrying the session header the daemon
/// identifies it by.
pub(crate) fn as_session(uri: &str, session_id: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(SESSION_HEADER, session_id)
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Whether this session's agent has reported since its own launch: the clock
/// a report stamps (`touch_session`) is later than the one the launch
/// stamped (`mark_session_launched`). The launch's own stamp is written
/// before the agent is up, so it alone says nothing about whether the agent
/// has said anything since.
pub(crate) fn heard_from(session: &AgentSession) -> bool {
    match (&session.last_activity_at, &session.launched_at) {
        (Some(heard), Some(launched)) => heard > launched,
        _ => false,
    }
}

// -- waiting ----------------------------------------------------------------

/// Wait for what the daemon was supposed to do, rather than guessing at how
/// long a pass takes.
///
/// The patience is the caller's: what is waited on here ranges from a store
/// write to a reconciliation tick coming round, and each file says in a
/// constant of its own how long its own kind of waiting is worth.
pub(crate) async fn eventually(
    patience: Duration,
    what: &str,
    mut check: impl AsyncFnMut() -> bool,
) {
    let deadline = Instant::now() + patience;
    loop {
        if check().await {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Wait for the first event matching `pred`, skipping unrelated ones.
pub(crate) async fn next_event(
    rx: &mut Receiver<BusEvent>,
    pred: impl Fn(&BusEvent) -> bool,
) -> BusEvent {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let event = rx.recv().await.expect("event bus closed");
            if pred(&event) {
                return event;
            }
        }
    })
    .await
    .expect("expected a matching event within the timeout")
}

/// What came out of an SSE body next. The three are worth telling apart: a
/// stream that closes is a different thing from one that says nothing, and
/// both are behaviours the session-log tests assert.
#[derive(Debug)]
pub(crate) enum Sse {
    Message(String),
    /// The daemon closed the connection.
    Closed,
    /// Nothing arrived in the time allowed.
    Silent,
}

/// Read from an SSE body until one complete message (`\n\n`-terminated) is in.
pub(crate) async fn next_sse(body: &mut Body, within: Duration) -> Sse {
    let read = tokio::time::timeout(within, async {
        let mut buf = String::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.expect("sse body error");
            if let Some(chunk) = frame.data_ref() {
                buf.push_str(&String::from_utf8_lossy(chunk));
                if buf.contains("\n\n") {
                    return Some(buf);
                }
            }
        }
        None
    })
    .await;
    match read {
        Ok(Some(message)) => Sse::Message(message),
        Ok(None) => Sse::Closed,
        Err(_) => Sse::Silent,
    }
}

pub(crate) async fn next_sse_message(body: &mut Body) -> String {
    match next_sse(body, TIMEOUT).await {
        Sse::Message(message) => message,
        other => panic!("expected an sse message, got {other:?}"),
    }
}

/// The next SSE message, which has to be a `name` one: its decoded payload.
pub(crate) async fn expect_sse(body: &mut Body, name: &str) -> serde_json::Value {
    let (got, payload) = parse_sse(&next_sse_message(body).await);
    assert_eq!(
        got, name,
        "expected an {name} message, got {got}: {payload}"
    );
    payload
}

/// Assert that a stream is over: nothing at all follows, message or frame.
pub(crate) async fn sse_is_closed(body: &mut Body) {
    match next_sse(body, TIMEOUT).await {
        Sse::Closed => {}
        other => panic!("expected the stream to be closed, got {other:?}"),
    }
}

/// `event:` name and decoded `data:` payload of one SSE message.
pub(crate) fn parse_sse(message: &str) -> (String, serde_json::Value) {
    let mut name = None;
    let mut data = None;
    for line in message.trim_end().lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            name = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("data: ") {
            assert!(
                data.is_none(),
                "payload must fit one data line: {message:?}"
            );
            data = Some(rest.to_string());
        }
    }
    let name = name.expect("every message carries an event name");
    let data = data.expect("every message carries a payload");
    (name, serde_json::from_str(&data).expect("payload is JSON"))
}

// -- the shell --------------------------------------------------------------

/// Run a shell command in `dir` — a repository being set up, or read back —
/// failing the test if it does not succeed. The trimmed stdout comes back for
/// the callers that want it.
pub(crate) fn sh(dir: &Path, cmd: &str) -> String {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed in {}: {cmd}\n{}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
