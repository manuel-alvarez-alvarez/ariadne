//! Launcher: turns "spawn an agent for X" into a worktree, a session row and
//! a tmux process.
//!
//! A launch is refused rather than duplicated. Every spawn asks first whether
//! the seat already has a live session, and counts "tmux could not be asked"
//! as a yes: a wrong no puts two agents on one piece of work, where a wrong
//! yes costs a scheduler tick that asks again.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};

use ariadne_core::spawn_plan::SpawnPlanFile;
use ariadne_core::{
    AgentKind, AttentionReason, PromptKind, Seat, SessionStatus, TaskStatus, probe,
};
use ariadne_store::{
    AgentSession, NewSession, Repository, SessionFilter, Store, Task, TaskAgent, TaskFilter,
    author_branch,
};

use crate::acp::{AcpLaunch, AcpRuntime};
use crate::agents::{SpawnCtx, SpawnPlan, adapter_for, prompts, write_skills};
use crate::branch::BranchWatchers;
use crate::config::Config;
use crate::gitwt::GitManager;
use crate::tmux::{TmuxManager, TmuxSpawn, session_name, tail};

/// How often a freshly launched pane is read while something is waited for in
/// it: the directory-trust dialog to appear, or the TUI itself to draw its
/// first frame. One `tmux capture-pane` per turn, so the interval is the
/// latency it adds to whatever comes next — a fifth of a second of it, where
/// half of one was two and a half times the wait for nothing.
const PANE_POLL: Duration = Duration::from_millis(200);
/// How long a freshly launched pane is watched for a dialog before the watch
/// gives up. Generous (two minutes): a slow CLI start draws the dialog well
/// after the spawn, and a watcher that has already stopped leaves the agent
/// sitting on a question nobody is told about.
const DIALOG_WATCH: Duration = Duration::from_secs(120);
/// The beat between a TUI drawing its first frame and the resume instruction
/// being typed into it, for the one CLI that takes its instruction that way
/// (opencode; see [`SpawnPlan::post_launch_input`]).
///
/// It is not the frame that is waited for but the input handling behind it.
/// Measured on opencode 1.18.20: a paste 100 ms after its first frame landed
/// in the composer and submitted, every time, so 300 is three times what it
/// needed — and the cost of being wrong is a message nobody sees, which is
/// why this one keeps a margin rather than the tightest number that worked.
const INPUT_BEAT: Duration = Duration::from_millis(300);

pub struct Launcher {
    pub cfg: Arc<Config>,
    pub store: Store,
    pub tmux: TmuxManager,
    pub git: GitManager,
    /// The daemon-owned ACP agents: a session of kind `acp` runs as a child
    /// process driven here, never in a tmux pane.
    pub acp: AcpRuntime,
    /// The task branches whose head the daemon is following, so that a commit
    /// an author makes reaches the clients watching its diff.
    pub branches: BranchWatchers,
}

impl Launcher {
    fn run_dir(&self, session_id: &str) -> PathBuf {
        self.cfg.run_dir.join(session_id)
    }

    /// Where a session's spawn plan is written: everything the launch was
    /// made of, kept afterwards as the record of how the agent was started.
    fn spawn_plan_file(&self, session_id: &str) -> PathBuf {
        self.run_dir(session_id).join("spawn.json")
    }

    /// Where the last measured pane grid is kept, beside the console log it
    /// belongs to.
    fn pane_size_file(&self, session_id: &str) -> PathBuf {
        self.run_dir(session_id).join("pane-size")
    }

    /// Remember the grid a pane is drawing at.
    ///
    /// A session's console log is raw terminal bytes, and they only mean
    /// anything at the size they were written at — but once the session ends,
    /// tmux no longer has a pane to ask. So every measurement taken while it
    /// lives is written down, and the last one is what a viewer of the
    /// finished log gets. Best effort: a size we fail to store only costs the
    /// viewer a default.
    pub async fn record_pane_size(&self, session_id: &str, cols: u16, rows: u16) {
        let path = self.pane_size_file(session_id);
        let contents = format!("{cols}x{rows}\n");
        // The run dir exists from the spawn that wrote the agent's config into
        // it; a session with no run dir has no console log to size either.
        if let Err(e) = tokio::fs::write(&path, contents).await {
            tracing::debug!(session = %session_id, error = %e, "storing the pane size failed");
        }
    }

    /// The last grid recorded for a session, if one ever was.
    pub async fn last_pane_size(&self, session_id: &str) -> Option<(u16, u16)> {
        let raw = tokio::fs::read_to_string(self.pane_size_file(session_id))
            .await
            .ok()?;
        crate::tmux::parse_size(raw.trim())
    }

