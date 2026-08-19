//! Launcher: turns "spawn an agent for X" into worktree + session row + tmux
//! process. Used by the debug spawn endpoint now and by the scheduler loop
//! once autonomous orchestration lands.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use ariadne_core::spawn_plan::SpawnPlanFile;
use ariadne_core::{PromptKind, Role, SessionStatus, TaskStatus};
use ariadne_store::{AgentSession, NewSession, SessionFilter, Store, Task};

use crate::agents::{SpawnCtx, SpawnPlan, adapter_for, detect_first_available, prompts};
use crate::config::Config;
use crate::gitwt::GitManager;
use crate::tmux::{TmuxManager, TmuxSpawn, session_name, tail};

pub struct Launcher {
    pub cfg: Arc<Config>,
    pub store: Store,
    pub tmux: TmuxManager,
    pub git: GitManager,
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

    /// Agent kind for a profile: explicit, or the first installed CLI
    /// (claude_code, then codex, then opencode) for auto profiles.
    fn resolve_agent_kind(
        &self,
        profile: &ariadne_store::Profile,
    ) -> Result<ariadne_core::AgentKind> {
        match profile.agent_kind() {
            Some(kind) => Ok(kind),
            None => detect_first_available().ok_or_else(|| {
                anyhow!(
                    "profile {} has no agent kind and no coding agent CLI (claude, codex, opencode) was found on PATH",
                    profile.name
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
    /// The window is generous (two minutes): a slow CLI start renders the
    /// dialog well after spawn, and a watcher that has already given up leaves
    /// the agent waiting on it forever. A single failed capture is likewise no
    /// reason to stop watching — only the session going away is.
    fn auto_accept_trust(&self, tmux_session: String) {
        const PATTERNS: [&str; 4] = [
            "do you trust",
            "trust this folder",
            "trust the contents",
            "press enter to continue",
        ];
        let tmux = self.tmux.clone();
        tokio::spawn(async move {
            for _ in 0..240 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if !tmux.has_session(&tmux_session).await {
                    return;
                }
                let Ok(pane) = tmux.capture_pane(&tmux_session, 50).await else {
                    continue;
                };
                let lower = pane.to_lowercase();
                if PATTERNS.iter().any(|p| lower.contains(p)) {
                    tracing::info!(session = %tmux_session, "accepting directory-trust dialog");
                    let _ = tmux.send_enter(&tmux_session).await;
                    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                }
            }
        });
    }

    /// Assemble the adapter context for launching `session` in `cwd`.
    ///
    /// The agent's flags are read here rather than baked into the adapters, on
    /// every spawn and every resume alike: an edit to the agent config is meant
    /// to reach the next launch, whichever path that launch comes down.
    ///
    /// The model the adapter is handed is written back onto the session row
    /// for the same reason, and because that is the only moment it is known:
    /// a resume after a profile edit launches with the model in effect now,
    /// and the row is what says which one that was.
    async fn spawn_ctx(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        profile: &ariadne_store::Profile,
        system_prompt: String,
        initial_prompt: String,
    ) -> Result<SpawnCtx> {
        let agent = self.store.get_agent_config(session.agent_kind()).await?;
        if session.model != profile.model {
            self.store
                .set_session_model(&session.id, profile.model.as_deref())
                .await?;
        }
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
            model: profile.model.clone(),
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
        self.store
            .set_session_status(&session.id, SessionStatus::Running)
            .await?;
        self.auto_accept_trust(session.tmux_session.clone());
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
            if !is_executable(Path::new(&bin)) {
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
        let profile = self.store.get_profile(&session.profile_id).await?;
        let ctx = self
            .spawn_ctx(session, cwd, &profile, system_prompt, initial_prompt)
            .await?;
        let plan = adapter_for(session.agent_kind()).plan_spawn(&ctx)?;
        self.launch(session, plan).await
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
                agent_kind: self.resolve_agent_kind(&profile)?,
                model: profile.model.clone(),
                tmux_session: session_name(&goal.id, None, "planner", None),
                worktree_path: None,
                review_round: None,
            })
            .await?;

        let system = prompts::system_prompt(&profile);
        let template =
            prompts::template_for(&self.store, &profile.id, PromptKind::PlannerBriefing).await;
        let briefing = prompts::planner_briefing(&template, &goal, &repos);
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

        let worktree = self
            .cfg
            .worktree_root
            .join(tail(&goal.id))
            .join(format!("{}-eng", tail(&task.id)));
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

        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                role: Role::Engineer,
                profile_id: profile.id.clone(),
                agent_kind: self.resolve_agent_kind(&profile)?,
                model: profile.model.clone(),
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
        let template =
            prompts::template_for(&self.store, &profile.id, PromptKind::EngineerBriefing).await;
        let briefing = prompts::engineer_briefing(&template, &task, &goal, &repo, &deps);
        self.spawn(&session, worktree, system, briefing).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
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

    /// The engineer's review-request summary: the latest message it wrote, if
    /// any.
    pub(crate) async fn engineer_summary(&self, task_id: &str) -> Result<Option<String>> {
        Ok(self
            .store
            .list_task_messages(task_id, None, 200)
            .await?
            .into_iter()
            .rev()
            .find(|m| m.author_role() == ariadne_core::AuthorRole::Engineer)
            .map(|m| m.body))
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
        if !self
            .store
            .list_task_reviewers(task_id)
            .await?
            .contains(&profile.id)
        {
            return Err(anyhow!(
                "profile {} is not a reviewer of task {}",
                profile.id,
                task_id
            ));
        }
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
                agent_kind: self.resolve_agent_kind(&profile)?,
                model: profile.model.clone(),
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

        let summary = self.engineer_summary(&task.id).await?;
        let system = prompts::system_prompt(&profile);
        let template =
            prompts::template_for(&self.store, &profile.id, PromptKind::ReviewerBriefing).await;
        let briefing =
            prompts::reviewer_briefing(&template, &task, &goal, &repo, summary.as_deref());
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

        // Find this reviewer's most recent session with a captured internal id
        // (codex and opencode report theirs from a hook, so a session that
        // never got going may have none — that is nothing to resume).
        let previous = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await?
            .into_iter()
            .rev()
            .find(|s| {
                s.role() == Role::Reviewer
                    && s.profile_id == profile.id
                    && s.internal_session_id.is_some()
            });
        let Some(previous) = previous else {
            return self.spawn_reviewer(task_id, profile_id).await;
        };
        let internal = previous
            .internal_session_id
            .clone()
            .expect("filtered above");

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

        let ctx = self
            .spawn_ctx(
                &session,
                worktree,
                &profile,
                prompts::system_prompt(&profile),
                String::new(),
            )
            .await?;
        let plan = adapter_for(session.agent_kind()).plan_resume(&ctx, &internal, instruction)?;
        self.launch(&session, plan).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Resume the engineer's previous agent session with a new instruction,
    /// relaunching the very same session — row, id and tmux name — so a task
    /// bounced through several review rounds keeps one engineer session rather
    /// than one per round (spawn afresh if there is nothing to resume).
    pub async fn resume_engineer(&self, task_id: &str, instruction: &str) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let profile = self.store.get_profile(&task.engineer_profile_id).await?;

        // Find the most recent engineer session with a captured internal id.
        let previous = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await?
            .into_iter()
            .rev()
            .find(|s| s.role() == Role::Engineer && s.internal_session_id.is_some());
        let Some(previous) = previous else {
            return self.spawn_engineer(task_id).await;
        };
        let internal = previous
            .internal_session_id
            .clone()
            .expect("filtered above");
        let worktree = PathBuf::from(
            task.worktree_path
                .clone()
                .or(previous.worktree_path.clone())
                .context("engineer session has no worktree")?,
        );
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

        let ctx = self
            .spawn_ctx(
                &session,
                worktree,
                &profile,
                prompts::system_prompt(&profile),
                String::new(),
            )
            .await?;
        let plan = adapter_for(session.agent_kind()).plan_resume(&ctx, &internal, instruction)?;
        self.launch(&session, plan).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
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
        if self.tmux.has_session(&previous.tmux_session).await {
            // Already alive — attaching needs nothing from us.
            return Ok(previous);
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
        let ctx = self
            .spawn_ctx(
                &session,
                cwd,
                &profile,
                prompts::system_prompt(&profile),
                String::new(),
            )
            .await?;
        let plan = adapter_for(session.agent_kind()).plan_resume(
            &ctx,
            &internal,
            instruction.unwrap_or(""),
        )?;
        self.launch(&session, plan).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
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

/// A file this daemon could exec: present, a file, and with an execute bit.
/// (`http::doctor` asks the same question to *report* on a binary; here it
/// decides whether a spawn happens at all.)
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // Follows symlinks on purpose: what matters is what running it reaches.
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Whether a bare name is an executable on the daemon's own `PATH`.
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| is_executable(&dir.join(name))))
}
