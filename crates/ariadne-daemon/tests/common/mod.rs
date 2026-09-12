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
//! Every test binary compiles this module whole, so most of it is dead code in
//! most of them; the crate-wide allow below is what keeps that from being a
//! warning per test binary rather than a signal worth reading.
#![allow(dead_code)]

pub mod acp;

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
use ariadne_store::{
    AgentPin, AgentSession, Goal, NewAgentEvent, NewGoal, NewMessage, NewRepository, NewSession,
    NewTask, NewTaskAgent, Repository, SessionFilter, Store, Task, TaskAgent,
};

/// How long a test waits for something the daemon does off the request path —
/// a reconciliation, an event, a delivery — before giving up.
///
/// Generous because some of what is waited on is not the daemon thinking: a
/// stub agent is a python process the daemon starts and talks to, and every
/// test in the crate runs beside the others.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The registry id the harness registers its stub agent under.
pub const STUB: &str = "stub";

pub struct Harness {
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
    /// One connection of this test's own to the database the store is on, for
    /// the columns a test writes behind the store's back. One, and kept: a
    /// pool per write would be a handful of file descriptors opened and closed
    /// for every clock a test moves, and thirty tests doing that at once run a
    /// machine out of them.
    db: sqlx::SqlitePool,
}

pub struct HarnessBuilder {
    home: Option<PathBuf>,
    scheduler: bool,
    spawns: bool,
    dies: bool,
    logs: Option<LogBuffer>,
    discover_agents: bool,
}

/// The pin the fixtures staff an agent on: a model of the registry agent the
/// harness registers. A model is required everywhere, so every seeded row
/// names one, and a test that cares which model it is names its own.
pub fn test_pin() -> AgentPin {
    AgentPin {
        model: format!("{STUB}:test-model"),
        effort: None,
    }
}

/// A daemon in a temporary directory, its registry holding the stub agent,
/// and no scheduler.
pub fn harness() -> HarnessBuilder {
    HarnessBuilder {
        home: None,
        scheduler: false,
        spawns: true,
        dies: false,
        logs: None,
        discover_agents: false,
    }
}

impl HarnessBuilder {
    /// Build the daemon around an already prepared home directory — a
    /// `config.toml` in it is read as `ariadned` would read it, its registry
    /// included.
    pub fn home(mut self, home: PathBuf) -> Self {
        self.home = Some(home);
        self
    }

    /// Run a real scheduler behind the router, as the daemon does. No sleep
    /// inhibition: nothing in a test runs long enough to matter.
    pub fn scheduler(mut self) -> Self {
        self.scheduler = true;
        self
    }

    /// A daemon that cannot start anything: the registry's stub agent names
    /// no executable, so every fresh session dies at the launch.
    pub fn cannot_spawn(mut self) -> Self {
        self.spawns = false;
        self
    }

    /// A daemon whose agent starts and exits at once: every launch works,
    /// and not one agent is ever heard from.
    pub fn dying_agent(mut self) -> Self {
        self.dies = true;
        self
    }

    /// Serve `/v1/logs` from a buffer the test already holds.
    pub fn logs(mut self, logs: LogBuffer) -> Self {
        self.logs = Some(logs);
        self
    }

