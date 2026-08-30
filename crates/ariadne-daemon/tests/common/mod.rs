//! One daemon in a temporary directory, and the scaffolding every integration
//! test in this crate drives it with.
//!
//! A harness is a real store, a real launcher, the axum router the daemon
//! serves and — where a test asks for one — a real scheduler, all pointed at a
//! `TempDir` that goes when the test does. What varies between tests is the
//! `tmux` behind it, so that is what [`HarnessBuilder::tmux`] takes: a stub
//! script driven by files in the harness directory, a `tmux` that answers
//! nothing, a `tmux` binary that is not there at all, or the real one.
//!
//! Every test binary compiles this module whole, so most of it is dead code in
//! most of them; the crate-wide allow below is what keeps that from being a
//! warning per test binary rather than a signal worth reading.
#![allow(dead_code)]

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
use ariadne_api::stream::DomainEvent;
use ariadne_core::{
    Actor, AgentKind, AttentionReason, GoalStatus, Role, SessionStatus, TaskStatus,
};
use ariadne_daemon::branch::BranchWatchers;
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::log::LogBuffer;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{
    AgentSession, Goal, NewAgentEvent, NewGoal, NewProfile, NewRepository, NewSession,
    NewTask, Profile, Repository, ReviewerSlot, SessionFilter, Store, Task,
};

/// How long a test waits for something the daemon does off the request path —
/// a reconciliation, an event, a delivery — before giving up.
///
/// Generous because some of what is waited on is not the daemon thinking: a
/// nudge no composer will let go of spends several seconds of widening backoff
/// inside `send_submitted` before anybody hears about it, and every test in
/// the crate runs beside the others.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Which `tmux` a harness runs on.
pub enum Tmux {
    /// The stub script, driven by files in the harness directory: the sessions
    /// a test marked alive, the screen it wrote, the keystrokes it reads back.
    Stub,
    /// A `tmux` that answers "no" to everything, the way the real one does for
    /// a session that has ended — and, unlike the real one, the same on a
    /// machine that has no tmux at all.
    Gone,
    /// A `tmux` binary that is not there: every question comes back
    /// unanswered rather than answered "no", which is what a machine briefly
    /// out of process slots looks like from here. [`Harness::tmux_returns`]
    /// puts the stub where it was looked for.
    Missing,
    /// The real `tmux` on `PATH`. Only a test that drives an actual pane wants
    /// one, and every such test is `#[ignore]`d.
    Real,
}

pub struct Harness {
    pub store: Store,
    pub launcher: Arc<Launcher>,
    pub router: Router,
    pub state: AppState,
    pub bus: EventBus,
    pub logs: LogBuffer,
    /// Present when the harness was built with [`HarnessBuilder::scheduler`].
    pub sched: Option<UnboundedSender<SchedEvent>>,
    pub dir: tempfile::TempDir,
    /// One connection of this test's own to the database the store is on, for
    /// the columns a test writes behind the store's back. One, and kept: a
    /// pool per write would be a handful of file descriptors opened and closed
    /// for every clock a test moves, and thirty tests doing that at once run a
    /// machine out of them.
    db: sqlx::SqlitePool,
}

pub struct HarnessBuilder {
    tmux: Tmux,
    home: Option<PathBuf>,
    scheduler: bool,
    spawns: bool,
    logs: Option<LogBuffer>,
    typed_input_window: Option<Duration>,
    compaction_timeout: Option<Duration>,
}

/// A daemon in a temporary directory: a stub `tmux`, no scheduler.
pub fn harness() -> HarnessBuilder {
    HarnessBuilder {
        tmux: Tmux::Stub,
        home: None,
        scheduler: false,
        spawns: true,
        logs: None,
        typed_input_window: None,
        compaction_timeout: None,
    }
}

impl HarnessBuilder {
    pub fn tmux(mut self, tmux: Tmux) -> Self {
        self.tmux = tmux;
        self
    }

