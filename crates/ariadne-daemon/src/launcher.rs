//! Launcher: turns "spawn an agent for X" into a worktree, a session row and
//! an ACP agent process.
//!
//! A launch is refused rather than duplicated. Every spawn asks first whether
//! the seat already has a live session: a wrong no puts two agents on one
//! piece of work, where a wrong yes costs a scheduler tick that asks again.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use ariadne_core::models::agent_of;
use ariadne_core::{PromptKind, Seat, SessionStatus, TaskStatus};
use ariadne_store::{
    AgentSession, NewSession, Repository, SessionFilter, Store, Task, TaskAgent, TaskFilter,
    author_branch,
};

use crate::acp::{AcpLaunch, AcpRuntime};
use crate::acp_discovery::AgentRegistry;
use crate::agents::{SpawnCtx, SpawnPlan, plan_resume, plan_spawn, prompts, write_skills};
use crate::branch::BranchWatchers;
use crate::config::Config;
use crate::gitwt::GitManager;

pub struct Launcher {
    pub cfg: Arc<Config>,
    pub store: Store,
    pub git: GitManager,
    /// The daemon-owned ACP agents: every session runs as a child process
    /// driven here.
    pub acp: AcpRuntime,
    /// The ACP agent registry: which executable a session's pin names, and
    /// what discovery measured about it.
    pub registry: AgentRegistry,
    /// The task branches whose head the daemon is following, so that a commit
    /// an author makes reaches the clients watching its diff.
    pub branches: BranchWatchers,
}

impl Launcher {
    fn run_dir(&self, session_id: &str) -> PathBuf {
        self.cfg.run_dir.join(session_id)
    }