    /// Refuse to double-spawn: one live session per (task, seat) —
    /// per (task, seat, agent) for reviewers, and for the authors of a task
    /// staffed with several.
    ///
    /// A pane tmux will not answer for counts as live. This is the last guard
    /// before a second agent starts working on somebody else's task, and the
    /// two ways of being wrong are not comparable: a spawn refused because
    /// tmux was briefly unreachable is retried on the next tick, while one
    /// allowed on the same grounds has to be noticed by a human.
    async fn assert_no_live_session(
        &self,
        goal_id: &str,
        task_id: Option<&str>,
        seat: Seat,
        agent_id: Option<&str>,
    ) -> Result<()> {
        let live = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                task_id: task_id.map(str::to_string),
                live_only: true,
                ..Default::default()
            })
            .await?;
        for s in live {
            if s.seat() == seat
                && agent_id.is_none_or(|a| Some(a) == s.task_agent_id.as_deref())
                && self.session_process_alive(&s).await
            {
                return Err(anyhow!(
                    "a live {} session already exists: {} (tmux {})",
                    seat.as_str(),
                    s.id,
                    s.tmux_session
                ));
            }
        }
        Ok(())
    }

    /// Whether the process behind a session is alive, counting "could not be
    /// asked" as yes — what every spawn decision reads. An `acp` session has
    /// no pane: the runtime that owns its child answers instead
    /// ([`AcpRuntime::is_running`]), and it always answers.
    pub async fn session_process_alive(&self, session: &AgentSession) -> bool {
        match session.agent_kind() {
            AgentKind::Acp => self.acp.is_running(&session.id),
            _ => {
                self.tmux
                    .has_session_or_unknown(&session.tmux_session)
                    .await
            }
        }
    }

    /// Take the tmux name the session about to be created will run under.
    ///
    /// A name is derived from the goal, the task and the seat, so a seat has
    /// exactly one of them: a pane still holding it when
    /// [`Self::assert_no_live_session`] has just found nothing live belongs to
    /// a session the database has already retired. That is the daemon and the
    /// machine disagreeing, and tmux is the half that cannot be talked round —
    /// `new-session` refuses a duplicate name, and it refuses it *after* the
    /// session row was written, so every attempt leaves another dead row
    /// behind until the spawn budget runs out and the user is told an agent
    /// that could have started will not.
    ///
    /// The leftover pane is therefore killed rather than worked around: what
    /// goes is an agent nothing in the database is waiting on, and what is
    /// kept is the one name its seat has. A tmux that will not answer is left
    /// alone, here as everywhere — the spawn behind this fails the way it
    /// always did, and the next pass asks again.
    async fn claim_pane(&self, name: &str) -> Result<()> {
        if !self.tmux.has_session(name).await {
            return Ok(());
        }
        tracing::warn!(
            tmux = %name,
            "a pane still holds the name this session needs and no live session claims it; killing it"
        );
        self.tmux
            .kill_session(name)
            .await
            .with_context(|| format!("claiming the tmux session {name}"))
    }

    /// What a dialog only a person can answer looks like in a pane,
    /// lowercased. Read through [`dialog_on_the_pane`], which is what both the
    /// watcher below and the typed-input deliverer ask: one must not paste
    /// into such a dialog, and the other must not answer it.
    const DIALOG_PATTERNS: [&'static str; 4] = [
        "do you trust",
        "trust this folder",
        "trust the contents",
        "press enter to continue",
    ];

    /// Coding-agent TUIs open a one-time directory-trust dialog over a folder
    /// they do not know — and every worktree is a fresh folder. Watch the
    /// pane for one, and raise the session for the user.
    ///
    /// Watched rather than answered. The daemon used to press Enter on
    /// whatever answer the dialog highlighted, and which answer that is
    /// belongs to the CLI: Claude Code 2.1 highlights "No, exit", so every
    /// Enter closed the agent that had just been started and the next tick
    /// started another — a goal spawning an orchestrator every five seconds
    /// and nobody being told, because the alarm each death raised was cleared
    /// by the launch that replaced it.
    ///
    /// A question on an agent's terminal is the user's to answer, so the
    /// session says it is waiting on one and the dialog is left standing.
    /// Nothing takes that pane away in the meantime: an agent waiting on a
    /// person is never nudged and never relaunched (009), and typing into the
    /// pane is what takes the flag down (008) — which is exactly answering the
    /// question.
    ///
    /// The window is [`DIALOG_WATCH`], and a single failed capture is no
    /// reason to stop watching — only the session going away, or the dialog
    /// being found, is.
    fn watch_for_a_dialog(&self, session_id: String, tmux_session: String) {
        let tmux = self.tmux.clone();
        let store = self.store.clone();
        tokio::spawn(async move {
            let deadline = std::time::Instant::now() + DIALOG_WATCH;
            while std::time::Instant::now() < deadline {
                tokio::time::sleep(PANE_POLL).await;
                if !tmux.has_session(&tmux_session).await {
                    return;
                }
                let Ok(pane) = tmux.capture_pane(&tmux_session, 50).await else {
                    continue;
                };
                if !dialog_on_the_pane(&pane) {
                    continue;
                }
                tracing::info!(session = %tmux_session, "a dialog nobody but the user can answer is on this pane; flagging it");
                raise_waiting_input(&store, &session_id).await;
                return;
            }
        });
    }

    /// Type a resume instruction into the pane once its TUI is up — the
    /// delivery for [`SpawnPlan::post_launch_input`], whose docs say which
    /// CLI needs it and why.
    ///
    /// Readiness is judged from the pane itself: something has to be drawn,
    /// and it must not be the directory-trust dialog, whose accept would
    /// swallow the paste. One short beat later the instruction goes in
    /// through [`TmuxManager::send_submitted`], which is the only way to know
    /// the TUI took it rather than left it sitting in its composer. When it
    /// cannot be confirmed the session is raised for the user: a resumed
    /// agent that never heard its instruction sits there doing nothing, and
    /// this is the only place that knows it. The watch window is
    /// `typed_input_window`, the trust watcher's two minutes, and a pane that
    /// never draws anything in it ends the same way: giving up is a delivery
    /// that did not happen, so it is raised rather than logged. Delivery is
    /// attempted once; a session that goes away has nobody left waiting on
    /// it.
    fn deliver_typed_input(&self, session_id: String, tmux_session: String, input: String) {
        let tmux = self.tmux.clone();
        let store = self.store.clone();
        let deadline = std::time::Instant::now() + self.cfg.typed_input_window;
        tokio::spawn(async move {
            while std::time::Instant::now() < deadline {
                tokio::time::sleep(PANE_POLL).await;
                if !tmux.has_session(&tmux_session).await {
                    return;
                }
                let Ok(pane) = tmux.capture_pane(&tmux_session, 50).await else {
                    continue;
                };
                if pane.trim().is_empty() || dialog_on_the_pane(&pane) {
                    continue;
                }
                // One more beat: a TUI that just painted its first frame may
                // still be wiring up its input handling.
                tokio::time::sleep(INPUT_BEAT).await;
                match tmux.send_submitted(&tmux_session, &input).await {
                    Ok(true) => {
                        tracing::info!(session = %tmux_session, "typed the resume instruction into the TUI")
                    }
                    Ok(false) => {
                        tracing::warn!(session = %tmux_session, "the resume instruction stayed in the TUI's composer; flagging for user attention");
                        raise_stalled(&store, &session_id).await;
                    }
                    Err(e) => {
                        tracing::warn!(session = %tmux_session, error = %e, "typing the resume instruction failed");
                        raise_stalled(&store, &session_id).await;
                    }
                }
                return;
            }
            // The same place every other way of not delivering this ends: an
            // agent that never heard its instruction sits there doing
            // nothing, and a line in the log tells nobody.
            tracing::warn!(session = %tmux_session, "gave up waiting for a TUI to type the resume instruction into; flagging for user attention");
            raise_stalled(&store, &session_id).await;
        });
    }

    /// Assemble the adapter context for launching `session` in `cwd`.
    ///
    /// The agent's flags are read here rather than baked into the adapters, on
    /// every spawn and every resume alike: an edit to the agent config is meant
    /// to reach the next launch, whichever path that launch comes down.
    ///
    /// The model and the effort it runs at are the opposite: they are the ones
    /// the session was created with, off the pin its seat carries, and no
    /// launch of that session ever moves either. Editing a profile is meant to
    /// steer the work defined after it, not to switch the model out from under
    /// a conversation already running.
    async fn spawn_ctx(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        initial_prompt: String,
    ) -> Result<SpawnCtx> {
        let agent = self.store.get_agent_config(session.agent_kind()).await?;
        // What this agent knows, read here rather than passed in: one place
        // decides what a session is briefed with, and the index in the prompt
        // and the documents on disk are then the same list by construction.
        // The orchestrator is staffed by nobody, so its one skill — the
        // playbook — is named in code, and an edit to it reaches the next
        // launch the way any task agent's skill does.
        let skills = match session.seat() {
            Seat::Orchestrator => vec![
                self.store
                    .get_skill(ariadne_store::defaults::ORCHESTRATION_SKILL)
                    .await?,
            ],
            Seat::Author | Seat::Reviewer => match &session.task_agent_id {
                Some(id) => self.store.agent_skills(id).await?,
                None => Vec::new(),
            },
        };
        let run_dir = self.run_dir(&session.id);
        // Written before the adapter plans anything, and by the same call that
        // renders the index, so what the prompt names is what is on disk.
        let skills_dir = match skills.is_empty() {
            true => None,
            false => {
                let documents: Vec<(String, String)> = skills
                    .iter()
                    .map(|skill| (skill.name.clone(), skill.document_text().to_string()))
                    .collect();
                Some(write_skills(&run_dir, &documents)?)
            }
        };
        let system_prompt = prompts::system_prompt(session.seat(), &skills, skills_dir.as_deref());
        Ok(SpawnCtx {
            session_id: session.id.clone(),
            // Minted here, once per launch: the row is told it in
            // [`Self::launch`], before the pane it belongs to exists.
            launch_id: ariadne_core::id::new_id(),
            goal_id: session.goal_id.clone(),
            task_id: session.task_id.clone(),
            seat: session.seat(),
            run_dir,
            cwd,
            socket_path: self.cfg.socket_path.clone(),
            cli_bin: self.cfg.cli_bin.clone(),
            system_prompt,
            skills_dir,
            initial_prompt,
            model: session.model.clone(),
            effort: session.effort.clone(),
            extra_flags: agent.extra_flags(),
        })
    }

    /// Shared launch tail for fresh spawns and resumes: persist the internal
    /// session id, start the tmux process, mark the session running and watch
    /// for the directory-trust dialog.
    ///
    /// `launch_id` is what this run of the agent will report under, and the
    /// row is given it before tmux is asked for a pane. Both halves of that
    /// order matter on a relaunch, where one process is being torn down as
    /// another starts under the same session id: written any later, the new
    /// agent's first events would name a launch the row had not heard of;
    /// written any earlier, the old agent's last ones would still count.
    async fn launch(&self, session: &AgentSession, plan: SpawnPlan, launch_id: &str) -> Result<()> {
        if let Some(internal) = &plan.internal_session_id {
            self.store
                .set_session_internal_id(&session.id, internal)
                .await?;
        }
        let mut env = plan.env;
        env.push(("ARIADNE_CLI".into(), self.cfg.cli_bin.clone()));
        // The console log's directory has to exist before pipe-pane appends to
        // it — a missing dir fails silently in the pipe's shell, and the agent
        // adapter only creates the run dir when it has config files to write
        // there (codex does not).
        std::fs::create_dir_all(self.run_dir(&session.id)).context("creating session run dir")?;
        if session.agent_kind() == AgentKind::Acp {
            return self
                .launch_acp(session, plan.argv, env, plan.cwd, launch_id)
                .await;
        }
        let spawn = self.tmux_spawn(session, plan.argv, env, plan.cwd)?;
        self.store
            .set_session_launch(&session.id, launch_id)
            .await?;
        self.tmux
            .new_session(&spawn)
            .await
            .context("spawning tmux session")?;
        // Stamped before the status, so that a session seen `running` is
        // always a session whose launch is dated: the scheduler measures a
        // resumed agent's silence from here, and a launch it cannot date is a
        // launch it cannot watch.
        self.store.mark_session_launched(&session.id).await?;
        self.store
            .set_session_status(&session.id, SessionStatus::Running)
            .await?;
        self.watch_for_a_dialog(session.id.clone(), session.tmux_session.clone());
        if let Some(input) = plan.post_launch_input {
            self.deliver_typed_input(session.id.clone(), session.tmux_session.clone(), input);
        }
        Ok(())
    }

    /// The ACP half of [`Self::launch`]: no pane and no `_spawn` — the daemon
    /// spawns the agent as its own child and drives it over the protocol
    /// (`crate::acp`). The spawn plan is still written as the record of what
    /// was launched, and the row moves through the same states in the same
    /// order as a tmux launch. What runs is `Config::acp_bin` — the plan's
    /// own argv head outside a test — with the rest of the argv behind it.
    async fn launch_acp(
        &self,
        session: &AgentSession,
        argv: Vec<String>,
        env: Vec<(String, String)>,
        cwd: PathBuf,
        launch_id: &str,
    ) -> Result<()> {
        let config_path = env
            .iter()
            .find(|(key, _)| key == ariadne_core::acp::CONFIG_ENV)
            .map(|(_, value)| PathBuf::from(value))
            .context("an ACP launch plan names no ACP config")?;
        let raw = std::fs::read_to_string(&config_path)
            .with_context(|| format!("reading the ACP config {}", config_path.display()))?;
        let config: ariadne_core::acp::LaunchConfig = serde_json::from_str(&raw)
            .with_context(|| format!("reading the ACP config {}", config_path.display()))?;
        let (repository_id, permission_mode) = match &session.task_id {
            Some(task_id) => {
                let task = self.store.get_task(task_id).await?;
                let permission_mode = task.permission_mode().unwrap_or(self.cfg.permission_mode);
                (task.repo_id, permission_mode)
            }
            None => {
                let repo = self
                    .store
                    .list_goal_repositories(&session.goal_id)
                    .await?
                    .into_iter()
                    .next()
                    .context("ACP session goal has no repository")?;
                (repo.id, self.cfg.permission_mode)
            }
        };
        let args = argv
            .split_first()
            .map(|(_, args)| args.to_vec())
            .expect("the ACP adapter plans a non-empty argv");
        write_spawn_plan(
            &self.spawn_plan_file(&session.id),
            &SpawnPlanFile::new(argv, env.clone(), cwd.clone()),
        )?;
        self.store
            .set_session_launch(&session.id, launch_id)
            .await?;
        self.acp
            .launch(AcpLaunch {
                session_id: session.id.clone(),
                launch_id: launch_id.to_string(),
                program: self.cfg.acp_bin.clone(),
                args,
                env,
                cwd,
                config,
                repository_id,
                permission_mode,
            })
            .await
            .context("spawning the ACP agent")?;
        self.store.mark_session_launched(&session.id).await?;
        // Running, but only over a row still starting: unlike a pane, the
        // driver reports from the moment it spawns, and an agent that failed
        // fast has ended the row by now — Running written over that would
        // resurrect it until the sweep retires it again.
        if self.store.get_session(&session.id).await?.status() == SessionStatus::Starting {
            self.store
                .set_session_status(&session.id, SessionStatus::Running)
                .await?;
        }
        Ok(())
    }

    /// The tmux side of a launch: the plan goes to a file, and tmux gets a
    /// command whose length says nothing about what is in it.
    ///
    /// It used to say everything. The agent's argv — briefing, system prompt
    /// and all — plus one `-e` pair per environment variable rode in the
    /// `tmux new-session` arguments, and tmux hands a command to its server as
    /// a single message capped near 16KB. A five-kilobyte reviewer briefing
    /// reached it: `new-session` answered "command too long" for every attempt
    /// the spawn had, and the task was failed for it.
    ///
    /// So nothing that varies goes through tmux any more. `ariadne _spawn`
    /// reads the plan, applies the environment and `exec`s the argv, which
    /// leaves the agent itself as the pane's root process — tmux is watching
    /// the same thing it always was.
    fn tmux_spawn(
        &self,
        session: &AgentSession,
        argv: Vec<String>,
        env: Vec<(String, String)>,
        cwd: PathBuf,
    ) -> Result<TmuxSpawn> {
        let cli_bin = self.spawn_cli_bin()?;
        let plan_file = self.spawn_plan_file(&session.id);
        write_spawn_plan(&plan_file, &SpawnPlanFile::new(argv, env, cwd.clone()))?;
        Ok(TmuxSpawn {
            session: session.tmux_session.clone(),
            // `_spawn` enters the plan's cwd itself; tmux is told it too so
            // that a pane which never gets that far is still where it belongs.
            cwd,
            // Deliberately empty, and the whole point: the environment is in
            // the plan file.
            env: Vec::new(),
            argv: vec![cli_bin, "_spawn".into(), plan_file.display().to_string()],
            log_file: Some(self.run_dir(&session.id).join("console.log")),
        })
    }

    /// The `ariadne` binary tmux runs, checked before the spawn rather than
    /// after.
    ///
    /// `cli_bin` used to be a string handed to the agents for their hooks and
    /// their MCP entry, where a wrong value costs a hook. It is now the pane's
    /// root process, so a wrong one costs the whole session — and the pane is
    /// gone before anyone can read why. A path is therefore checked here; a
    /// bare name is not, because the daemon cannot answer for it: the pane's
    /// `PATH` comes from the tmux server, which the daemon did not start.
    fn spawn_cli_bin(&self) -> Result<String> {
        let bin = self.cfg.cli_bin.clone();
        let bad = |reason: &str| {
            anyhow!(
                "cannot launch an agent session: cli_bin {bin:?} {reason}. \
                 Sessions are started as `<cli_bin> _spawn <plan>`, so set `cli_bin` in \
                 {}/config.toml to the path of the `ariadne` binary that belongs to this \
                 daemon.",
                self.cfg.root.display()
            )
        };
        if bin.trim().is_empty() {
            return Err(bad("is empty"));
        }
        if bin.contains('/') {
            if !probe::is_executable(Path::new(&bin)) {
                return Err(bad("is not an executable file"));
            }
        } else if !on_path(&bin) {
            // Best effort only — see above on whose PATH decides.
            tracing::warn!(
                cli_bin = %bin,
                "cli_bin is not on the daemon's PATH; agent sessions will fail to start \
                 unless the tmux server's PATH has it"
            );
        }
        Ok(bin)
    }

    async fn spawn(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        initial_prompt: String,
    ) -> Result<()> {
        let ctx = self.spawn_ctx(session, cwd, initial_prompt).await?;
        let plan = adapter_for(session.agent_kind()).plan_spawn(&ctx)?;
        self.launch(session, plan, &ctx.launch_id).await?;
        self.clear_superseded_attention(session).await;
        Ok(())
    }

    /// The session of `seat` on this task there is something to resume, and the
    /// agent conversation it left behind: the most recent one with a captured
    /// internal id.
    ///
    /// Codex and opencode report theirs from a hook, so a session that never
    /// got going may have none, and that is nothing to resume — the caller
    /// spawns afresh instead. `agent_id` tells a task's reviewers apart, which
    /// is the only thing that does; every other seat has one session per task.
    async fn resumable_session(
        &self,
        task_id: &str,
        seat: Seat,
        agent_id: Option<&str>,
    ) -> Result<Option<(AgentSession, String)>> {
        let found = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task_id.to_string()),
                ..Default::default()
            })
            .await?
            .into_iter()
            .rev()
            .find(|s| {
                s.seat() == seat
                    && agent_id.is_none_or(|wanted| s.task_agent_id.as_deref() == Some(wanted))
                    && s.internal_session_id.is_some()
            });
        Ok(found.map(|session| {
            let internal = session.internal_session_id.clone().expect("filtered above");
            (session, internal)
        }))
    }

    /// The tail every resume shares, once the row has been put back on its
    /// feet: the agent launched again in `cwd`, on the conversation `internal`
    /// names, with `instruction` to come back to.
    async fn launch_resumed(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        internal: &str,
        instruction: &str,
    ) -> Result<AgentSession> {
        let ctx = self.spawn_ctx(session, cwd, String::new()).await?;
        let plan = adapter_for(session.agent_kind()).plan_resume(&ctx, internal, instruction)?;
        self.launch(session, plan, &ctx.launch_id).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Drop the attention carried by the sessions this fresh one replaces.
    ///
    /// A session that ended needing the user keeps saying so until something
    /// is done about it, and starting its replacement is that something — but
    /// only once the replacement is actually up: a spawn that dies on the way
    /// leaves the old row flagged, which is what the flag is for. Resumes take
    /// the other road (`restart_session` clears the row it relaunches); this
    /// is for the fresh spawn that supersedes a row instead of reviving it.
    ///
    /// "Replaces" is the identity a spawn is refused for: the seat on this
    /// goal and task, and for a reviewer the profile too, since a task's
    /// reviewers are siblings that only their profile tells apart.
    async fn clear_superseded_attention(&self, session: &AgentSession) {
        let Ok(siblings) = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(session.goal_id.clone()),
                task_id: session.task_id.clone(),
                ..Default::default()
            })
            .await
        else {
            return;
        };
        for previous in siblings {
            if previous.id != session.id
                && previous.task_id == session.task_id
                && previous.seat() == session.seat()
                && (previous.seat() != Seat::Reviewer
                    || previous.task_agent_id == session.task_agent_id)
                && previous.attention_reason().is_some()
            {
                tracing::info!(
                    session = %previous.id,
                    replacement = %session.id,
                    "superseded by a fresh session, clearing its attention"
                );
                let _ = self.store.clear_session_attention(&previous.id).await;
            }
        }
    }

    /// Spawn the orchestrator for a goal (cwd = first repo).
    pub async fn spawn_orchestrator(&self, goal_id: &str) -> Result<AgentSession> {
        let goal = self.store.get_goal(goal_id).await?;
        let repos = self.store.list_goal_repositories(goal_id).await?;
        let repo = repos.first().context("goal has no repos")?;
        self.assert_no_live_session(goal_id, None, Seat::Orchestrator, None)
            .await?;
        let tmux_session = session_name(&goal.id, None, "orchestrator", None);
        self.claim_pane(&tmux_session).await?;

        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: None,
                seat: Seat::Orchestrator,
                task_agent_id: None,
                agent_kind: goal.agent_kind(),
                model: goal.model.clone(),
                effort: goal.effort.clone(),
                tmux_session,
                worktree_path: None,
            })
            .await?;

        let template = prompts::template_for(PromptKind::OrchestratorBriefing);
        let briefing = prompts::orchestrator_briefing(template, &goal, &repos);
        self.spawn(&session, PathBuf::from(&repo.path), briefing)
            .await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Spawn the author for a task: worktree + branch + session. On a task
    /// staffed with several authors this is the picked winner where the pick
    /// has settled, and the first author otherwise —
    /// [`Self::spawn_author_agent`] is how a named one of several starts.
    pub async fn spawn_author(&self, task_id: &str) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let author = match &task.picked_agent_id {
            Some(picked) => self.store.get_task_agent(picked).await?,
            None => self.store.task_author(task_id).await?,
        };
        self.spawn_author_agent(task_id, &author.id).await
    }

    /// Spawn one author of a task, by the staffed agent it runs.
    pub async fn spawn_author_agent(&self, task_id: &str, agent_id: &str) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        let seat = self.author_seat(&task, agent_id).await?;
        // One live session per author. A one-author task is guarded by the
        // seat alone, as it always was, so a session staffed on an author the
        // task no longer lists still refuses a second.
        let guard = seat.sibling_tail().map(|_| seat.author.id.as_str());
        self.assert_no_live_session(&goal.id, Some(task_id), Seat::Author, guard)
            .await?;
        let tmux_session = session_name(
            &goal.id,
            Some(&task.id),
            "author",
            seat.sibling_tail().as_deref(),
        );
        // An acp author runs no pane: the name is stored — the row carries
        // one for every session — but nothing tmux is claimed or created.
        if seat.author.agent_kind() != AgentKind::Acp {
            self.claim_pane(&tmux_session).await?;
        }

        let worktree = self.author_worktree(&task, &repo, None, &seat).await?;

        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                seat: Seat::Author,
                task_agent_id: Some(seat.author.id.clone()),
                agent_kind: seat.author.agent_kind(),
                model: seat.author.model.clone(),
                effort: seat.author.effort.clone(),
                tmux_session,
                worktree_path: Some(worktree.display().to_string()),
            })
            .await?;

        // Re-read: worktree_path may just have been set.
        let task = self.store.get_task(task_id).await?;
        let mut deps = Vec::new();
        for dep_id in self.store.list_task_dependencies(&task.id).await? {
            deps.push(self.store.get_task(&dep_id).await?);
        }
        let template = prompts::template_for(PromptKind::AuthorBriefing);
        let seen = seat.task_as_seen(&task, Some(worktree.display().to_string()));
        let briefing = prompts::author_briefing(template, &seen, &goal, &repo, &deps);
        self.spawn(&session, worktree, briefing).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Give a ready task an author session that resumes an outside
    /// conversation in the task's worktree: a hand-started CLI transcript, or
    /// a stored ACP session, continued through `session/load`.
    ///
    /// The adopted conversation becomes the task's first author. A task
    /// staffed with several authors adopts into that seat alone; its
    /// siblings are spawned by the scheduler as usual.
    pub async fn adopt_author(
        &self,
        task_id: &str,
        agent_kind: AgentKind,
        agent_id: Option<&str>,
        internal_session_id: &str,
    ) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        if task.status() != TaskStatus::Ready {
            anyhow::bail!("task {task_id} is {}, not ready", task.status);
        }
        let goal = self.store.get_goal(&task.goal_id).await?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        let author = self.store.task_author(task_id).await?;
        if author.agent_kind() != agent_kind {
            anyhow::bail!(
                "outside session uses {}, but task author uses {}",
                agent_kind.as_str(),
                author.agent_kind().as_str()
            );
        }
        // An acp author's model carries `<agent id>:<model>`: which ACP
        // agent this is is part of the pin, not a separate field, so the
        // outside session's agent id has to match it the same way the CLI
        // kind above does.
        if agent_kind == AgentKind::Acp {
            let pinned_agent_id = author.model.split_once(':').map(|(id, _)| id);
            if agent_id != pinned_agent_id {
                anyhow::bail!(
                    "outside session belongs to acp agent {}, but task author uses {}",
                    agent_id.unwrap_or("<none>"),
                    pinned_agent_id.unwrap_or("<none>")
                );
            }
        }
        let seat = self.author_seat(&task, &author.id).await?;
        let guard = seat.sibling_tail().map(|_| author.id.clone());
        self.assert_no_live_session(&goal.id, Some(task_id), Seat::Author, guard.as_deref())
            .await?;
        let tmux_session = session_name(
            &goal.id,
            Some(&task.id),
            "author",
            seat.sibling_tail().as_deref(),
        );
        // An acp author runs no pane, exactly as a fresh spawn does.
        if agent_kind != AgentKind::Acp {
            self.claim_pane(&tmux_session).await?;
        }
        let worktree = self.author_worktree(&task, &repo, None, &seat).await?;
        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                seat: Seat::Author,
                task_agent_id: Some(author.id.clone()),
                agent_kind,
                model: author.model.clone(),
                effort: author.effort.clone(),
                tmux_session,
                worktree_path: Some(worktree.display().to_string()),
            })
            .await?;
        self.store
            .set_session_internal_id(&session.id, internal_session_id)
            .await?;
        let task = self.store.get_task(task_id).await?;
        let mut deps = Vec::new();
        for dep_id in self.store.list_task_dependencies(&task.id).await? {
            deps.push(self.store.get_task(&dep_id).await?);
        }
        let template = prompts::template_for(PromptKind::AuthorBriefing);
        let seen = seat.task_as_seen(&task, Some(worktree.display().to_string()));
        let briefing = prompts::author_briefing(template, &seen, &goal, &repo, &deps);
        self.launch_resumed(&session, worktree, internal_session_id, &briefing)
            .await
    }

    /// One author's place on a task: the agent row, its branch, and whether
    /// it is the task's lone author.
    async fn author_seat(&self, task: &Task, agent_id: &str) -> Result<AuthorSeat> {
        let authors = self.store.list_task_authors(&task.id).await?;
        let lone = authors.len() == 1;
        let author = authors
            .into_iter()
            .find(|a| a.id == agent_id)
            .ok_or_else(|| anyhow!("agent {agent_id} is not an author of task {}", task.id))?;
        let branch = author_branch(&task.branch, author.ordinal);
        Ok(AuthorSeat {
            author,
            branch,
            lone,
        })
    }

    /// The author's worktree, checked out on its own branch: created on the
    /// first spawn, and created again whenever it has been cleaned up under a
    /// task that is still going. Nobody else ever holds the branch — the
    /// author keeps it from the first commit to the merge.
    ///
    /// `keep` is the tree a resumed author was working in — kept while it is
    /// still on disk, since an agent is put back where it left off rather than
    /// beside it. A fresh spawn passes `None` and gets the canonical path.
    ///
    /// The task's own `worktree_path` is the lone author's — set here for a
    /// one-author task exactly as it always was, and for one of several only
    /// once the pick has named it the winner (`scheduler::tasks`).
    async fn author_worktree(
        &self,
        task: &Task,
        repo: &Repository,
        keep: Option<PathBuf>,
        seat: &AuthorSeat,
    ) -> Result<PathBuf> {
        let worktree = match keep {
            Some(existing) if existing.is_dir() => existing,
            _ => {
                let name = match seat.sibling_tail() {
                    None => format!("{}-eng", tail(&task.id)),
                    Some(sibling) => format!("{}-eng-{sibling}", tail(&task.id)),
                };
                self.cfg.worktree_root.join(tail(&task.goal_id)).join(name)
            }
        };
        if !worktree.exists() {
            std::fs::create_dir_all(worktree.parent().unwrap())?;
            self.git
                .add_worktree(
                    &PathBuf::from(&repo.path),
                    &worktree,
                    &seat.branch,
                    &repo.base_branch,
                )
                .await?;
        }
        if seat.lone {
            self.store
                .set_task_worktree(&task.id, Some(&worktree.display().to_string()))
                .await?;
        }
        // There is a tree to commit in now: follow what the branch does.
        match seat.sibling_tail() {
            None => self.branches.watch(task, Path::new(&repo.path)),
            Some(_) => self.branches.watch_author(
                task,
                &seat.author.id,
                &seat.branch,
                Path::new(&repo.path),
            ),
        }
        Ok(worktree)
    }

    /// The reviewer's detached worktree, pinned at the branch tip: created on
    /// the first round, re-pointed at the tip on every later one — the same
    /// worktree serves the whole review, as the same session does.
    ///
    /// The tip has to be a commit. In a repository that had none when the task
    /// started, the task branch is unborn until the author commits, and there
    /// is nothing for a reviewer to be pinned at.
    ///
    /// `branch` is the branch under review — the task's own where the caller
    /// names none, and one author's of several where it does. One worktree
    /// serves the reviewer either way, re-pointed at whichever review it is
    /// briefed on.
    async fn reviewer_worktree(
        &self,
        task: &Task,
        agent_id: &str,
        branch: Option<&str>,
    ) -> Result<PathBuf> {
        let branch = branch.unwrap_or(&task.branch);
        let repo_path = {
            let repo = self.store.get_repository(&task.repo_id).await?;
            PathBuf::from(repo.path)
        };
        self.git
            .ensure_branch_has_commits(&repo_path, branch)
            .await
            .with_context(|| format!("task {} has nothing to review yet", task.id))?;
        let worktree = self
            .cfg
            .worktree_root
            .join(tail(&task.goal_id))
            .join(format!("{}-rev-{}", tail(&task.id), tail(agent_id)));
        if worktree.exists() {
            // New round: refresh to the current branch tip.
            self.git.checkout_detached(&worktree, branch).await?;
        } else {
            std::fs::create_dir_all(worktree.parent().unwrap())?;
            self.git
                .add_detached_worktree(&repo_path, &worktree, branch)
                .await?;
        }
        Ok(worktree)
    }

    /// Spawn one reviewer for a task (detached worktree at the branch tip).
    ///
    /// The session is not tied to the round it starts in: later rounds resume
    /// this very session (see [`Launcher::resume_reviewer`]), so its name says
    /// which reviewer of which task it is and nothing about when it began.
    pub async fn spawn_reviewer(&self, task_id: &str, agent_id: &str) -> Result<AgentSession> {
        self.spawn_reviewer_for(task_id, agent_id, None).await
    }

    /// The same, started for one named author's review: its worktree pins at
    /// that author's branch, and its briefing carries that author's summary.
    /// `None` reads the task's own branch and summary, which is the whole of
    /// a one-author task.
    pub async fn spawn_reviewer_for(
        &self,
        task_id: &str,
        agent_id: &str,
        author: Option<&str>,
    ) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        // The agent has to be one this task staffs as a reviewer: that is both
        // the proof it reviews the task at all, and where its pin comes from.
        let reviewer = self
            .store
            .list_task_reviewers(task_id)
            .await?
            .into_iter()
            .find(|r| r.id == agent_id)
            .ok_or_else(|| anyhow!("agent {agent_id} is not a reviewer of task {task_id}"))?;
        self.assert_no_live_session(&goal.id, Some(task_id), Seat::Reviewer, Some(&reviewer.id))
            .await?;
        let tmux_session = session_name(
            &goal.id,
            Some(&task.id),
            "reviewer",
            Some(tail(&reviewer.id)),
        );
        self.claim_pane(&tmux_session).await?;

        let branch = self.review_branch(&task, author).await?;
        let worktree = self
            .reviewer_worktree(&task, &reviewer.id, branch.as_deref())
            .await?;
        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                seat: Seat::Reviewer,
                task_agent_id: Some(reviewer.id.clone()),
                agent_kind: reviewer.agent_kind(),
                model: reviewer.model.clone(),
                effort: reviewer.effort.clone(),
                tmux_session,
                worktree_path: Some(worktree.display().to_string()),
            })
            .await?;

        let summary = match author {
            Some(author_id) => Some(verdict_addressed_to(
                author_id,
                self.store
                    .author_review_summary(task_id, author_id)
                    .await?
                    .as_deref(),
            )),
            None => self.store.review_summary(&task.id).await?,
        };
        let seen = match branch {
            Some(branch) => Task {
                branch,
                ..task.clone()
            },
            None => task.clone(),
        };
        let template = prompts::template_for(PromptKind::ReviewerBriefing);
        let briefing =
            prompts::reviewer_briefing(template, &seen, &goal, &repo, summary.as_deref());
        self.spawn(&session, worktree, briefing).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Move a reviewer's detached worktree to the branch of the review it is
    /// about to be briefed on, while its session stays up.
    ///
    /// The resume paths re-point the worktree by relaunching the session;
    /// this is the same re-point for a pane that survived the last review —
    /// a contested task hands a live reviewer the next author's review, and
    /// the tree it verifies in has to be on that author's branch before the
    /// briefing that names it lands. The reviewer is detached and read-only,
    /// so between reviews there is nothing of its own in the tree to lose.
    pub async fn refresh_reviewer_worktree(
        &self,
        task_id: &str,
        agent_id: &str,
        author: Option<&str>,
    ) -> Result<PathBuf> {
        let task = self.store.get_task(task_id).await?;
        let branch = self.review_branch(&task, author).await?;
        self.reviewer_worktree(&task, agent_id, branch.as_deref())
            .await
    }

    /// The branch one review is about: the named author's own, or `None` for
    /// the task's — a one-author task, or a caller that did not say.
    pub(crate) async fn review_branch(
        &self,
        task: &Task,
        author: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(author_id) = author else {
            return Ok(None);
        };
        let agent = self.store.get_task_agent(author_id).await?;
        Ok(Some(author_branch(&task.branch, agent.ordinal)))
    }

    /// Resume a reviewer's previous agent session for the task's current
    /// round, relaunching the very same session — row, id and tmux name — so
    /// a reviewer that sees a task through several rounds remembers what it
    /// asked for last time instead of reading the change afresh every round
    /// (spawn afresh if there is nothing to resume).
    ///
    /// The reviewer's worktree is re-pointed at the branch tip before the
    /// agent starts, so the tree it wakes up in is the one it is asked about;
    /// the row's `review_round` moves to the round being reviewed now.
    pub async fn resume_reviewer(
        &self,
        task_id: &str,
        agent_id: &str,
        instruction: &str,
    ) -> Result<AgentSession> {
        self.resume_reviewer_for(task_id, agent_id, None, instruction)
            .await
    }

    /// The same, resumed for one named author's review: the worktree is
    /// re-pointed at that author's branch rather than the task's.
    pub async fn resume_reviewer_for(
        &self,
        task_id: &str,
        agent_id: &str,
        author: Option<&str>,
        instruction: &str,
    ) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let reviewer = self.store.get_task_agent(agent_id).await?;

        let Some((previous, internal)) = self
            .resumable_session(&task.id, Seat::Reviewer, Some(&reviewer.id))
            .await?
        else {
            return self.spawn_reviewer_for(task_id, agent_id, author).await;
        };

        let branch = self.review_branch(&task, author).await?;
        let worktree = self
            .reviewer_worktree(&task, &reviewer.id, branch.as_deref())
            .await?;
        if self.tmux.has_session(&previous.tmux_session).await {
            self.tmux.kill_session(&previous.tmux_session).await.ok();
        }
        let session = self
            .store
            .restart_session(&previous.id, Some(&worktree.display().to_string()))
            .await?;

        self.launch_resumed(&session, worktree, &internal, instruction)
            .await
    }

    /// Resume the author's previous agent session with a new instruction,
    /// relaunching the very same session — row, id and tmux name — so a task
    /// bounced through several review rounds keeps one author session rather
    /// than one per round (spawn afresh if there is nothing to resume). On a
    /// task staffed with several authors this is the picked winner where the
    /// pick has settled, and the first author otherwise —
    /// [`Self::resume_author_agent`] is how a named one of several comes back.
    pub async fn resume_author(&self, task_id: &str, instruction: &str) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let author = match &task.picked_agent_id {
            Some(picked) => self.store.get_task_agent(picked).await?,
            None => self.store.task_author(task_id).await?,
        };
        self.resume_author_agent(task_id, &author.id, instruction)
            .await
    }

    /// Resume one author of a task, by the staffed agent it runs.
    pub async fn resume_author_agent(
        &self,
        task_id: &str,
        agent_id: &str,
        instruction: &str,
    ) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let seat = self.author_seat(&task, agent_id).await?;
        // A one-author task resumes whatever author session it last had, as
        // it always did — a session staffed before a re-staff included. One
        // of several is its own conversation, so only its own comes back.
        let of_agent = seat.sibling_tail().map(|_| agent_id);
        let Some((previous, internal)) = self
            .resumable_session(&task.id, Seat::Author, of_agent)
            .await?
        else {
            return self.spawn_author_agent(task_id, agent_id).await;
        };
        // The tree it was working in, from the task or from the session's own
        // row, and a new one in its place where it is no longer on disk. The
        // task's own column is the lone author's (or the picked winner's), so
        // one of several trusts its session row alone.
        let keep = match seat.sibling_tail() {
            None => task
                .worktree_path
                .clone()
                .or_else(|| previous.worktree_path.clone()),
            Some(_) => match task.picked_agent_id.as_deref() == Some(agent_id) {
                true => task
                    .worktree_path
                    .clone()
                    .or_else(|| previous.worktree_path.clone()),
                false => previous.worktree_path.clone(),
            },
        }
        .map(PathBuf::from);
        let repo = self.store.get_repository(&task.repo_id).await?;
        let worktree = self.author_worktree(&task, &repo, keep, &seat).await?;
        if self.tmux.has_session(&previous.tmux_session).await {
            self.tmux.kill_session(&previous.tmux_session).await.ok();
        }
        // Same conversation, same session: the row goes back to `starting` and
        // is launched again. Its console log is appended to rather than rolled
        // over, so the terminal reads as the one continuous transcript the
        // agent actually produced.
        let session = self
            .store
            .restart_session(&previous.id, Some(&worktree.display().to_string()))
            .await?;

        self.launch_resumed(&session, worktree, &internal, instruction)
            .await
    }

    /// Revive an ended session in a fresh tmux, continuing the same agent
    /// conversation via its stored internal id. The session itself is revived
    /// — same row, same id — so the caller gets back what it asked for. Used
    /// by `ariadne attach` when no tmux is alive. `instruction: None` resumes
    /// into an idle TUI so the user can type themselves.
    pub async fn revive_session(
        &self,
        session_id: &str,
        instruction: Option<&str>,
    ) -> Result<AgentSession> {
        let previous = self.store.get_session(session_id).await?;
        // "Could not ask" counts as alive here, the way it does for the spawn
        // guards: a tmux that cannot be reached has said nothing about the
        // pane, and a relaunch on top of a live agent puts two of them on one
        // piece of work. A wrong "yes" costs a tick, and the caller asks
        // again.
        if self.session_process_alive(&previous).await {
            // Already alive — attaching needs nothing from us.
            return Ok(previous);
        }
        // A finished goal has no work left for an agent to come back to, and
        // the scheduler kills what is live under one: reviving here would put
        // a session up only for the next tick to take it down again. Refused
        // at the source instead, so nobody watches an agent start and vanish.
        let goal = self.store.get_goal(&previous.goal_id).await?;
        if goal.status().is_terminal() {
            anyhow::bail!(
                "cannot revive session {}: its goal is {}",
                previous.id,
                goal.status
            );
        }
        let internal = previous.internal_session_id.clone().with_context(|| {
            format!(
                "session {} has no internal agent id to resume from",
                previous.id
            )
        })?;
        let seat = previous.seat();

        let cwd = match seat {
            Seat::Orchestrator => {
                let repos = self.store.list_goal_repositories(&previous.goal_id).await?;
                PathBuf::from(&repos.first().context("goal has no repos")?.path)
            }
            Seat::Author | Seat::Reviewer => PathBuf::from(
                previous
                    .worktree_path
                    .clone()
                    .context("session has no worktree to revive in")?,
            ),
        };
        if !cwd.is_dir() {
            anyhow::bail!(
                "cannot revive session {}: its working directory {} is gone \
                 (task finished and was cleaned up?)",
                previous.id,
                cwd.display()
            );
        }

        // Neither the worktree nor (for a reviewer) the round changes: this is
        // the same session put back on its feet, not a new round of work.
        let session = self.store.restart_session(&previous.id, None).await?;
        self.launch_resumed(&session, cwd, &internal, instruction.unwrap_or(""))
            .await
    }

    /// Kill a session's agent process and mark the session exited: the tmux
    /// pane of most kinds, the daemon-owned child of an `acp` session.
    pub async fn kill_session(&self, session_id: &str) -> Result<()> {
        let session = self.store.get_session(session_id).await?;
        if session.agent_kind() == AgentKind::Acp {
            self.acp.kill(&session.id);
        } else if self.tmux.has_session(&session.tmux_session).await {
            self.tmux.kill_session(&session.tmux_session).await?;
        }
        if session.status().is_live() {
            self.store
                .set_session_status(session_id, SessionStatus::Exited)
                .await?;
        }
        Ok(())
    }

    /// Follow the branch of every task that already has a worktree.
    ///
    /// What the daemon does once at startup: the watches live only as long as
    /// the process holding them, and the tasks in flight when the last one
    /// stopped are still being committed to. A task whose repository cannot be
    /// read is skipped rather than failing the sweep — the others are worth
    /// following either way.
    pub async fn watch_task_branches(&self) -> Result<()> {
        for task in self.store.list_tasks(TaskFilter::default()).await? {
            let repo = match self.store.get_repository(&task.repo_id).await {
                Ok(repo) => repo,
                Err(e) => {
                    tracing::warn!(task = %task.id, error = %e, "cannot follow the task branch");
                    continue;
                }
            };
            // A task staffed with several authors keeps its worktrees on the
            // sessions rather than on the task, so each author whose session
            // holds one is followed on its own branch.
            let authors = self.store.list_task_authors(&task.id).await?;
            if authors.len() > 1 && !task.status().is_terminal() {
                let with_worktrees: std::collections::HashSet<String> = self
                    .store
                    .list_sessions(SessionFilter {
                        task_id: Some(task.id.clone()),
                        ..Default::default()
                    })
                    .await?
                    .into_iter()
                    .filter(|s| s.worktree_path.is_some())
                    .filter_map(|s| s.task_agent_id)
                    .collect();
                for author in authors.iter().filter(|a| with_worktrees.contains(&a.id)) {
                    let branch = author_branch(&task.branch, author.ordinal);
                    self.branches
                        .watch_author(&task, &author.id, &branch, Path::new(&repo.path));
                }
                continue;
            }
            if worth_following(&task) {
                self.branches.watch(&task, Path::new(&repo.path));
            }
        }
        Ok(())
    }

    /// Cleanup after a finished/cancelled task: kill sessions, remove
    /// worktrees, optionally delete the branch. Idempotent: safe to call
    /// repeatedly on the same task.
    ///
    /// `remove_worktrees = false` keeps the worktrees on disk (and therefore
    /// also the branch — the author worktree has it checked out, which pins
    /// it) so finished or cancelled work can be inspected later.
    pub async fn cleanup_task(
        &self,
        task_id: &str,
        remove_worktrees: bool,
        delete_branch: bool,
    ) -> Result<()> {
        let task = self.store.get_task(task_id).await?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        let repo_path = PathBuf::from(&repo.path);
        // The task is over: whatever happens to its branch from here is not
        // something anybody is following it for.
        self.branches.unwatch(task_id);

        for session in self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?
        {
            if self.tmux.has_session(&session.tmux_session).await {
                tracing::info!(task = %task.id, session = %session.id, "cleanup: killing agent session");
            }
            self.kill_session(&session.id).await.ok();
        }

        if !remove_worktrees {
            return Ok(());
        }

        for session in self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await?
        {
            if let Some(wt) = &session.worktree_path {
                let wt = PathBuf::from(wt);
                if wt.exists() {
                    tracing::info!(task = %task.id, worktree = %wt.display(), "cleanup: removing worktree");
                    self.git.remove_worktree(&repo_path, &wt).await.ok();
                }
            }
        }
        if let Some(wt) = &task.worktree_path {
            let wt = PathBuf::from(wt);
            if wt.exists() {
                tracing::info!(task = %task.id, worktree = %wt.display(), "cleanup: removing worktree");
                self.git.remove_worktree(&repo_path, &wt).await.ok();
            }
            self.store.set_task_worktree(&task.id, None).await?;
        }
        self.git.prune_worktrees(&repo_path).await.ok();
        if delete_branch && task.status() == TaskStatus::Finished {
            // Every author's branch, not only the task's own: the losers of
            // a several-author task went with the pick, but a crash between
            // the pick and this cleanup leaves theirs for here.
            let mut branches = vec![task.branch.clone()];
            for author in self.store.list_task_authors(&task.id).await? {
                branches.push(author_branch(&task.branch, author.ordinal));
            }
            branches.dedup();
            for branch in branches {
                if self
                    .git
                    .branch_exists(&repo_path, &branch)
                    .await
                    .unwrap_or(false)
                {
                    self.git.delete_branch(&repo_path, &branch).await.ok();
                }
            }
        }
        Ok(())
    }

    /// Take down the authors the pick passed over: their sessions, their
    /// worktrees and their branches, leaving the winner's untouched.
    ///
    /// Unconditional, unlike the merged-work cleanup behind the config flags:
    /// a losing branch is one the reviewers judged and set aside, and the
    /// task is still running — nothing later comes back for it. Idempotent,
    /// so a pass that crashed halfway just runs again.
    pub async fn cleanup_losing_authors(&self, task_id: &str) -> Result<()> {
        let task = self.store.get_task(task_id).await?;
        let Some(winner) = task.picked_agent_id.clone() else {
            return Ok(());
        };
        let repo = self.store.get_repository(&task.repo_id).await?;
        let repo_path = PathBuf::from(&repo.path);
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await?;
        for author in self.store.list_task_authors(&task.id).await? {
            if author.id == winner {
                continue;
            }
            self.branches.unwatch_author(&task.id, &author.id);
            for session in sessions
                .iter()
                .filter(|s| s.task_agent_id.as_deref() == Some(author.id.as_str()))
            {
                if session.status().is_live() {
                    tracing::info!(task = %task.id, session = %session.id, "the pick passed this author over, killing its session");
                    self.kill_session(&session.id).await.ok();
                }
                if let Some(wt) = &session.worktree_path {
                    let wt = PathBuf::from(wt);
                    if wt.exists() {
                        tracing::info!(task = %task.id, worktree = %wt.display(), "removing a losing author's worktree");
                        self.git.remove_worktree(&repo_path, &wt).await.ok();
                    }
                }
            }
            let branch = author_branch(&task.branch, author.ordinal);
            if self
                .git
                .branch_exists(&repo_path, &branch)
                .await
                .unwrap_or(false)
            {
                tracing::info!(task = %task.id, branch = %branch, "deleting a losing author's branch");
                self.git.delete_branch(&repo_path, &branch).await.ok();
            }
        }
        self.git.prune_worktrees(&repo_path).await.ok();
        Ok(())
    }
}