    /// Build the daemon around an already prepared home directory — a
    /// `config.toml` in it is read as `ariadned` would read it.
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

    /// A daemon that cannot start anything: `cli_bin` names no executable, so
    /// every fresh session dies at the launch.
    pub fn cannot_spawn(mut self) -> Self {
        self.spawns = false;
        self
    }

    /// Serve `/v1/logs` from a buffer the test already holds.
    pub fn logs(mut self, logs: LogBuffer) -> Self {
        self.logs = Some(logs);
        self
    }

    /// How long a freshly launched pane is watched for a TUI to type a resume
    /// instruction into. Seconds rather than the configured two minutes, for
    /// the tests about what happens when the window runs out.
    pub fn typed_input_window(mut self, window: Duration) -> Self {
        self.typed_input_window = Some(window);
        self
    }

    /// How long a compaction the daemon typed is waited for. Seconds rather
    /// than the configured minutes, for the test about the wait running out.
    pub fn compaction_timeout(mut self, timeout: Duration) -> Self {
        self.compaction_timeout = Some(timeout);
        self
    }

    async fn build(self) -> Harness {
        raise_open_file_limit();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = Store::open(&db_path).await.unwrap();
        // Installed before anything writes, exactly as the daemon does at
        // startup.
        let bus = ariadne_daemon::bus::start(store.clone());
        let mut config = Config::load(Some(self.home.unwrap_or(dir.path().join("home")))).unwrap();
        if !self.spawns {
            config.cli_bin = dir.path().join("no-such-ariadne").display().to_string();
        }
        if let Some(window) = self.typed_input_window {
            config.typed_input_window = window;
        }
        if let Some(timeout) = self.compaction_timeout {
            config.compaction_timeout = timeout;
        }
        let tmux = match self.tmux {
            Tmux::Stub => write_tmux_stub(dir.path()),
            Tmux::Gone => {
                write_script(&dir.path().join("tmux-gone.sh"), "#!/bin/sh\nexit 1\n");
                TmuxManager::new(dir.path().join("tmux-gone.sh").display().to_string())
            }
            Tmux::Missing => TmuxManager::new(stub_path(dir.path()).display().to_string()),
            Tmux::Real => TmuxManager::default(),
        };
        let launcher = Arc::new(Launcher {
            cfg: Arc::new(config),
            store: store.clone(),
            tmux,
            git: GitManager,
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
        };
        // Lazy: most tests never write behind the store's back, and a
        // connection opened for every harness in every binary is a hundred
        // file descriptors a full run has no use for.
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_lazy(&format!("sqlite://{}", db_path.display()))
            .unwrap();
        Harness {
            router: http::router(state.clone()),
            state,
            store,
            launcher,
            bus,
            logs,
            sched,
            dir,
            db,
        }
    }
}

impl IntoFuture for HarnessBuilder {
    type Output = Harness;
    type IntoFuture = Pin<Box<dyn Future<Output = Harness> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.build())
    }
}

/// The open files this binary needs, asked for once before the first daemon
/// starts.
///
/// Every test here runs a daemon of its own — a store with its pools, a tmux
/// stub, a scheduler — and libtest runs as many at once as the machine has
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

fn stub_path(dir: &Path) -> PathBuf {
    dir.join("tmux-stub.sh")
}

