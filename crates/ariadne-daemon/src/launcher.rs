//! Launcher: turns "spawn an agent for X" into worktree + session row + tmux
//! process. Used by the debug spawn endpoint now and by the scheduler loop
//! once autonomous orchestration lands.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use ariadne_core::{Role, SessionStatus, TaskStatus};
use ariadne_store::{AgentSession, NewSession, SessionFilter, Store};

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
                && self.tmux.has_session(&s.tmux_session).await
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
    /// folders — and every worktree is a fresh folder. Watch the pane for a
    /// short window and accept the (pre-selected "yes") dialog with Enter.
    fn auto_accept_trust(&self, tmux_session: String) {
        const PATTERNS: [&str; 4] = [
            "do you trust",
            "trust this folder",
            "trust the contents",
            "press enter to continue",
        ];
        let tmux = self.tmux.clone();
        tokio::spawn(async move {
            for _ in 0..40 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if !tmux.has_session(&tmux_session).await {
                    return;
                }
                let Ok(pane) = tmux.capture_pane(&tmux_session, 50).await else {
                    return;
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
    fn spawn_ctx(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        profile: &ariadne_store::Profile,
        system_prompt: String,
        initial_prompt: String,
    ) -> SpawnCtx {
        SpawnCtx {
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
            extra_flags: profile.extra_flags(),
        }
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
        self.tmux
            .new_session(&TmuxSpawn {
                session: session.tmux_session.clone(),
                cwd: plan.cwd,
                env,
                argv: plan.argv,
                log_file: Some(self.run_dir(&session.id).join("console.log")),
            })
            .await
            .context("spawning tmux session")?;
        self.store
            .set_session_status(&session.id, SessionStatus::Running)
            .await?;
        self.auto_accept_trust(session.tmux_session.clone());
        Ok(())
    }

    async fn spawn(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        system_prompt: String,
        initial_prompt: String,
    ) -> Result<()> {
        let profile = self.store.get_profile(&session.profile_id).await?;
        let ctx = self.spawn_ctx(session, cwd, &profile, system_prompt, initial_prompt);
        let plan = adapter_for(session.agent_kind()).plan_spawn(&ctx)?;
        self.launch(session, plan).await
    }

    /// Spawn the planner for a goal (cwd = first repo).
    pub async fn spawn_planner(&self, goal_id: &str) -> Result<AgentSession> {
        let goal = self.store.get_goal(goal_id).await?;
        let repos = self.store.list_goal_repos(goal_id).await?;
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
                tmux_session: session_name(&goal.id, None, "planner", None),
                worktree_path: None,
                review_round: None,
            })
            .await?;

        let system = prompts::system_prompt(&profile, Role::Planner);
        let briefing = prompts::planner_briefing(&goal, &repos);
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
        let repo = self.store.get_goal_repo(&task.repo_id).await?;
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
        let system = prompts::system_prompt(&profile, Role::Engineer);
        let briefing = prompts::engineer_briefing(&task, &goal, &repo, &deps);
        self.spawn(&session, worktree, system, briefing).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Spawn one reviewer for a task's current round (detached worktree at
    /// the branch tip).
    pub async fn spawn_reviewer(&self, task_id: &str, profile_id: &str) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        let repo = self.store.get_goal_repo(&task.repo_id).await?;
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

        let worktree = self.cfg.worktree_root.join(tail(&goal.id)).join(format!(
            "{}-rev-{}",
            tail(&task.id),
            tail(&profile.id)
        ));
        let repo_path = PathBuf::from(&repo.path);
        if worktree.exists() {
            // New round: refresh to the current branch tip.
            self.git.checkout_detached(&worktree, &task.branch).await?;
        } else {
            std::fs::create_dir_all(worktree.parent().unwrap())?;
            self.git
                .add_detached_worktree(&repo_path, &worktree, &task.branch)
                .await?;
        }

        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                role: Role::Reviewer,
                profile_id: profile.id.clone(),
                agent_kind: self.resolve_agent_kind(&profile)?,
                tmux_session: session_name(
                    &goal.id,
                    Some(&task.id),
                    &format!("rev{}", tail(&profile.id)),
                    Some(task.review_round),
                ),
                worktree_path: Some(worktree.display().to_string()),
                review_round: Some(task.review_round),
            })
            .await?;

        // The engineer's review-request summary is the latest message, if any.
        let summary = self
            .store
            .list_task_messages(&task.id, None, 200)
            .await?
            .into_iter()
            .rev()
            .find(|m| m.author_role() == ariadne_core::AuthorRole::Engineer)
            .map(|m| m.body);
        let system = prompts::system_prompt(&profile, Role::Reviewer);
        let briefing = prompts::reviewer_briefing(&task, &goal, &repo, summary.as_deref());
        self.spawn(&session, worktree, system, briefing).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Resume the engineer's previous agent session with a new instruction,
    /// reusing the same tmux session name (spawn again if nothing to resume).
    pub async fn resume_engineer(&self, task_id: &str, instruction: &str) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
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
        if previous.status().is_live() {
            self.store
                .set_session_status(&previous.id, SessionStatus::Exited)
                .await?;
        }

        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                role: Role::Engineer,
                profile_id: profile.id.clone(),
                agent_kind: previous.agent_kind(),
                tmux_session: previous.tmux_session.clone(),
                worktree_path: Some(worktree.display().to_string()),
                review_round: None,
            })
            .await?;

        let ctx = self.spawn_ctx(
            &session,
            worktree,
            &profile,
            prompts::system_prompt(&profile, Role::Engineer),
            String::new(),
        );
        let plan = adapter_for(previous.agent_kind()).plan_resume(&ctx, &internal, instruction)?;
        self.launch(&session, plan).await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Revive an ended session in a fresh tmux, continuing the same agent
    /// conversation via its stored internal id. Used by `ariadne attach` when
    /// no tmux is alive. `instruction: None` resumes into an idle TUI so the
    /// user can type themselves.
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
                let repos = self.store.list_goal_repos(&previous.goal_id).await?;
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

        if previous.status().is_live() {
            self.store
                .set_session_status(&previous.id, SessionStatus::Exited)
                .await?;
        }
        let session = self
            .store
            .create_session(NewSession {
                goal_id: previous.goal_id.clone(),
                task_id: previous.task_id.clone(),
                role,
                profile_id: profile.id.clone(),
                agent_kind: previous.agent_kind(),
                tmux_session: previous.tmux_session.clone(),
                worktree_path: previous.worktree_path.clone(),
                review_round: previous.review_round,
            })
            .await?;
        let ctx = self.spawn_ctx(
            &session,
            cwd,
            &profile,
            prompts::system_prompt(&profile, role),
            String::new(),
        );
        let plan = adapter_for(previous.agent_kind()).plan_resume(
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
        let repo = self.store.get_goal_repo(&task.repo_id).await?;
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