/// One author's place on a task: the agent row, the branch it owns, and
/// whether it is the task's lone author.
///
/// The lone author is the shape everything always had — the task branch, the
/// `-eng` worktree, the unsuffixed tmux name — so `sibling_tail()` is what
/// every naming decision reads: `None` keeps a one-author task exactly as it
/// was, and `Some` carries the tail that tells several authors apart.
struct AuthorSeat {
    author: TaskAgent,
    /// The branch this author works on ([`author_branch`]).
    branch: String,
    lone: bool,
}

impl AuthorSeat {
    /// What tells this author from its siblings, or `None` for a lone one.
    fn sibling_tail(&self) -> Option<String> {
        (!self.lone).then(|| tail(&self.author.id).to_string())
    }

    /// The task as this author sees it: its own branch and worktree in place
    /// of the task's, so every briefing renders the seat it is for. A lone
    /// author sees the task as it stands.
    fn task_as_seen(&self, task: &Task, worktree: Option<String>) -> Task {
        match self.lone {
            true => task.clone(),
            false => Task {
                branch: self.branch.clone(),
                worktree_path: worktree.or_else(|| task.worktree_path.clone()),
                ..task.clone()
            },
        }
    }
}

/// The summary one reviewer reads on a contested task, opened by the address
/// its verdict takes: several reviews run side by side there, and a verdict
/// that names no author is one the daemon refuses.
pub(crate) fn verdict_addressed_to(author_id: &str, summary: Option<&str>) -> String {
    format!(
        "Give your verdict to author {author_id}.\n\n{}",
        summary.unwrap_or("(none provided)")
    )
}

