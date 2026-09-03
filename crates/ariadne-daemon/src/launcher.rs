//! Launcher: turns "spawn an agent for X" into a worktree, a session row and
//! a tmux process.
//!
//! A launch is refused rather than duplicated. Every spawn asks first whether
//! the role already has a live session, and counts "tmux could not be asked"
//! as a yes: a wrong no puts two agents on one piece of work, where a wrong
//! yes costs a scheduler tick that asks again.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};

use ariadne_core::spawn_plan::SpawnPlanFile;
use ariadne_core::{
    AgentKind, AttentionReason, PromptKind, Role, SessionStatus, TaskStatus, probe,
};
use ariadne_store::{
    AgentSession, NewSession, Profile, Repository, SessionFilter, Store, Task, TaskFilter,
};

use crate::agents::{SpawnCtx, SpawnPlan, adapter_for, detect_first_available, prompts};
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
/// How long the directory-trust watcher watches for the dialog before giving
/// up. Generous (two minutes): a slow CLI start renders the dialog well after
/// the spawn, and a watcher that has already stopped leaves the agent waiting
/// on it for ever.
const TRUST_WATCH: Duration = Duration::from_secs(120);
/// And how long it leaves the pane alone after accepting one, before reading
/// it again. Unshortened on purpose: what this waits for is the dialog to go,
/// and every capture taken before it has is another Enter into a pane that
/// has already answered.
const TRUST_ACCEPTED: Duration = Duration::from_millis(700);
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
    /// The task branches whose head the daemon is following, so that a commit
    /// an engineer makes reaches the clients watching its diff.
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

    /// Agent kind for a pinned value: the pin itself, or the first installed
    /// CLI (claude_code, then codex, then opencode) when the pin is auto.
    ///
    /// The pin is the one taken from the profile when the work was defined —
    /// the task's for an engineer, the reviewer slot's for a reviewer, the
    /// goal's for a planner — never the profile as it reads now. `owner` names
    /// the row it came from, for the error a missing CLI raises.
    fn resolve_agent_kind(&self, pinned: Option<AgentKind>, owner: &str) -> Result<AgentKind> {
        match pinned {
            Some(kind) => Ok(kind),
            None => detect_first_available().ok_or_else(|| {
                anyhow!(
                    "{owner} is pinned to no agent kind (auto) and no coding agent CLI (claude, codex, opencode) was found on PATH"
                )
            }),
        }
    }

    /// Refuse to double-spawn: one live session per (task, role) —
    /// per (task, role, profile) for reviewers.
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
        role: Role,
        profile_id: Option<&str>,
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
            if s.role() == role
                && (role != Role::Reviewer || profile_id.is_none_or(|p| p == s.profile_id))
                && self.tmux.has_session_or_unknown(&s.tmux_session).await
            {
                return Err(anyhow!(
                    "a live {} session already exists: {} (tmux {})",
                    role.as_str(),
                    s.id,
                    s.tmux_session
                ));
            }
        }
        Ok(())
    }

    /// Coding-agent TUIs show a one-time directory-trust dialog for unknown
    /// folders — and every worktree is a fresh folder. Watch the pane and
    /// accept the (pre-selected "yes") dialog with Enter.
    ///
    /// The window is [`TRUST_WATCH`], and a single failed capture is no
    /// reason to stop watching — only the session going away is.
    /// What the trust dialog looks like in a pane, lowercased. Shared with
    /// the typed-input deliverer, which must not paste into that dialog.
    const TRUST_PATTERNS: [&'static str; 4] = [
        "do you trust",
        "trust this folder",
        "trust the contents",
        "press enter to continue",
    ];

    fn auto_accept_trust(&self, tmux_session: String) {
        let tmux = self.tmux.clone();
        tokio::spawn(async move {
            let deadline = std::time::Instant::now() + TRUST_WATCH;
            while std::time::Instant::now() < deadline {
                tokio::time::sleep(PANE_POLL).await;
                if !tmux.has_session(&tmux_session).await {
                    return;
                }
                let Ok(pane) = tmux.capture_pane(&tmux_session, 50).await else {
                    continue;
                };
                let lower = pane.to_lowercase();
                if Self::TRUST_PATTERNS.iter().any(|p| lower.contains(p)) {
                    tracing::info!(session = %tmux_session, "accepting directory-trust dialog");
                    let _ = tmux.send_enter(&tmux_session).await;
                    tokio::time::sleep(TRUST_ACCEPTED).await;
                }
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
                let lower = pane.to_lowercase();
                if lower.trim().is_empty() || Self::TRUST_PATTERNS.iter().any(|p| lower.contains(p))
                {
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
    /// the session was created with, off the pin its role carries, and no
    /// launch of that session ever moves either. Editing a profile is meant to
    /// steer the work defined after it, not to switch the model out from under
    /// a conversation already running.
    async fn spawn_ctx(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        system_prompt: String,
        initial_prompt: String,
    ) -> Result<SpawnCtx> {
        let agent = self.store.get_agent_config(session.agent_kind()).await?;
        Ok(SpawnCtx {
            session_id: session.id.clone(),
            goal_id: session.goal_id.clone(),
            task_id: session.task_id.clone(),
            role: session.role(),
            run_dir: self.run_dir(&session.id),
            cwd,
            socket_path: self.cfg.socket_path.clone(),
            cli_bin: self.cfg.cli_bin.clone(),
            system_prompt,
            initial_prompt,
            model: session.model.clone(),
            effort: session.effort.clone(),
            extra_flags: agent.extra_flags(),
        })
    }

    /// Shared launch tail for fresh spawns and resumes: persist the internal
    /// session id, start the tmux process, mark the session running and watch
    /// for the directory-trust dialog.
    async fn launch(&self, session: &AgentSession, plan: SpawnPlan) -> Result<()> {
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
        let spawn = self.tmux_spawn(session, plan.argv, env, plan.cwd)?;
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
        self.auto_accept_trust(session.tmux_session.clone());
        if let Some(input) = plan.post_launch_input {
            self.deliver_typed_input(session.id.clone(), session.tmux_session.clone(), input);
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
        system_prompt: String,
        initial_prompt: String,
    ) -> Result<()> {
        let ctx = self
            .spawn_ctx(session, cwd, system_prompt, initial_prompt)
            .await?;
        let plan = adapter_for(session.agent_kind()).plan_spawn(&ctx)?;
        self.launch(session, plan).await?;
        self.clear_superseded_attention(session).await;
        Ok(())
    }

    /// The session of `role` on this task there is something to resume, and the
    /// agent conversation it left behind: the most recent one with a captured
    /// internal id.
    ///
    /// Codex and opencode report theirs from a hook, so a session that never
    /// got going may have none, and that is nothing to resume — the caller
    /// spawns afresh instead. `profile_id` tells a task's reviewers apart,
    /// which is the only thing that does; every other role has one session per
    /// task.
    async fn resumable_session(
        &self,
        task_id: &str,
        role: Role,
        profile_id: Option<&str>,
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
                s.role() == role
                    && profile_id.is_none_or(|wanted| s.profile_id == wanted)
                    && s.internal_session_id.is_some()
            });
        Ok(found.map(|session| {
            let internal = session
                .internal_session_id
                .clone()
                .expect("filtered above");
            (session, internal)
        }))
    }

    /// The tail every resume shares, once the row has been put back on its
    /// feet: the agent launched again in `cwd`, on the conversation `internal`
    /// names, with `instruction` to come back to.
    async fn launch_resumed(
        &self,
        session: &AgentSession,
        profile: &Profile,
        cwd: PathBuf,
        internal: &str,
        instruction: &str,
    ) -> Result<AgentSession> {
        let ctx = self
            .spawn_ctx(session, cwd, prompts::system_prompt(profile), String::new())
            .await?;
        let plan = adapter_for(session.agent_kind()).plan_resume(&ctx, internal, instruction)?;
        self.launch(session, plan).await?;
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
    /// "Replaces" is the identity a spawn is refused for: the role on this
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
                && previous.role() == session.role()
                && (previous.role() != Role::Reviewer || previous.profile_id == session.profile_id)
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

    /// Spawn the planner for a goal (cwd = first repo).
    pub async fn spawn_planner(&self, goal_id: &str) -> Result<AgentSession> {
        let goal = self.store.get_goal(goal_id).await?;
        let repos = self.store.list_goal_repositories(goal_id).await?;
        let repo = repos.first().context("goal has no repos")?;
        let profile = self.store.get_profile(&goal.planner_profile_id).await?;
        self.assert_no_live_session(goal_id, None, Role::Planner, None)
            .await?;

        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: None,
                role: Role::Planner,
                profile_id: profile.id.clone(),
                agent_kind: self
                    .resolve_agent_kind(goal.agent_kind(), &format!("goal {}", goal.id))?,
                model: goal.model.clone(),
                effort: goal.effort.clone(),
                tmux_session: session_name(&goal.id, None, "planner", None),
                worktree_path: None,
                review_round: None,
            })
            .await?;

        let system = prompts::system_prompt(&profile);
        let template = prompts::template_for(PromptKind::PlannerBriefing);
        let briefing = prompts::planner_briefing(template, &goal, &repos);
        self.spawn(&session, PathBuf::from(&repo.path), system, briefing)
            .await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Spawn the engineer for a task: worktree + branch + session.
    pub async fn spawn_engineer(&self, task_id: &str) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        let profile = self.store.get_profile(&task.engineer_profile_id).await?;
        self.assert_no_live_session(&goal.id, Some(task_id), Role::Engineer, None)
            .await?;

        let worktree = self.engineer_worktree(&task, &repo, None).await?;

        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                role: Role::Engineer,
                profile_id: profile.id.clone(),
                agent_kind: self
                    .resolve_agent_kind(task.agent_kind(), &format!("task {}", task.id))?,
                model: task.model.clone(),
                effort: task.effort.clone(),
                tmux_session: session_name(&goal.id, Some(&task.id), "engineer", None),
                worktree_path: Some(worktree.display().to_string()),
                review_round: None,
            })
            .await?;

        // Re-read: worktree_path was just set.
        let task = self.store.get_task(task_id).await?;
        let mut deps = Vec::new();
        for dep_id in self.store.list_task_dependencies(&task.id).await? {
            deps.push(self.store.get_task(&dep_id).await?);
        }
        let system = prompts::system_prompt(&profile);
        let template = prompts::template_for(PromptKind::EngineerBriefing);
        let briefing = prompts::engineer_briefing(template, &task, &goal, &repo, &deps);
        self.spawn(&session, worktree, system, briefing).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// The engineer's worktree, checked out on the task branch: created on the
    /// first spawn, and created again whenever it has been cleaned up under a
    /// task that is still going. Nobody else ever holds the branch — the
    /// engineer keeps it from the first commit to the merge.
    ///
    /// `keep` is the tree a resumed engineer was working in — kept while it is
    /// still on disk, since an agent is put back where it left off rather than
    /// beside it. A fresh spawn passes `None` and gets the canonical path.
    async fn engineer_worktree(
        &self,
        task: &Task,
        repo: &Repository,
        keep: Option<PathBuf>,
    ) -> Result<PathBuf> {
        let worktree = match keep {
            Some(existing) if existing.is_dir() => existing,
            _ => self
                .cfg
                .worktree_root
                .join(tail(&task.goal_id))
                .join(format!("{}-eng", tail(&task.id))),
        };
        if !worktree.exists() {
            std::fs::create_dir_all(worktree.parent().unwrap())?;
            self.git
                .add_worktree(
                    &PathBuf::from(&repo.path),
                    &worktree,
                    &task.branch,
                    &repo.base_branch,
                )
                .await?;
        }
        self.store
            .set_task_worktree(&task.id, Some(&worktree.display().to_string()))
            .await?;
        // There is a tree to commit in now: follow what the branch does.
        self.branches.watch(task, Path::new(&repo.path));
        Ok(worktree)
    }

    /// The reviewer's detached worktree, pinned at the branch tip: created on
    /// the first round, re-pointed at the tip on every later one — the same
    /// worktree serves the whole review, as the same session does.
    async fn reviewer_worktree(&self, task: &Task, profile_id: &str) -> Result<PathBuf> {
        let worktree = self
            .cfg
            .worktree_root
            .join(tail(&task.goal_id))
            .join(format!("{}-rev-{}", tail(&task.id), tail(profile_id)));
        if worktree.exists() {
            // New round: refresh to the current branch tip.
            self.git.checkout_detached(&worktree, &task.branch).await?;
        } else {
            let repo = self.store.get_repository(&task.repo_id).await?;
            std::fs::create_dir_all(worktree.parent().unwrap())?;
            self.git
                .add_detached_worktree(&PathBuf::from(&repo.path), &worktree, &task.branch)
                .await?;
        }
        Ok(worktree)
    }

    /// Spawn one reviewer for a task (detached worktree at the branch tip).
    ///
    /// The session is not tied to the round it starts in: later rounds resume
    /// this very session (see [`Launcher::resume_reviewer`]), so its name says
    /// which reviewer of which task it is and nothing about when it began.
    pub async fn spawn_reviewer(&self, task_id: &str, profile_id: &str) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        let profile = self.store.get_profile(profile_id).await?;
        // The reviewer's slot on the task: both the proof that this profile
        // reviews it at all, and the agent and model it was assigned with.
        let slot = self
            .store
            .list_task_reviewer_pins(task_id)
            .await?
            .into_iter()
            .find(|r| r.profile_id == profile.id)
            .ok_or_else(|| {
                anyhow!(
                    "profile {} is not a reviewer of task {}",
                    profile.id,
                    task_id
                )
            })?;
        self.assert_no_live_session(&goal.id, Some(task_id), Role::Reviewer, Some(&profile.id))
            .await?;

        let worktree = self.reviewer_worktree(&task, &profile.id).await?;
        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                role: Role::Reviewer,
                profile_id: profile.id.clone(),
                agent_kind: self.resolve_agent_kind(
                    slot.agent_kind(),
                    &format!("reviewer {} of task {}", profile.id, task.id),
                )?,
                model: slot.model.clone(),
                effort: slot.effort.clone(),
                tmux_session: session_name(
                    &goal.id,
                    Some(&task.id),
                    "reviewer",
                    Some(tail(&profile.id)),
                ),
                worktree_path: Some(worktree.display().to_string()),
                review_round: Some(task.review_round),
            })
            .await?;

        let summary = self.store.review_summary(&task.id).await?;
        let system = prompts::system_prompt(&profile);
        let template = prompts::template_for(PromptKind::ReviewerBriefing);
        let briefing =
            prompts::reviewer_briefing(template, &task, &goal, &repo, summary.as_deref());
        self.spawn(&session, worktree, system, briefing).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
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
        profile_id: &str,
        instruction: &str,
    ) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let profile = self.store.get_profile(profile_id).await?;

        let Some((previous, internal)) = self
            .resumable_session(&task.id, Role::Reviewer, Some(&profile.id))
            .await?
        else {
            return self.spawn_reviewer(task_id, profile_id).await;
        };

        let worktree = self.reviewer_worktree(&task, &profile.id).await?;
        if self.tmux.has_session(&previous.tmux_session).await {
            self.tmux.kill_session(&previous.tmux_session).await.ok();
        }
        let session = self
            .store
            .restart_session(
                &previous.id,
                Some(&worktree.display().to_string()),
                Some(task.review_round),
            )
            .await?;

        self.launch_resumed(&session, &profile, worktree, &internal, instruction)
            .await
    }

    /// Resume the engineer's previous agent session with a new instruction,
    /// relaunching the very same session — row, id and tmux name — so a task
    /// bounced through several review rounds keeps one engineer session rather
    /// than one per round (spawn afresh if there is nothing to resume).
    pub async fn resume_engineer(&self, task_id: &str, instruction: &str) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let profile = self.store.get_profile(&task.engineer_profile_id).await?;

        let Some((previous, internal)) = self
            .resumable_session(&task.id, Role::Engineer, None)
            .await?
        else {
            return self.spawn_engineer(task_id).await;
        };
        // The tree it was working in, from the task or from the session's own
        // row, and a new one in its place where it is no longer on disk.
        let keep = task
            .worktree_path
            .clone()
            .or_else(|| previous.worktree_path.clone())
            .map(PathBuf::from);
        let repo = self.store.get_repository(&task.repo_id).await?;
        let worktree = self.engineer_worktree(&task, &repo, keep).await?;
        if self.tmux.has_session(&previous.tmux_session).await {
            self.tmux.kill_session(&previous.tmux_session).await.ok();
        }
        // Same conversation, same session: the row goes back to `starting` and
        // is launched again. Its console log is appended to rather than rolled
        // over, so the terminal reads as the one continuous transcript the
        // agent actually produced.
        let session = self
            .store
            .restart_session(&previous.id, Some(&worktree.display().to_string()), None)
            .await?;

        self.launch_resumed(&session, &profile, worktree, &internal, instruction)
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
        if self
            .tmux
            .has_session_or_unknown(&previous.tmux_session)
            .await
        {
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
        let profile = self.store.get_profile(&previous.profile_id).await?;
        let role = previous.role();

        let cwd = match role {
            Role::Planner => {
                let repos = self.store.list_goal_repositories(&previous.goal_id).await?;
                PathBuf::from(&repos.first().context("goal has no repos")?.path)
            }
            Role::Engineer | Role::Reviewer => PathBuf::from(
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
        let session = self.store.restart_session(&previous.id, None, None).await?;
        self.launch_resumed(
            &session,
            &profile,
            cwd,
            &internal,
            instruction.unwrap_or(""),
        )
        .await
    }

    /// Kill a session's tmux process and mark it exited.
    pub async fn kill_session(&self, session_id: &str) -> Result<()> {
        let session = self.store.get_session(session_id).await?;
        if self.tmux.has_session(&session.tmux_session).await {
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
            if !worth_following(&task) {
                continue;
            }
            match self.store.get_repository(&task.repo_id).await {
                Ok(repo) => self.branches.watch(&task, Path::new(&repo.path)),
                Err(e) => {
                    tracing::warn!(task = %task.id, error = %e, "cannot follow the task branch")
                }
            }
        }
        Ok(())
    }

    /// Cleanup after a merged/cancelled task: kill sessions, remove worktrees,
    /// optionally delete the branch.
    /// Idempotent: safe to call repeatedly on the same task.
    ///
    /// `remove_worktrees = false` keeps the worktrees on disk (and therefore
    /// also the branch — the engineer worktree has it checked out, which pins
    /// it) so merged or cancelled work can be inspected later.
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
        if delete_branch
            && task.status() == TaskStatus::Merged
            && self
                .git
                .branch_exists(&repo_path, &task.branch)
                .await
                .unwrap_or(false)
        {
            self.git.delete_branch(&repo_path, &task.branch).await.ok();
        }
        Ok(())
    }
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