    /// Run ACP registry discovery while the harness starts, on a home of the
    /// test's own ([`Self::home`]). The harness's own home is always
    /// discovered — the daemon discovers its registry at every start, and a
    /// resume is gated on what discovery measured — with the stub probed
    /// until discovery accepts it and the probes' traffic dropped from its
    /// log.
    pub fn discover_agents(mut self) -> Self {
        self.discover_agents = true;
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
                    (true, true) => {
                        let exits = dir.path().join("exits-at-once");
                        write_script(&exits, "#!/bin/sh\nexit 0\n");
                        exits.display().to_string()
                    }
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
        let config = Config::load(Some(home)).unwrap();
        let agent_registry = ariadne_daemon::acp_discovery::AgentRegistry::test_registry(
            &config.acp_agents,
            config.root.clone(),
            store.clone(),
        );
        // Installed before anything writes, exactly as the daemon does at
        // startup.
        let bus = ariadne_daemon::bus::start(store.clone());
        let settle = own_home && self.spawns && !self.dies;
        let discover = self.discover_agents || settle;
        if discover {
            agent_registry.discover().await;
        }
        let launcher = Arc::new(Launcher {
            cfg: Arc::new(config),
            store: store.clone(),
            git: GitManager,
            acp: ariadne_daemon::acp::AcpRuntime::new(store.clone()),
            registry: agent_registry.clone(),
            branches: BranchWatchers::new(bus.clone()),
        });
        let sched = self
            .scheduler
            .then(|| scheduler::start(store.clone(), launcher.clone(), false));
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
            outside_sessions: ariadne_daemon::acp_sessions::OutsideSessions::default(),
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

fn write_script(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

// -- the stub agent, from the test's side -------------------------------------

impl Harness {
    pub fn at(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Start the harness's stub agent under a session the test seeded, the
    /// way a launch would have: a live agent process the runtime owns, with
    /// no briefing to answer. Returns once the agent has reported its session
    /// start, so what the test writes to the row afterwards is not written
    /// over by the handshake.
    pub async fn agent_runs(&self, session: &AgentSession) {
        let repository_id = match &session.task_id {
            Some(task) => self.store.get_task(task).await.unwrap().repo_id,
            None => {
                self.store
                    .list_goal_repositories(&session.goal_id)
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
        self.launcher
            .acp
            .launch(AcpLaunch {
                session_id: session.id.clone(),
                launch_id: ariadne_core::id::new_id(),
                program: self.agent.bin.clone(),
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
        eventually(TIMEOUT, "the stub agent to start", || async {
            self.store
                .list_session_events(&session.id)
                .await
                .unwrap()
                .iter()
                .any(|event| event.kind == "session_start")
        })
        .await;
    }

    /// Whether the runtime still owns an agent process for this session.
    pub fn agent_is_running(&self, session: &AgentSession) -> bool {
        self.launcher.acp.is_running(&session.id)
    }

    /// Every prompt the harness's stub agent was handed for this session, in
    /// order, as the agent read it.
    pub fn prompts_to(&self, session: &AgentSession) -> Vec<String> {
        self.agent.prompts_for(&session.id)
    }

    /// Every prompt this session's agent was handed since its launch, as one
    /// text: what a delivery to it is read back from.
    pub fn prompted(&self, session: &AgentSession) -> String {
        self.prompts_to(session).join("\n\n")
    }

    /// Everything this session's agent was told: the system prompt and the
    /// briefing of its last launch, and every prompt the harness's stub was
    /// sent for it since — whichever way a briefing travelled, a relaunch or
    /// a prompt to an agent already up.
    pub fn told(&self, session_id: &str) -> String {
        let mut told = Vec::new();
        if let Some(launch) = self.launch_file(session_id) {
            told.push(launch.system_prompt);
            told.extend(launch.initial_prompt);
        }
        told.extend(self.agent.prompts_for(session_id));
        told.join("\n\n")
    }

    /// The launch file the adapter last wrote for this session: what its
    /// agent was told, pinned to and connected to.
    pub fn launch_file(&self, session_id: &str) -> Option<LaunchConfig> {
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
pub struct Cast {
    pub goal: Goal,
    pub task: Task,
    pub repo: Repository,
    pub author: TaskAgent,
    pub reviewer: TaskAgent,
}

impl Harness {
    /// A toy git repo under the harness directory: `main` at one commit, and a
    /// `next` branch one commit ahead of it, checked out on `main`.
    pub fn git_repo(&self, name: &str) -> PathBuf {
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
    pub async fn repository(&self, path: &Path) -> Repository {
        std::fs::create_dir_all(path).unwrap();
        self.store
            .create_repository(NewRepository {
                path: path.display().to_string(),
                base_branch: "main".into(),
                description: None,
            })
            .await
            .unwrap()
    }

    /// One reviewer's verdict on the round a task stands in, as the reviewer
    /// itself would send it: a message to the author, of the kind that closes
    /// a round.
    pub async fn verdict(
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
    pub async fn verdict_from(
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
    pub async fn goal(&self) -> (Goal, Repository) {
        let repo = self.repository(&self.at("repo")).await;
        let goal = self.goal_on(&repo, test_pin()).await;
        (goal, repo)
    }

    pub async fn goal_on(&self, repo: &Repository, pin: AgentPin) -> Goal {
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
    pub async fn task_on(
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
                permission_mode: None,
            })
            .await
            .unwrap()
    }

    /// A goal still in planning, with a repository behind it and nothing else:
    /// no task, so nothing but the orchestrator is under reconciliation.
    pub async fn planning_goal(&self) -> Goal {
        let (goal, _repo) = self.goal().await;
        goal
    }

    /// A goal in planning with one task on it, and the agents staffed on that
    /// task: the shape most tests start from.
    pub async fn cast(&self) -> Cast {
        self.cast_reviewed_by(1).await
    }

    /// The same, with `reviewers` reviewers on the task. A task is approved
    /// when every one of them has approved, so two of them is where a round
    /// one verdict does not close — a reviewer sitting with its work done.
    pub async fn cast_reviewed_by(&self, reviewers: usize) -> Cast {
        self.cast_pinned(&test_pin().model, reviewers).await
    }

    /// The same on another model: what a goal and a task's agents run on is
    /// what they were pinned to when they were created.
    pub async fn cast_pinned(&self, model: &str, reviewers: usize) -> Cast {
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
    pub async fn move_agent(&self, agent_id: &str, model: &str) {
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
    pub async fn active_cast(&self) -> Cast {
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

    pub async fn activate(&self, goal: &Goal) -> Goal {
        self.store
            .set_goal_status(&goal.id, GoalStatus::Active)
            .await
            .unwrap()
    }

    /// A session of `seat`, as the launcher would have created it — a row,
    /// with no agent process under it until [`Self::agent_runs`] starts one.
    pub async fn session(
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
    pub async fn lone_session(&self, name: &str) -> AgentSession {
        let repo = self.repository(&self.at(&format!("repo-{name}"))).await;
        let goal = self.goal_on(&repo, test_pin()).await;
        // An orchestrator is staffed on no task, so its session carries no
        // agent.
        self.orchestrator_session(&goal).await
    }

    /// An orchestrator session on `goal`.
    pub async fn orchestrator_session(&self, goal: &Goal) -> AgentSession {
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
                goal_id: goal.id.clone(),
                task_id: task.map(|t| t.id.clone()),
                seat,
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

    /// A session that has already run once and ended: the agent id a resume
    /// goes back to, and no agent left.
    pub async fn ended(&self, session: &AgentSession) -> AgentSession {
        self.store
            .set_session_internal_id(&session.id, "uuid-1234")
            .await
            .unwrap();
        self.set_status(session, SessionStatus::Exited).await;
        self.store.get_session(&session.id).await.unwrap()
    }

    /// Take a session's row out from under the daemon, the way deleting the
    /// goal it belonged to would. Straight SQL: nothing an agent can call does
    /// this, which is the point — it is the state the daemon has to cope with,
    /// not one it is asked to produce.
    pub async fn forget_session(&self, session: &AgentSession) {
        sqlx::query("DELETE FROM agent_sessions WHERE id = ?")
            .bind(&session.id)
            .execute(&self.db)
            .await
            .unwrap();
    }

    /// A task whose author session has already run once: a worktree on disk,
    /// an agent conversation to resume, and no agent left running.
    /// What the launcher relaunches when the reviewers bounce a task back.
    pub async fn resumable_author(&self) -> (Cast, AgentSession) {
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
    pub async fn make_resumable(&self, task: &Task, session: &AgentSession) {
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
    /// have taken it part of the way.
    pub async fn advance(&self, task: &Task, to: TaskStatus) {
        let steps = [
            (TaskStatus::Ready, Actor::Daemon),
            (TaskStatus::InProgress, Actor::Daemon),
            (TaskStatus::UnderReview, Actor::Author),
        ];
        let now = self.status(&task.id).await;
        let reached = steps.iter().position(|(status, _)| *status == now);
        for (at, (status, actor)) in steps.into_iter().enumerate() {
            if reached.is_none_or(|reached| at > reached) {
                self.store
                    .transition_task(&task.id, status, actor, None, None)
                    .await
                    .unwrap();
            }
            if status == to {
                return;
            }
        }
    }

    /// One event recorded for an agent, straight into the store.
    pub async fn reports(&self, session: &AgentSession, kind: &str) {
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
    pub async fn ingest(&self, session: &AgentSession, kind: &str, payload: serde_json::Value) {
        self.ingest_as(session, None, kind, payload).await;
    }

    /// The same event, reported by a named launch of that session: what
    /// every agent the daemon starts reports under, and the only thing that
    /// tells the agent running from the one it replaced.
    pub async fn ingest_from(
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
    pub async fn launch_id(&self, session: &AgentSession) -> Option<String> {
        self.store.get_session(&session.id).await.unwrap().launch_id
    }

    /// Raise a flag on a session, the way the ingestion or a sweep would.
    pub async fn raise(&self, session: &AgentSession, reason: AttentionReason) {
        self.store
            .set_session_attention(&session.id, reason)
            .await
            .unwrap();
    }

    /// Move a session's lifecycle status, the way its agent reporting would.
    pub async fn set_status(&self, session: &AgentSession, status: SessionStatus) {
        self.store
            .set_session_status(&session.id, status)
            .await
            .unwrap();
    }

    pub async fn session_status(&self, session: &AgentSession) -> SessionStatus {
        self.store.get_session(&session.id).await.unwrap().status()
    }

    pub async fn attention(&self, session: &AgentSession) -> Option<AttentionReason> {
        self.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason()
    }

    /// Poke the scheduler about a task, the way an HTTP handler does after a
    /// write. Only for a harness built with [`HarnessBuilder::scheduler`].
    pub fn notify(&self, task_id: &str) {
        self.wake(SchedEvent::TaskChanged(task_id.to_string()));
    }

    /// The same about a goal: what a status change sends.
    pub fn notify_goal(&self, goal_id: &str) {
        self.wake(SchedEvent::GoalChanged(goal_id.to_string()));
    }

    fn wake(&self, event: SchedEvent) {
        self.sched
            .as_ref()
            .expect("this harness has no scheduler")
            .send(event)
            .unwrap();
    }

    pub async fn status(&self, task_id: &str) -> TaskStatus {
        self.store.get_task(task_id).await.unwrap().status()
    }

    /// Every session a goal has ever had, live or not — an orchestrator's
    /// included, which is the one no task lists.
    pub async fn sessions_of_goal(&self, goal_id: &str) -> Vec<AgentSession> {
        self.store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                ..Default::default()
            })
            .await
            .unwrap()
    }

    /// Every session a task has ever had, live or not.
    pub async fn sessions_of(&self, task_id: &str) -> Vec<AgentSession> {
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
    pub async fn running_session(&self, task_id: &str, seat: Seat) -> Option<AgentSession> {
        self.sessions_of(task_id).await.into_iter().find(|s| {
            s.seat() == seat
                && matches!(s.status(), SessionStatus::Running | SessionStatus::Idle)
                && s.launched_at.is_some()
        })
    }

    // -- the clock ----------------------------------------------------------

    /// An agent that has been sitting there doing nothing for `secs`.
    pub async fn idle_for(&self, session: &AgentSession, secs: i64) {
        self.store
            .set_session_status(&session.id, SessionStatus::Idle)
            .await
            .unwrap();
        self.backdate(&["last_activity_at"], session, secs).await;
    }

    /// An agent launched `secs` ago, running ever since and silent all the
    /// while: what a turn that never ends looks like from outside the agent.
    pub async fn launched_ago(&self, session: &AgentSession, secs: i64) {
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
    pub async fn starting_for(&self, session: &AgentSession, secs: i64) {
        self.backdate(&["created_at"], session, secs).await;
    }

    /// When this session's agent process was last started, which is what a
    /// relaunch moves.
    pub async fn launched_at(&self, session: &AgentSession) -> Option<String> {
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
    pub async fn stale_attention(&self, session: &AgentSession, reason: AttentionReason) {
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

    /// A raw statement against the database the store is on, for the rows a
    /// test has to write behind its back.
    pub fn db(&self) -> &sqlx::SqlitePool {
        &self.db
    }

    // -- HTTP ---------------------------------------------------------------

    /// The whole response, for the handful of tests that assert on a header.
    pub async fn response(&self, request: Request<Body>) -> axum::response::Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    pub async fn send(&self, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = self.response(request).await;
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    /// Send a request expected to answer `expected` and decode its JSON body.
    pub async fn json<T: DeserializeOwned>(
        &self,
        request: Request<Body>,
        expected: StatusCode,
    ) -> T {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// The same for `200 OK`, which is what most reads answer.
    pub async fn get<T: DeserializeOwned>(&self, uri: &str) -> T {
        self.json(get(uri), StatusCode::OK).await
    }

    /// Send a request expected to fail and decode the error envelope.
    pub async fn error(&self, request: Request<Body>, expected: StatusCode) -> ErrorBody {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// The body of a streaming response, to be read message by message.
    pub async fn stream(&self, request: Request<Body>) -> Body {
        let response = self.response(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        response.into_body()
    }
}

// -- requests ---------------------------------------------------------------

pub fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

pub fn post(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

pub fn delete(uri: &str) -> Request<Body> {
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

pub fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    json_request(Method::POST, uri, body)
}

pub fn put_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    json_request(Method::PUT, uri, body)
}

pub fn patch_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    json_request(Method::PATCH, uri, body)
}

/// A request an agent makes as itself, carrying the session header the daemon
/// identifies it by.
pub fn as_session(uri: &str, session_id: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(SESSION_HEADER, session_id)
        .body(Body::from(body.to_string()))
        .unwrap()
}

// -- waiting ----------------------------------------------------------------

/// Wait for what the daemon was supposed to do, rather than guessing at how
/// long a pass takes.
///
/// The patience is the caller's: what is waited on here ranges from a store
/// write to a reconciliation tick coming round, and each file says in a
/// constant of its own how long its own kind of waiting is worth.
pub async fn eventually(patience: Duration, what: &str, mut check: impl AsyncFnMut() -> bool) {
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
pub async fn next_event(rx: &mut Receiver<BusEvent>, pred: impl Fn(&BusEvent) -> bool) -> BusEvent {
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
pub enum Sse {
    Message(String),
    /// The daemon closed the connection.
    Closed,
    /// Nothing arrived in the time allowed.
    Silent,
}

/// Read from an SSE body until one complete message (`\n\n`-terminated) is in.
pub async fn next_sse(body: &mut Body, within: Duration) -> Sse {
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

pub async fn next_sse_message(body: &mut Body) -> String {
    match next_sse(body, TIMEOUT).await {
        Sse::Message(message) => message,
        other => panic!("expected an sse message, got {other:?}"),
    }
}

/// The next SSE message, or `None` if none arrives within `within` — for
/// asserting that a stream is deliberately saying nothing.
pub async fn sse_message_within(body: &mut Body, within: Duration) -> Option<String> {
    match next_sse(body, within).await {
        Sse::Message(message) => Some(message),
        Sse::Silent => None,
        Sse::Closed => panic!("the stream closed instead of staying open"),
    }
}

/// The next SSE message, which has to be a `name` one: its decoded payload.
pub async fn expect_sse(body: &mut Body, name: &str) -> serde_json::Value {
    let (got, payload) = parse_sse(&next_sse_message(body).await);
    assert_eq!(
        got, name,
        "expected an {name} message, got {got}: {payload}"
    );
    payload
}

/// Assert that a stream is over: nothing at all follows, message or frame.
pub async fn sse_is_closed(body: &mut Body) {
    match next_sse(body, TIMEOUT).await {
        Sse::Closed => {}
        other => panic!("expected the stream to be closed, got {other:?}"),
    }
}

/// `event:` name and decoded `data:` payload of one SSE message.
pub fn parse_sse(message: &str) -> (String, serde_json::Value) {
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
pub fn sh(dir: &Path, cmd: &str) -> String {
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