/// Whether a task's branch is one to follow: there is a worktree to commit in,
/// and a task still being worked on in it.
///
/// A failed task is not terminal — the user can retry it, and the spawn that
/// revives it takes the watch up again — but until then nobody is committing
/// on its branch, and nothing should be said about it.
pub(crate) fn worth_following(task: &Task) -> bool {
    task.worktree_path.is_some()
        && !task.status().is_terminal()
        && task.status() != TaskStatus::Failed
}

/// Raise a session for the user, from a spawned task that has nothing to
/// return its failure to. A flag that will not store is only worth a line in
/// the log — the delivery it was about is already lost.
async fn raise_stalled(store: &Store, session_id: &str) {
    if let Err(e) = store
        .set_session_attention(session_id, AttentionReason::Stalled)
        .await
    {
        tracing::warn!(session = %session_id, error = %e, "flagging the session failed");
    }
}

/// Write a spawn plan where `ariadne _spawn` will read it.
///
/// 0600: the plan holds the agent's whole environment, and everything a
/// session was told. The mode is set as the file is created, which is the only
/// way it comes into being — the run dir is the daemon's own.
fn write_spawn_plan(path: &Path, plan: &SpawnPlanFile) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let json = plan.to_json().context("rendering the spawn plan")?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating the spawn plan {}", path.display()))?;
    file.write_all(json.as_bytes())
        .with_context(|| format!("writing the spawn plan {}", path.display()))
}