fn write_script(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The one stub `tmux`, whose every answer is a file in the harness directory.
///
/// `alive` holds the sessions there are — a line per name, or a bare `*` for
/// "all of them" — so a killed session stops being one of the living exactly
/// as it does in tmux, which is what the daemon's next decision about an agent
/// it just killed turns on. Every call is written to `tmux-commands.log` argv
/// and all, and `send-keys` to a log of its own, which is how "this agent was
/// nudged" and "this is what was pasted into it" are asserted. The panes draw
/// `pane`, report the geometry in `pane-size`, and the marker files make each
/// of those fail on its own — a pane that is there but says nothing is a
/// different thing from one that is gone.
fn write_tmux_stub(dir: &Path) -> TmuxManager {
    let bin = stub_path(dir);
    write_script(&bin, &stub_script(dir));
    std::fs::write(dir.join("alive"), "").unwrap();
    TmuxManager::new(bin.display().to_string())
}

fn stub_script(dir: &Path) -> String {
    let at = |name: &str| dir.join(name).display().to_string();
    // Answering `has-session` and `display-message` costs no process at all:
    // the log follower asks both several times a second, and a stub that forked
    // a `grep` per question put enough latency between a measurement and the
    // capture that goes with it to make a coherent read of the pane fail.
    // Everything the stub writes that something else reads is renamed into
    // place, for the same reason: a reader must never catch a truncated file.
    format!(
        "#!/bin/sh\n\
         alive='{alive}'\n\
         echo \"$@\" >> '{commands}'\n\
         target=''\n\
         prev=''\n\
         for a in \"$@\"; do\n\
        \x20 if [ \"$prev\" = \"-t\" ]; then target=\"$a\"; fi\n\
        \x20 prev=\"$a\"\n\
         done\n\
         living() {{\n\
        \x20 while IFS= read -r name; do\n\
        \x20   if [ \"$name\" = '*' ] || [ \"$name\" = \"$target\" ]; then return 0; fi\n\
        \x20 done < \"$alive\"\n\
        \x20 return 1\n\
         }}\n\
         case \"$1\" in\n\
        \x20 has-session) living || exit 1 ;;\n\
        \x20 display-message)\n\
        \x20   [ -f '{measure_fails}' ] && exit 1\n\
        \x20   living || exit 1\n\
        \x20   if IFS= read -r size < '{size}' 2>/dev/null; then\n\
        \x20     echo \"$size\"\n\
        \x20   else\n\
        \x20     echo '80x24 0,0'\n\
        \x20   fi ;;\n\
        \x20 kill-session)\n\
        \x20   echo \"$target\" >> '{killed}'\n\
        \x20   grep -vx \"$target\" \"$alive\" > \"$alive.tmp\" 2>/dev/null\n\
        \x20   mv \"$alive.tmp\" \"$alive\" 2>/dev/null ;;\n\
        \x20 send-keys)\n\
        \x20   if [ -f '{refusing}' ]; then echo \"$target\" >> '{refused}'; exit 1; fi\n\
        \x20   echo \"$@\" >> '{sent}' ;;\n\
        \x20 capture-pane)\n\
        \x20   [ -f '{capture_fails}' ] && exit 1\n\
        \x20   if [ -f '{resize}' ]; then\n\
        \x20     cat '{resize}' > '{size}.tmp'; mv '{size}.tmp' '{size}'; rm '{resize}'\n\
        \x20   fi\n\
        \x20   cat '{pane}' 2>/dev/null ;;\n\
         esac\n\
         exit 0\n",
        alive = at("alive"),
        commands = at("tmux-commands.log"),
        killed = at("kill-session.log"),
        sent = at("send-keys.log"),
        refusing = at("refusing"),
        refused = at("refused.log"),
        pane = at("pane"),
        size = at("pane-size"),
        resize = at("resize-on-capture"),
        capture_fails = at("capture-fails"),
        measure_fails = at("measure-fails"),
    )
}

// -- the stub tmux, from the test's side ------------------------------------

impl Harness {
    fn at(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.at(name)).unwrap_or_default()
    }

    /// Put a file where the stub reads it, in one step: a reader that catches
    /// a truncated one measures a pane nobody is drawing.
    fn put(&self, name: &str, contents: &str) {
        let tmp = self.at(&format!("{name}.writing"));
        std::fs::write(&tmp, contents).unwrap();
        std::fs::rename(&tmp, self.at(name)).unwrap();
    }

    fn marker(&self, name: &str, set: bool) {
        if set {
            std::fs::write(self.at(name), "").unwrap();
        } else {
            let _ = std::fs::remove_file(self.at(name));
        }
    }

    /// Tell the stub tmux this pane exists.
    pub fn pane_exists(&self, session: &AgentSession) {
        let mut names = self.read("alive");
        names.push_str(&session.tmux_session);
        names.push('\n');
        self.put("alive", &names);
    }

    /// Every session the daemon asks about is alive, whatever its name.
    pub fn every_pane_exists(&self) {
        self.put("alive", "*\n");
    }

    /// Whether the stub still has this pane: a killed session is struck off
    /// the list of the living, as it is in tmux.
    pub fn pane_is_alive(&self, session: &AgentSession) -> bool {
        let alive = self.read("alive");
        alive
            .lines()
            .any(|name| name == session.tmux_session || name == "*")
    }

    /// What every pane draws: a composer holding `text`, for good. A nudge
    /// pasted into it is still there after the Enter, however many are sent.
    pub fn composer_keeps(&self, text: &str) {
        self.pane_draws(&format!("> {text}\n"));
    }

    /// What the stub tmux's `capture-pane` prints, leaving the geometry as it
    /// is — the two are set apart so that a test changing what the pane draws
    /// does not quietly change the grid it draws it at.
    pub fn pane_draws(&self, contents: &str) {
        self.put("pane", contents);
    }

    /// A pane to capture, on a session that exists. The pane is tmux's default
    /// 80×24 with its cursor at the bottom left until a test says otherwise.
    pub fn stub_pane(&self, contents: &str) {
        self.every_pane_exists();
        self.pane_geometry(80, 24, 0, 23);
        self.pane_draws(contents);
    }

    /// What the stub tmux's `display-message` reports about the pane's screen.
    pub fn pane_geometry(&self, cols: u16, rows: u16, cursor_x: u16, cursor_y: u16) {
        self.put(
            "pane-size",
            &format!("{cols}x{rows} {cursor_x},{cursor_y}\n"),
        );
    }

    /// Resize the pane during the next `capture-pane`, once: the capture comes
    /// back drawn at the new grid, and only a measurement taken *after* it can
    /// know that.
    pub fn resize_on_capture(&self, cols: u16, rows: u16, cursor_x: u16, cursor_y: u16) {
        self.put(
            "resize-on-capture",
            &format!("{cols}x{rows} {cursor_x},{cursor_y}\n"),
        );
    }

    /// Whether the stub tmux's `capture-pane` fails — a pane that is there
    /// (`display-message` still answers) but cannot be read.
    pub fn capture_fails(&self, fails: bool) {
        self.marker("capture-fails", fails);
    }

    /// Whether the stub tmux's `display-message` fails — a pane that is there
    /// (`has-session` still succeeds) but cannot be measured.
    pub fn measure_fails(&self, fails: bool) {
        self.marker("measure-fails", fails);
    }

    /// Whether the stub tmux takes keystrokes at all. While it does not it
    /// notes what it turned away, which is what a machine briefly out of
    /// process slots looks like from the daemon's side.
    pub fn keystrokes_refused(&self, refusing: bool) {
        self.marker("refusing", refusing);
    }

    /// Take the stub tmux binary away. A daemon that cannot run a process sees
    /// every question unanswered, rather than answered "no".
    pub fn tmux_vanishes(&self) {
        let (bin, parked) = (stub_path(self.dir.path()), self.at("tmux-stub.parked"));
        if bin.exists() {
            std::fs::rename(bin, parked).unwrap();
        }
    }

    /// Put a working tmux where one was looked for: after
    /// [`Self::tmux_vanishes`], or for the first time under [`Tmux::Missing`].
    pub fn tmux_returns(&self) {
        let (bin, parked) = (stub_path(self.dir.path()), self.at("tmux-stub.parked"));
        if parked.exists() {
            std::fs::rename(parked, bin).unwrap();
            return;
        }
        write_tmux_stub(self.dir.path());
    }

    /// The argv of every `tmux` call the daemon made, one per line.
    pub fn tmux_calls(&self) -> Vec<String> {
        self.read("tmux-commands.log")
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The `tmux` calls whose first word is `verb`.
    pub fn tmux_calls_of(&self, verb: &str) -> Vec<String> {
        self.tmux_calls()
            .into_iter()
            .filter(|call| call.starts_with(&format!("{verb} ")) || call == verb)
            .collect()
    }

    /// How many `send-keys` this session's pane was handed.
    pub fn keystrokes(&self, session: &AgentSession) -> usize {
        self.read("send-keys.log")
            .lines()
            .filter(|line| target_of(line).as_deref() == Some(&session.tmux_session))
            .count()
    }

    /// How many bare Enters this session's pane was sent: a submission, as
    /// opposed to the paste that put something in the composer.
    pub fn enters(&self, session: &AgentSession) -> usize {
        self.read("send-keys.log")
            .lines()
            .filter(|line| target_of(line).as_deref() == Some(&session.tmux_session))
            .filter(|line| line.split_whitespace().count() == 4 && line.ends_with(" Enter"))
            .count()
    }

    /// The panes tmux refused keystrokes for, in order.
    pub fn refused_panes(&self) -> Vec<String> {
        self.read("refused.log").lines().map(String::from).collect()
    }

    /// The panes the daemon asked tmux to kill, in order.
    pub fn killed_panes(&self) -> Vec<String> {
        self.read("kill-session.log")
            .lines()
            .map(String::from)
            .collect()
    }

    /// Everything pasted into a pane, as the agent would have read it: the
    /// stub logs the `send-keys -H` payload one hexadecimal byte per argument,
    /// which is how the bytes travel.
    pub fn pasted(&self, session: &AgentSession) -> String {
        let mut bytes = Vec::new();
        for line in self.read("send-keys.log").lines() {
            let args: Vec<&str> = line.split_whitespace().collect();
            let Some(hex) = args.iter().position(|a| *a == "-H") else {
                continue;
            };
            if target_of(line).as_deref() != Some(&session.tmux_session) {
                continue;
            }
            bytes.extend(
                args[hex + 1..]
                    .iter()
                    .filter_map(|a| u8::from_str_radix(a, 16).ok()),
            );
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// The `-t <session>` of one logged `tmux` call.
fn target_of(call: &str) -> Option<String> {
    let args: Vec<&str> = call.split_whitespace().collect();
    let at = args.iter().position(|a| *a == "-t")?;
    args.get(at + 1).map(|s| s.to_string())
}

// -- seeding ----------------------------------------------------------------

/// The people of one goal with one task, and the repository behind it: every
/// agent that can be spawned for it.
pub struct Cast {
    pub goal: Goal,
    pub task: Task,
    pub repo: Repository,
    pub planner: Profile,
    pub engineer: Profile,
    pub reviewer: Profile,
}

impl Harness {
    pub async fn profile(&self, name: &str, role: Role) -> Profile {
        self.profile_on(name, role, Some(AgentKind::ClaudeCode), None)
            .await
    }

    pub async fn profile_on(
        &self,
        name: &str,
        role: Role,
        agent_kind: Option<AgentKind>,
        model: Option<&str>,
    ) -> Profile {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind,
                model: model.map(str::to_string),
                effort: None,
                system_prompt: Some(format!("You are {name}.")),
            })
            .await
            .unwrap()
    }

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

    /// A registered repository at `path`, which need not exist: only the tests
    /// that spawn an engineer ever have git look at it.
    pub async fn repository(&self, path: &Path) -> Repository {
        self.store
            .create_repository(NewRepository {
                path: path.display().to_string(),
                base_branch: "main".into(),
                description: None,
                merge_strategy: Default::default(),
            })
            .await
            .unwrap()
    }

    /// A goal still in planning, on a repository of its own.
    pub async fn goal(&self, planner: &Profile) -> (Goal, Repository) {
        self.goal_needing(planner, 1).await
    }

    /// The same, for a goal that wants `approvals` of them: a round that one
    /// verdict does not close is where a reviewer sits with its work done.
    async fn goal_needing(&self, planner: &Profile, approvals: i64) -> (Goal, Repository) {
        let repo = self.repository(&self.at("repo")).await;
        let goal = self.goal_on(planner, &repo, approvals).await;
        (goal, repo)
    }

    pub async fn goal_on(&self, planner: &Profile, repo: &Repository, approvals: i64) -> Goal {
        self.store
            .create_goal(NewGoal {
                title: "Ship the UI".into(),
                description: "desc".into(),
                planner_profile_id: planner.id.clone(),
                max_tasks: None,
                required_approvals: approvals,
                repository_ids: vec![repo.id.clone()],
                pin: None,
            })
            .await
            .unwrap()
    }

    /// A task on a goal, with the agents given.
    pub async fn task_on(
        &self,
        goal: &Goal,
        repo: &Repository,
        title: &str,
        engineer: &Profile,
        reviewers: &[&Profile],
    ) -> Task {
        self.store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo.id.clone(),
                title: title.into(),
                description: "do things".into(),
                engineer_profile_id: engineer.id.clone(),
                pin: None,
                reviewers: reviewers.iter().map(|p| ReviewerSlot::of(&p.id)).collect(),
                depends_on: vec![],
            })
            .await
            .unwrap()
    }

    /// A goal still in planning, with a repository behind it and nothing else:
    /// no task, so nothing but the planner is under reconciliation.
    pub async fn planning_goal(&self) -> (Goal, Profile) {
        let planner = self.profile("planner", Role::Planner).await;
        let (goal, _repo) = self.goal(&planner).await;
        (goal, planner)
    }

    /// A goal in planning with one task on it, and the three profiles behind
    /// it: the shape most tests start from.
    pub async fn cast(&self) -> Cast {
        self.cast_needing(1).await
    }

    /// The same on another agent CLI, or on a model: what a task and its
    /// reviewer slots are pinned to is what their profiles were on at the
    /// moment the task was created.
    pub async fn cast_on(&self, agent_kind: AgentKind) -> Cast {
        self.cast_pinned(Some(agent_kind), None, 1).await
    }

    pub async fn cast_needing(&self, approvals: i64) -> Cast {
        self.cast_pinned(Some(AgentKind::ClaudeCode), None, approvals)
            .await
    }

    pub async fn cast_pinned(
        &self,
        agent_kind: Option<AgentKind>,
        model: Option<&str>,
        approvals: i64,
    ) -> Cast {
        let planner = self
            .profile_on("planner", Role::Planner, agent_kind, model)
            .await;
        let engineer = self
            .profile_on("engineer", Role::Engineer, agent_kind, model)
            .await;
        let reviewer = self
            .profile_on("reviewer", Role::Reviewer, agent_kind, model)
            .await;
        let (goal, repo) = self.goal_needing(&planner, approvals).await;
        let task = self
            .task_on(&goal, &repo, "task", &engineer, &[&reviewer])
            .await;
        Cast {
            goal,
            task,
            repo,
            planner,
            engineer,
            reviewer,
        }
    }

    /// Point a profile at another agent CLI and another model, which is what a
    /// `PUT /v1/profiles/{id}` from the UI amounts to.
    pub async fn move_profile(
        &self,
        profile_id: &str,
        agent_kind: Option<AgentKind>,
        model: Option<&str>,
    ) {
        self.store
            .update_profile(
                profile_id,
                ariadne_store::ProfileUpdate {
                    agent_kind: Some(agent_kind),
                    model: Some(model.map(str::to_string)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
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

    /// A live session of `role`, as the launcher would have created it.
    pub async fn session(
        &self,
        goal: &Goal,
        task: Option<&Task>,
        role: Role,
        profile_id: &str,
    ) -> AgentSession {
        let tmux = session_name(
            &goal.id,
            task.map(|t| t.id.as_str()),
            role.as_str(),
            Some(&profile_id[profile_id.len() - 4..]),
        );
        self.session_named(goal, task, role, profile_id, &tmux)
            .await
    }

    /// A planner session on a goal of its own, bound to the tmux session
    /// `tmux_name`: the least a test that only cares about one pane needs.
    pub async fn lone_session(&self, tmux_name: &str) -> AgentSession {
        // Everything named after the pane, so that a test wanting two of them
        // gets two of each rather than a conflict on the second.
        let planner = self
            .profile(&format!("planner-{tmux_name}"), Role::Planner)
            .await;
        let repo = self
            .repository(&self.at(&format!("repo-{tmux_name}")))
            .await;
        let goal = self.goal_on(&planner, &repo, 1).await;
        self.session_named(&goal, None, Role::Planner, &planner.id, tmux_name)
            .await
    }

    /// The same on another agent CLI: the ingestion path an agent's events take
    /// is the one its kind names.
    pub async fn session_on(
        &self,
        goal: &Goal,
        task: Option<&Task>,
        role: Role,
        profile_id: &str,
        agent_kind: AgentKind,
    ) -> AgentSession {
        let tmux = session_name(
            &goal.id,
            task.map(|t| t.id.as_str()),
            role.as_str(),
            Some(&profile_id[profile_id.len() - 4..]),
        );
        self.new_session(goal, task, role, profile_id, &tmux, agent_kind)
            .await
    }

    pub async fn session_named(
        &self,
        goal: &Goal,
        task: Option<&Task>,
        role: Role,
        profile_id: &str,
        tmux_session: &str,
    ) -> AgentSession {
        self.new_session(
            goal,
            task,
            role,
            profile_id,
            tmux_session,
            AgentKind::ClaudeCode,
        )
        .await
    }

    async fn new_session(
        &self,
        goal: &Goal,
        task: Option<&Task>,
        role: Role,
        profile_id: &str,
        tmux_session: &str,
        agent_kind: AgentKind,
    ) -> AgentSession {
        // A tree of its own per session, really there: what a resume comes
        // back in, and what a test can take away to see what happens when it
        // is not.
        let worktree = self.worktree_of(profile_id);
        std::fs::create_dir_all(&worktree).unwrap();
        self.store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: task.map(|t| t.id.clone()),
                role,
                profile_id: profile_id.to_string(),
                agent_kind,
                model: None,
                effort: None,
                tmux_session: tmux_session.to_string(),
                worktree_path: Some(worktree.display().to_string()),
                review_round: task.map(|t| t.review_round),
            })
            .await
            .unwrap()
    }

    fn worktree_of(&self, profile_id: &str) -> PathBuf {
        self.at(&format!("wt-{}", &profile_id[profile_id.len() - 4..]))
    }

    /// A session that has already run once and ended: the agent id a resume
    /// goes back to, and no pane left.
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

    /// A task whose engineer session has already run once: a worktree on disk,
    /// an agent conversation to resume, and a pane that is no longer alive.
    /// What the launcher relaunches when the reviewers bounce a task back.
    pub async fn resumable_engineer(&self) -> (Cast, AgentSession) {
        let cast = self.cast().await;
        let session = self
            .session(
                &cast.goal,
                Some(&cast.task),
                Role::Engineer,
                &cast.engineer.id,
            )
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

    /// Walk a fresh task up to the status a test wants to watch it in.
    pub async fn advance(&self, task: &Task, to: TaskStatus) {
        for (status, actor) in [
            (TaskStatus::Ready, Actor::Daemon),
            (TaskStatus::InProgress, Actor::Daemon),
            (TaskStatus::UnderReview, Actor::Engineer),
        ] {
            self.store
                .transition_task(&task.id, status, actor, None, None)
                .await
                .unwrap();
            if status == to {
                return;
            }
        }
    }

    /// One event reported by an agent, the way its hook or plugin would.
    pub async fn reports(&self, session: &AgentSession, kind: &str) {
        self.store
            .create_event(NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: session.task_id.clone(),
                agent_kind: Some(AgentKind::ClaudeCode),
                kind: kind.into(),
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();
    }

    /// One event reported by an agent, over the endpoint its hook or plugin
    /// posts to — the whole ingestion path, rather than the store write at the
    /// end of it.
    pub async fn ingest(&self, session: &AgentSession, kind: &str, payload: serde_json::Value) {
        let (status, body) = self
            .send(post_json(
                "/internal/agent-events",
                serde_json::json!({
                    "session_id": session.id,
                    "agent_kind": session.agent_kind,
                    "kind": kind,
                    "payload": payload,
                }),
            ))
            .await;
        assert_eq!(
            status,
            StatusCode::ACCEPTED,
            "{kind}: {}",
            String::from_utf8_lossy(&body)
        );
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

    /// The session of `role` that is up on the task, if there is one.
    ///
    /// `running` rather than merely live: a row is created before its agent is
    /// launched, and a test that reads what an agent was started with has to
    /// wait for the launch that wrote it down.
    pub async fn running_session(&self, task_id: &str, role: Role) -> Option<AgentSession> {
        self.sessions_of(task_id)
            .await
            .into_iter()
            .find(|s| s.role() == role && s.status() == SessionStatus::Running)
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
    /// while: what a turn that never ends and an instruction nobody submitted
    /// both look like from outside the pane, which is why what the pane draws
    /// is the only thing that tells them apart.
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

    /// Store a prompt template the store itself would refuse, straight into
    /// the row.
    ///
    /// Placeholders are validated when a prompt is saved, never when one is
    /// rendered, so a database can still hold a briefing naming a token
    /// nothing fills in: edited by hand, restored from a backup, or written
    /// before the check existed.
    pub async fn plant_prompt(
        &self,
        profile_id: &str,
        kind: ariadne_core::PromptKind,
        content: &str,
    ) {
        sqlx::query(
            "INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
             VALUES (?, ?, ?, 't')
             ON CONFLICT (profile_id, kind) DO UPDATE SET content = excluded.content",
        )
        .bind(profile_id)
        .bind(kind.as_str())
        .bind(content)
        .execute(&self.db)
        .await
        .unwrap();
    }

    /// A raw statement against the database the store is on, for the rows a
    /// test has to write behind its back.
    pub fn db(&self) -> &sqlx::SqlitePool {
        &self.db
    }

    /// The spawn plan the launcher last wrote for this session.
    pub fn spawn_plan(&self, session_id: &str) -> Option<ariadne_core::spawn_plan::SpawnPlanFile> {
        ariadne_core::spawn_plan::SpawnPlanFile::from_json(
            &std::fs::read_to_string(self.plan_file(session_id)).unwrap_or_default(),
        )
        .ok()
    }

    /// The argv of the last launch, as the launcher wrote it down for
    /// `ariadne _spawn`.
    pub fn spawn_argv(&self, session_id: &str) -> String {
        self.spawn_plan(session_id)
            .map(|plan| plan.argv.join(" "))
            .unwrap_or_default()
    }

    /// Write the console log tmux `pipe-pane` would have produced.
    pub fn write_console_log(&self, session_id: &str, contents: impl AsRef<[u8]>) {
        let path = self.console_log(session_id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// The spawn plan file itself, for the tests that assert on what tmux was
    /// handed rather than on what is in it.
    pub fn plan_file(&self, session_id: &str) -> PathBuf {
        self.launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("spawn.json")
    }

    pub fn console_log(&self, session_id: &str) -> PathBuf {
        self.launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("console.log")
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