    /// Refuse to double-spawn: one live session per (task, seat) —
    /// per (task, seat, agent) for reviewers, and for the authors of a task
    /// staffed with several.
    ///
    /// This is the last guard before a second agent starts working on
    /// somebody else's task.
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
                    "a live {} session already exists: {}",
                    seat.as_str(),
                    s.id
                ));
            }
        }
        Ok(())
    }

    /// Whether the agent process behind a session is alive: the runtime that
    /// owns the child answers ([`AcpRuntime::is_running`]), and it always
    /// answers.
    pub async fn session_process_alive(&self, session: &AgentSession) -> bool {
        self.acp.is_running(&session.id)
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
        let extra_flags = self.store.agent_flags(agent_of(&session.model)).await?;
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
        // The pin is `<agent>:<model>`; the agent is told only its own half.
        let model = session
            .model
            .split_once(':')
            .map_or(session.model.as_str(), |(_, model)| model)
            .to_string();
        Ok(SpawnCtx {
            session_id: session.id.clone(),
            // Minted here, once per launch: the row is told it in
            // [`Self::launch`], before the agent it belongs to exists.
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
            model,
            effort: session.effort.clone(),
            extra_flags,
        })
    }

    /// Shared launch tail for fresh spawns and resumes: persist the internal
    /// session id, and start the agent the session's pin names as the
    /// daemon's own child, driven over the protocol (`crate::acp`).
    ///
    /// What runs is the registry command of the agent the pin's first
    /// segment names, with the agent's configured flags behind it; a pin
    /// naming no registry agent fails the launch. A resume is gated on what
    /// discovery measured about that agent first.
    ///
    /// `launch_id` is what this run of the agent will report under, and the
    /// row is given it before the process starts. Both halves of that order
    /// matter on a relaunch, where one process is being torn down as another
    /// starts under the same session id: written any later, the new agent's
    /// first events would name a launch the row had not heard of; written any
    /// earlier, the old agent's last ones would still count.
    async fn launch(&self, session: &AgentSession, plan: SpawnPlan, launch_id: &str) -> Result<()> {
        if let Some(internal) = &plan.internal_session_id {
            self.store
                .set_session_internal_id(&session.id, internal)
                .await?;
        }
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
                    .context("the session's goal has no repository")?;
                (repo.id, self.cfg.permission_mode)
            }
        };
        let agent_id = agent_of(&session.model);
        let command = self.registry.command_of(agent_id).with_context(|| {
            format!(
                "session {} is pinned to `{}`, and the ACP registry holds no agent `{agent_id}`",
                session.id, session.model
            )
        })?;
        if plan.config.resume_session_id.is_some() {
            self.assert_acp_session_resumable(session, agent_id).await?;
        }
        let mut command = command.into_iter();
        let program = command.next().context("the registry command is empty")?;
        let args = command.chain(plan.args).collect();
        self.store
            .set_session_launch(&session.id, launch_id)
            .await?;
        self.acp
            .launch(AcpLaunch {
                session_id: session.id.clone(),
                launch_id: launch_id.to_string(),
                program,
                args,
                env: plan.env,
                cwd: plan.cwd,
                config: plan.config,
                repository_id,
                permission_mode,
            })
            .await
            .context("spawning the ACP agent")?;
        // Stamped before the status, so that a session seen `running` is
        // always a session whose launch is dated: the scheduler measures a
        // resumed agent's silence from here, and a launch it cannot date is a
        // launch it cannot watch.
        self.store.mark_session_launched(&session.id).await?;
        // Running, but only over a row still starting: the driver reports
        // from the moment it spawns, and an agent that failed fast has ended
        // the row by now — Running written over that would resurrect it until
        // the sweep retires it again.
        if self.store.get_session(&session.id).await?.status() == SessionStatus::Starting {
            self.store
                .set_session_status(&session.id, SessionStatus::Running)
                .await?;
        }
        Ok(())
    }

    /// Refuse to resume a session of an agent that cannot load one.
    ///
    /// Discovery measures whether an agent supports `session/load`
    /// (`session_load`), and an agent without it has no way back into a
    /// stored conversation: a launch that went ahead would open a fresh
    /// session and call it the old one. The refusal names the session and
    /// the agent, which is what the caller reports.
    async fn assert_acp_session_resumable(
        &self,
        session: &AgentSession,
        agent_id: &str,
    ) -> Result<()> {
        let resumable = self
            .registry
            .capabilities_of(agent_id)
            .await
            .is_some_and(|capabilities| capabilities.session_load);
        if !resumable {
            anyhow::bail!(
                "session {} is not resumable: ACP agent {agent_id} does not support session/load",
                session.id
            );
        }
        Ok(())
    }

    async fn spawn(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        initial_prompt: String,
    ) -> Result<()> {
        let ctx = self.spawn_ctx(session, cwd, initial_prompt).await?;
        let plan = plan_spawn(&ctx)?;
        self.launch(session, plan, &ctx.launch_id).await?;
        self.clear_superseded_attention(session).await;
        Ok(())
    }

    /// The session of `seat` on this task there is something to resume, and the
    /// agent conversation it left behind: the most recent one with a captured
    /// internal id.
    ///
    /// An agent reports its own at `session_start`, so a session that never
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
        let plan = plan_resume(&ctx, internal, instruction)?;
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

        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: None,
                seat: Seat::Orchestrator,
                task_agent_id: None,
                model: goal.model.clone(),
                effort: goal.effort.clone(),
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

        let worktree = self.author_worktree(&task, &repo, None, &seat).await?;

        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                seat: Seat::Author,
                task_agent_id: Some(seat.author.id.clone()),
                model: seat.author.model.clone(),
                effort: seat.author.effort.clone(),
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

    /// Give a ready task an author session that continues a stored ACP
    /// session in the task's worktree, through `session/load`.
    ///
    /// The adopted conversation becomes the task's first author. A task
    /// staffed with several authors adopts into that seat alone; its
    /// siblings are spawned by the scheduler as usual.
    pub async fn adopt_author(
        &self,
        task_id: &str,
        agent_id: &str,
        internal_session_id: &str,
    ) -> Result<AgentSession> {
        let task = self.store.get_task(task_id).await?;
        if task.status() != TaskStatus::Ready {
            anyhow::bail!("task {task_id} is {}, not ready", task.status);
        }
        let goal = self.store.get_goal(&task.goal_id).await?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        let author = self.store.task_author(task_id).await?;
        // Which agent this is is part of the pin, `<agent>:<model>`, so the
        // outside session's agent has to be the one the author is pinned to.
        let pinned = agent_of(&author.model);
        if agent_id != pinned {
            anyhow::bail!(
                "outside session belongs to agent {agent_id}, but task author uses {pinned}"
            );
        }
        let seat = self.author_seat(&task, &author.id).await?;
        let guard = seat.sibling_tail().map(|_| author.id.clone());
        self.assert_no_live_session(&goal.id, Some(task_id), Seat::Author, guard.as_deref())
            .await?;
        let worktree = self.author_worktree(&task, &repo, None, &seat).await?;
        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                seat: Seat::Author,
                task_agent_id: Some(author.id.clone()),
                model: author.model.clone(),
                effort: author.effort.clone(),
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
                model: reviewer.model.clone(),
                effort: reviewer.effort.clone(),
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
    /// this is the same re-point for an agent that survived the last review —
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
    /// round, relaunching the very same session — row and id — so
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
        // An agent still up under the row is taken down by the launch that
        // replaces it: one session, one agent.
        let session = self
            .store
            .restart_session(&previous.id, Some(&worktree.display().to_string()))
            .await?;

        self.launch_resumed(&session, worktree, &internal, instruction)
            .await
    }

    /// Resume the author's previous agent session with a new instruction,
    /// relaunching the very same session — row and id — so a task
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
        // Same conversation, same session: the row goes back to `starting` and
        // is launched again, and an agent still up under it is taken down by
        // that launch. Its events carry on in the one record, so the console
        // reads as the one continuous transcript the agent actually produced.
        let session = self
            .store
            .restart_session(&previous.id, Some(&worktree.display().to_string()))
            .await?;

        self.launch_resumed(&session, worktree, &internal, instruction)
            .await
    }

    /// Revive an ended session in a fresh agent process, continuing the same
    /// agent conversation via its stored internal id. The session itself is
    /// revived — same row, same id — so the caller gets back what it asked
    /// for. Used by `ariadne attach` when no agent is alive.
    /// `instruction: None` resumes into an idle agent so the user can type
    /// themselves.
    pub async fn revive_session(
        &self,
        session_id: &str,
        instruction: Option<&str>,
    ) -> Result<AgentSession> {
        let previous = self.store.get_session(session_id).await?;
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

    /// Kill a session's agent process — the daemon-owned child — and mark
    /// the session exited.
    pub async fn kill_session(&self, session_id: &str) -> Result<()> {
        let session = self.store.get_session(session_id).await?;
        self.acp.kill(&session.id);
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
            if self.acp.is_running(&session.id) {
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
/// The lone author is the shape everything always had — the task branch and
/// the `-eng` worktree — so `sibling_tail()` is what
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

/// Short display form of a ULID, for the paths a session's worktree is cut
/// at: ULIDs are long, and the trailing 8 chars are the distinctive random
/// part.
fn tail(id: &str) -> &str {
    &id[id.len().saturating_sub(8)..]
}