/// Whether a bare name is an executable on the daemon's own `PATH`.
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| probe::which(&path, name).is_some())
}

/// Raise a session for the user over a question on its own terminal. The
/// answer is theirs to give: the daemon presses nothing into a dialog, and
/// the flag is what tells them there is one.
async fn raise_waiting_input(store: &Store, session_id: &str) {
    if let Err(e) = store
        .set_session_attention(session_id, AttentionReason::WaitingInput)
        .await
    {
        tracing::warn!(session = %session_id, error = %e, "flagging the session failed");
    }
}

/// Whether this pane is sitting on a dialog only a person can answer.
///
/// Read off the screen as it is drawn: a CLI colours the words of its dialog,
/// and a pattern is no use against a line with an escape sequence through the
/// middle of it.
fn dialog_on_the_pane(pane: &str) -> bool {
    let screen = plain(pane).to_lowercase();
    Launcher::DIALOG_PATTERNS
        .iter()
        .any(|pattern| screen.contains(pattern))
}

/// A pane without the escape sequences `capture-pane -e` carries: the colours
/// a dialog highlights its selection with, and the hyperlinks its help text is
/// wrapped in. What is left is what a person reads off the screen.
fn plain(pane: &str) -> String {
    let mut out = String::with_capacity(pane.len());
    let mut chars = pane.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // A control sequence runs to its final byte.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            // An operating-system command (a hyperlink, here) runs to a BEL or
            // a string terminator, whose ESC this loop sees as the next one.
            Some(']') => {
                for c in chars.by_ref() {
                    if c == '\x07' || c == '\x1b' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{dialog_on_the_pane, plain};

    /// What Claude Code 2.1 draws over a folder it does not know, colours and
    /// hyperlink included, with the answer that closes the agent highlighted.
    /// The daemon that pressed Enter on it spawned an orchestrator every five
    /// seconds for as long as its goal wanted one.
    const CLAUDE: &str = "\u{1b}[38;5;220m────────\u{1b}[39m\n\
        \u{1b}[38;5;220m\u{1b}[1mAccessing workspace:\u{1b}[22m\u{1b}[39m\n\
        \u{1b}[1m/Users/me/dev/test\u{1b}[22m\n\
        Quick safety check: Is this a project you created or one you trust?\n\
        \u{1b}]8;id=zaxmda;https://code.claude.com/docs/en/security\u{1b}\\\u{1b}[38;5;246mSecurity guide\u{1b}[39m\u{1b}]8;;\u{1b}\\\n\
        \u{1b}[38;5;153m❯ No, exit\u{1b}[39m\n\
        \x20 Yes, I trust this folder\n\
        \u{1b}[38;5;246mEnter to confirm · Esc to cancel\u{1b}[39m\n";

    /// The escape sequences go, the words stay — including the ones inside a
    /// hyperlink, which is where the OSC form of them shows up.
    #[test]
    fn a_pane_reads_as_what_is_on_the_screen() {
        let screen = plain(CLAUDE);
        assert!(screen.contains("❯ No, exit"), "{screen}");
        assert!(screen.contains("Security guide"), "{screen}");
        assert!(!screen.contains('\u{1b}'), "{screen}");
    }

    /// A dialog is recognised through the colours it is drawn in, which is
    /// what a pattern read off the raw capture would miss.
    #[test]
    fn a_trust_dialog_is_recognised_on_a_pane() {
        assert!(dialog_on_the_pane(CLAUDE));
        assert!(dialog_on_the_pane(
            "Do you trust the files in this folder?\n> 1. Yes, allow it\n  2. No, exit\n"
        ));
    }

    /// And an agent at work is not a question: nothing is flagged for a pane
    /// the user has nothing to answer on.
    #[test]
    fn a_working_pane_is_not_a_question() {
        assert!(!dialog_on_the_pane(
            "\u{1b}[2m> Try \"how does this work?\"\u{1b}[22m\n  auto mode on\n"
        ));
        assert!(!dialog_on_the_pane(""));
    }
}
