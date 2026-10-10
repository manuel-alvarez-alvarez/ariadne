//! Store entity -> API DTO conversions.
//!
//! Every one of them is the same shape: most fields come across unchanged,
//! and a few do not — a column the store keeps as text and the API as an
//! enum, a derived flag, a name the caller loaded. Only the second kind is
//! worth reading, so [`dto!`] is what writes the first.

use std::collections::HashMap;

use ariadne_api::events::AgentEventDto;
use ariadne_api::goals::{GoalDto, GoalUsageDto};
use ariadne_api::messages::MessageDto;
use ariadne_api::permissions::{
    LearnedPermissionDto, LearnedPermissionLevel, LearnedPermissionScope, LearnedPermissionTarget,
};
use ariadne_api::repositories::{ForgeDto, RepositoryDto};
use ariadne_api::sessions::{OutsideSessionDto, SessionDto, SessionEntryDto, SessionKind};
use ariadne_api::skills::{SkillDto, SkillSeat};
use ariadne_api::tasks::{AgentUsageDto, TaskAgentDto, TaskDto, TaskTransitionDto, TaskUsageDto};
use ariadne_api::usage::TokenUsageDto;
use ariadne_api::workflows::{WorkflowDto, WorkflowStepDto};
use ariadne_core::models::agent_of;
use ariadne_core::workflow::WorkflowStep;
use ariadne_core::{Actor, MessageKind, Seat, TokenUsage};
use ariadne_store::{self as store, AgentUsage, SessionFilter, Store, StoreError};

/// One conversion per entity: the fields that are not a straight move, then
/// `..` and the ones that are.
///
/// The computed fields are written out first in the struct literal as well as
/// in the macro, because several of them borrow the row (`row.status()`) and
/// a field moved out of it first would have left it partially moved.
macro_rules! dto {
    ($(
        $(#[$doc:meta])*
        $vis:vis fn $name:ident($row:ident: $src:ty $(, $arg:ident: $arg_ty:ty)* $(,)?) -> $dst:ident {
            $($computed:ident: $expr:expr,)*
            .. $($moved:ident),* $(,)?
        }
    )*) => { $(
        $(#[$doc])*
        $vis fn $name($row: $src $(, $arg: $arg_ty)*) -> $dst {
            $dst {
                $($computed: $expr,)*
                $($moved: $row.$moved,)*
            }
        }
    )* };
}

dto! {
    pub(crate) fn skill_dto(s: store::Skill) -> SkillDto {
        seat: match s.seat() {
            store::SkillSeat::Orchestrator => SkillSeat::Orchestrator,
            store::SkillSeat::Task => SkillSeat::Task,
            store::SkillSeat::PullRequest => SkillSeat::PullRequest,
        },
        summary: s.summary().to_string(),
        document: s.document_text().to_string(),
        document_is_default: s.document_is_default(),
        builtin: s.is_builtin(),
        .. name, created_at, updated_at
    }

    pub(crate) fn workflow_dto(w: store::Workflow) -> WorkflowDto {
        document: w.document_text().to_string(),
        builtin: w.is_builtin(),
        steps: w.steps().into_iter().map(workflow_step_dto).collect(),
        .. name, created_at, updated_at
    }

    pub(crate) fn workflow_step_dto(s: WorkflowStep) -> WorkflowStepDto {
        .. id, title, description, skills, rank, gate
    }

    pub(crate) fn repository_dto(r: store::Repository) -> RepositoryDto {
        permission_mode: r.permission_mode(),
        forge: r.forge.map(forge_dto),
        .. id, path, base_branch, description, default_workflow, created_at, updated_at
    }

    fn forge_dto(f: store::ForgeIntegration) -> ForgeDto {
        kind: f.kind(),
        webhook: ariadne_api::repositories::WebhookDto {
            state: f.webhook_state,
            url: f.webhook_url,
            error: f.webhook_error,
            last_delivery_at: f.webhook_last_delivery_at,
            fetch_error: f.fetch_error,
        },
        .. host, owner, name, remote, enabled, login, review_model, review_effort
    }

    /// `repos` are the goal's repositories, `usage` its rollup and `steps`
    /// the columns it snapshotted, all of which the caller loads.
    fn goal_dto(
        g: store::Goal,
        repos: Vec<RepositoryDto>,
        usage: GoalUsageDto,
        steps: Vec<WorkflowStepDto>,
    ) -> GoalDto {
        status: g.status(),
        steps: steps,
        repos: repos,
        usage: usage,
        .. id, title, description, issue_url, orchestrated, model, effort, workflow,
           created_at, updated_at
    }

    /// `skills` is the agent's skill names in load order, which the caller
    /// loads beside the row; `session_id` is this agent's live session, which
    /// the caller finds among the task's.
    fn task_agent_dto(
        a: store::TaskAgent,
        skills: Vec<String>,
        session_id: Option<String>,
    ) -> TaskAgentDto {
        skills: skills,
        session_id: session_id,
        .. id, model, effort, brief, step
    }

    /// The agents come from the caller, which loads them with their skills,
    /// in the order the orchestrator listed them. So does `reason`, which only
    /// an ended task has. `sessions` is the task's own, which each agent's
    /// live one is found among.
    fn task_dto(
        t: store::Task,
        agents: Vec<(store::TaskAgent, Vec<String>)>,
        sessions: Vec<store::AgentSession>,
        depends_on: Vec<String>,
        usage: TaskUsageDto,
        reason: Option<String>,
    ) -> TaskDto {
        status: t.status(),
        stalled: t.is_stalled(),
        agents: agents
            .into_iter()
            .map(|(a, skills)| {
                let session_id = live_session_id(&a.id, &sessions);
                task_agent_dto(a, skills, session_id)
            })
            .collect(),
        depends_on: depends_on,
        usage: usage,
        reason: reason,
        .. id, goal_id, repo_id, title, description, branch, worktree_path, step,
           merge_commit, pr_url, created_at, updated_at
    }

    pub(crate) fn transition_dto(t: store::TaskTransition) -> TaskTransitionDto {
        .. id, from_status, to_status, actor, reason, created_at, from_step, to_step
    }

    pub(crate) fn message_dto(m: store::Message) -> MessageDto {
        // A kind or an actor this build does not know is carried rather than
        // dropped: the body is what somebody typed, and a listing that
        // silently loses a message is worse than one that shows a `note`.
        kind: m.kind().unwrap_or(MessageKind::Message),
        from_actor: m.from_actor().unwrap_or(Actor::Daemon),
        to_actor: m.to_actor().unwrap_or(Actor::Daemon),
        .. id, goal_id, task_id, from_agent_id, from_session, to_agent_id,
           body, delivered_at, created_at
    }

    /// `usage` is what this session has spent, which the caller loads.
    fn session_dto(s: store::AgentSession, usage: TokenUsageDto) -> SessionDto {
        seat: s.seat(),
        status: s.status(),
        attention_reason: s.attention_reason(),
        usage: usage,
        context_used: s.context_used.and_then(|value| u64::try_from(value).ok()),
        context_size: s.context_size.and_then(|value| u64::try_from(value).ok()),
        .. id, goal_id, task_id, task_agent_id, model, effort, internal_session_id,
           worktree_path, attention_since,
           last_activity_at, created_at, ended_at, title, switched_from, pull_request_id
    }
}

pub(crate) fn learned_permission_dto(row: store::LearnedPermission) -> LearnedPermissionDto {
    let json = |text: &str| serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    LearnedPermissionDto {
        tool_call: json(&row.tool_call),
        options: json(&row.options),
        target: match row.target.as_str() {
            "auto" => LearnedPermissionTarget::Auto,
            "learn" => LearnedPermissionTarget::Learn,
            "ai" => LearnedPermissionTarget::Ai,
            _ => LearnedPermissionTarget::Ask,
        },
        output: row.output.as_deref().map(json),
        level: match row.level.as_str() {
            "once" => LearnedPermissionLevel::Once,
            "family" => LearnedPermissionLevel::Family,
            _ => LearnedPermissionLevel::Command,
        },
        scope: match row.scope.as_str() {
            "all" => LearnedPermissionScope::All,
            _ => LearnedPermissionScope::Repository,
        },
        risk_tags: serde_json::from_str(&row.risk_tags).unwrap_or_default(),
        key: row.key,
        family: row.family,
        id: row.id,
        repository_id: row.repository_id,
        tool_name: row.tool_name,
        selected_option: row.selected_option,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// Not in the [`dto!`] block above: `summary` is built from both `kind` and
/// `payload` together, which the macro's one-expression-per-field shape has
/// no room for.
pub(crate) fn event_dto(e: store::AgentEvent) -> AgentEventDto {
    let payload: serde_json::Value =
        serde_json::from_str(&e.payload).unwrap_or(serde_json::Value::Null);
    let summary = super::classify::summarize(&e.kind, &payload);
    AgentEventDto {
        id: e.id,
        session_id: e.session_id,
        task_id: e.task_id,
        kind: e.kind,
        payload,
        summary,
        created_at: e.created_at,
    }
}

/// The names of the skills an agent loads, in the order they reach it.
///
/// An agent has no name of its own, so this is what a reader identifies it
/// by: an agent on `coding` and `testing` is read as exactly that.
async fn agent_skills(store: &Store, agent_id: &str) -> Vec<String> {
    store
        .agent_skills(agent_id)
        .await
        .map(|skills| skills.into_iter().map(|s| s.name).collect())
        .unwrap_or_default()
}

/// This agent's live session, so the orchestrator finds a session
/// `switch_session` can act on. An agent between runs carries none.
fn live_session_id(agent_id: &str, sessions: &[store::AgentSession]) -> Option<String> {
    sessions
        .iter()
        .find(|s| s.task_agent_id.as_deref() == Some(agent_id) && s.status().is_live())
        .map(|s| s.id.clone())
}

/// [`task_dto`] with everything it needs loaded from the store: the agents
/// staffed on the task with the skills each one loads, the dependencies, and
/// what has been spent.
///
/// The skills beside every agent are what a task is read for: an agent is its
/// skills, and no prompt can teach a reader to read an id.
pub(crate) async fn task_dto_of(store: &Store, task: store::Task) -> Result<TaskDto, StoreError> {
    let mut agents = Vec::new();
    for agent in store.list_task_agents(&task.id).await? {
        let skills = agent_skills(store, &agent.id).await;
        agents.push((agent, skills));
    }
    let sessions = store
        .list_sessions(SessionFilter {
            task_id: Some(task.id.clone()),
            ..Default::default()
        })
        .await?;
    let depends_on = store.list_task_dependencies(&task.id).await?;
    let usage = task_usage(store, &task.id, &agents).await?;
    let reason = store.ended_reason(&task).await?;
    Ok(task_dto(task, agents, sessions, depends_on, usage, reason))
}

/// [`session_dto`] with what the session has spent loaded from the store.
pub(crate) async fn session_dto_of(
    store: &Store,
    session: store::AgentSession,
) -> Result<SessionDto, StoreError> {
    let usage = store.session_usage(&session.id).await?;
    Ok(session_dto(session, usage.into()))
}

/// One row of the session listing: the session as its own endpoint answers
/// it, under the title of the work behind it, which the row does not hold.
pub(crate) async fn session_entry_of(
    store: &Store,
    session: store::AgentSession,
    title: Option<String>,
) -> Result<SessionEntryDto, StoreError> {
    let mut session = session_dto_of(store, session).await?;
    let title = title.or(session.title.take());
    Ok(SessionEntryDto {
        kind: SessionKind::Ariadne,
        agent_id: agent_of(&session.model).to_string(),
        title,
        working_directory: session.worktree_path,
        status: Some(session.status),
        usage: Some(session.usage),
        created_at: Some(session.created_at),
        id: session.id,
        goal_id: session.goal_id,
        task_id: session.task_id,
        seat: session.seat,
        task_agent_id: session.task_agent_id,
        model: Some(session.model),
        effort: session.effort,
        internal_session_id: session.internal_session_id,
        attention_reason: session.attention_reason,
        attention_since: session.attention_since,
        last_activity_at: session.last_activity_at,
        context_used: session.context_used,
        context_size: session.context_size,
        ended_at: session.ended_at,
        pull_request_id: session.pull_request_id,
    })
}

/// The same row for a conversation Ariadne did not start: the agent, the id
/// it loads back by, where it ran and when, and its first prompt as its
/// title. It has no goal, task, seat or status, because Ariadne runs no work
/// behind it.
pub(crate) fn outside_entry(outside: &OutsideSessionDto) -> SessionEntryDto {
    let text = |value: &String| (!value.is_empty()).then(|| value.clone());
    SessionEntryDto {
        kind: SessionKind::Outside,
        id: outside.internal_session_id.clone(),
        agent_id: outside.agent_id.clone(),
        title: text(&outside.first_prompt),
        internal_session_id: Some(outside.internal_session_id.clone()),
        working_directory: Some(outside.working_directory.clone()),
        last_activity_at: text(&outside.last_activity_at),
        goal_id: None,
        task_id: None,
        seat: None,
        task_agent_id: None,
        model: None,
        effort: None,
        status: None,
        attention_reason: None,
        attention_since: None,
        usage: None,
        context_used: None,
        context_size: None,
        created_at: None,
        ended_at: None,
        pull_request_id: None,
    }
}

/// [`goal_dto`] with everything it needs loaded: the repositories the goal
/// references, and what every session under it has spent.
pub(crate) async fn goal_dto_of(store: &Store, goal: store::Goal) -> Result<GoalDto, StoreError> {
    let mut dtos = goal_dtos_of(store, vec![goal]).await?;
    Ok(dtos.pop().expect("one goal in, one goal out"))
}

/// [`goal_dto_of`] for many goals, in the order given. The store reads what
/// they all need at once ([`Store::goal_parts`]), so the number of queries
/// stays the same whatever the number of goals, tasks and agents.
pub(crate) async fn goal_dtos_of(
    store: &Store,
    goals: Vec<store::Goal>,
) -> Result<Vec<GoalDto>, StoreError> {
    if goals.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = goals.iter().map(|g| g.id.clone()).collect();
    let mut parts = store.goal_parts(&ids).await?;
    let repositories = store.list_repositories().await?;
    goals
        .into_iter()
        .map(|goal| {
            let repos = parts
                .repository_ids
                .remove(&goal.id)
                .unwrap_or_default()
                .iter()
                .filter_map(|id| repositories.iter().find(|r| &r.id == id).cloned())
                .map(repository_dto)
                .collect();
            let usage = goal_usage(
                parts.seats.remove(&goal.id).unwrap_or_default(),
                parts.tasks.remove(&goal.id).unwrap_or_default(),
            );
            let steps = parts
                .steps
                .remove(&goal.id)
                .unwrap_or_default()
                .into_iter()
                .map(step_dto)
                .collect::<Result<Vec<_>, StoreError>>()?;
            Ok(goal_dto(goal, repos, usage, steps))
        })
        .collect()
}

/// A column a goal snapshotted, as the API answers it.
fn step_dto(s: store::GoalStep) -> Result<WorkflowStepDto, StoreError> {
    let invalid = |field| StoreError::Invalid(format!("column {} has invalid {field}", s.id));
    Ok(WorkflowStepDto {
        skills: serde_json::from_str(&s.skills).map_err(|_| invalid("skills"))?,
        rank: s
            .rank
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(|_| invalid("rank"))?,
        gate: s
            .gate
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(|_| invalid("gate"))?,
        id: s.id,
        title: s.title,
        description: s.description,
    })
}

/// What a task has spent, arranged the way it is read: one entry per agent
/// in column order, and the total of every session on the task.
///
/// The total is every session of the task, not only those of the agents the
/// task is staffed with today — a task re-staffed keeps what the first
/// staffing spent, and a total that did not count it would not add up. An
/// agent no longer staffed is listed after those that are, for the same
/// reason, with no column of its own.
async fn task_usage(
    store: &Store,
    task_id: &str,
    agents: &[(store::TaskAgent, Vec<String>)],
) -> Result<TaskUsageDto, StoreError> {
    let spent = store.task_usage(task_id).await?;
    let mut unstaffed = HashMap::new();
    for p in &spent {
        if agents.iter().all(|(agent, _)| agent.id != p.agent_id) {
            unstaffed.insert(p.agent_id.clone(), agent_skills(store, &p.agent_id).await);
        }
    }
    Ok(arranged_usage(spent, agents, &unstaffed))
}

/// [`task_usage`] with everything loaded: `unstaffed` holds the skills of
/// each agent in `spent` that is not in `agents`.
fn arranged_usage(
    spent: Vec<AgentUsage>,
    agents: &[(store::TaskAgent, Vec<String>)],
    unstaffed: &HashMap<String, Vec<String>>,
) -> TaskUsageDto {
    let total: TokenUsage = spent.iter().map(|p| p.usage).sum();
    let mut left = spent;

    let mut listed = Vec::new();
    for (agent, skills) in agents {
        if let Some(at) = left.iter().position(|p| p.agent_id == agent.id) {
            let spent = left.remove(at);
            listed.push(AgentUsageDto {
                step: Some(agent.step.clone()),
                agent_id: spent.agent_id,
                skills: skills.clone(),
                usage: spent.usage.into(),
            });
        }
    }
    for spent in left {
        listed.push(AgentUsageDto {
            step: None,
            skills: unstaffed.get(&spent.agent_id).cloned().unwrap_or_default(),
            agent_id: spent.agent_id,
            usage: spent.usage.into(),
        });
    }
    TaskUsageDto {
        total: total.into(),
        agents: listed,
    }
}

/// What a goal has spent: its orchestrator, the agents of every column of
/// every task, and the total of every session under it.
fn goal_usage(seats: Vec<store::SeatUsage>, tasks: Vec<store::TaskParts>) -> GoalUsageDto {
    let of = |seat: Seat| -> TokenUsageDto {
        seats
            .iter()
            .filter(|r| r.seat == seat)
            .map(|r| r.usage)
            .sum::<TokenUsage>()
            .into()
    };
    let agents = tasks
        .into_iter()
        .flat_map(|task| arranged_usage(task.usage, &task.agents, &task.unstaffed_skills).agents)
        .collect();
    GoalUsageDto {
        total: seats.iter().map(|r| r.usage).sum::<TokenUsage>().into(),
        orchestrator: of(Seat::Orchestrator),
        agents,
    }
}

/// The complete ledger row, shared by REST and events.
/// A ledger row as the API answers it, with the newest session the daemon
/// started on it.
pub(crate) async fn pull_request_dto_of(
    store: &Store,
    row: ariadne_store::PullRequest,
) -> Result<ariadne_api::pull_requests::PullRequestDto, StoreError> {
    // A request a task opened is kept by the agent of the task's current
    // column (030); any other has a session of its own (029).
    let filter = match &row.origin_task_id {
        Some(task_id) if row.role == "author" => SessionFilter {
            task_id: Some(task_id.clone()),
            ..Default::default()
        },
        _ => SessionFilter {
            pull_request_id: Some(row.id.clone()),
            ..Default::default()
        },
    };
    let current_agent = match row.origin_task_id.as_deref() {
        Some(task_id) if row.role == "author" => {
            let task = store.get_task(task_id).await?;
            match task.step.as_deref() {
                Some(step) => store
                    .list_task_agents(task_id)
                    .await?
                    .into_iter()
                    .find(|agent| agent.step == step)
                    .map(|agent| agent.id),
                None => None,
            }
        }
        _ => None,
    };
    let session_id = store
        .list_sessions(filter)
        .await?
        .into_iter()
        .rev()
        .find(|session| {
            row.role != "author"
                || (session.seat() == Some(Seat::Agent)
                    && session.task_agent_id.is_some()
                    && session.task_agent_id.as_deref() == current_agent.as_deref())
        })
        .map(|session| session.id);
    Ok(pull_request_dto(row, session_id))
}

/// A request Ariadne works on as the API answers it: its live read and
/// its row, with the session already looked up.
pub(crate) fn pull_request_dto(
    pull: ariadne_store::PullRequest,
    session_id: Option<String>,
) -> ariadne_api::pull_requests::PullRequestDto {
    ariadne_api::pull_requests::PullRequestDto {
        failed_checks: serde_json::from_str(&pull.failed_checks).unwrap_or_default(),
        behind_base: pull.behind_base,
        review_asked: pull.review_asked,
        review_model: pull.review_model,
        review_effort: pull.review_effort.clone(),
        review_skills: serde_json::from_str(&pull.review_skills_json).unwrap_or_default(),
        review_requested: pull.review_requested,
        body: pull.body,
        session_id,
        id: Some(pull.id),
        repository_id: pull.repository_id,
        number: pull.number,
        url: pull.url,
        title: pull.title,
        author_login: pull.author_login,
        state: pull.state,
        draft: pull.draft,
        head_branch: pull.head_branch,
        head_sha: pull.head_sha,
        head_repo: pull.head_repo,
        base_branch: pull.base_branch,
        checks: pull.checks,
        review_decision: pull.review_decision,
        unanswered_comments: pull.unanswered_comments,
        origin_task_id: pull.origin_task_id,
        opened_at: pull.opened_at,
        updated_at: pull.forge_updated_at,
        role: pull.role,
        ready: pull.ready,
    }
}

/// A request nobody works on as the API answers it: the forge's read alone.
pub(crate) fn forge_pull_dto(
    repository_id: &str,
    pull: crate::forge::pulls::ForgePullRequest,
    role: &str,
    review_requested: bool,
) -> ariadne_api::pull_requests::PullRequestDto {
    ariadne_api::pull_requests::PullRequestDto {
        id: None,
        repository_id: repository_id.to_string(),
        number: pull.number,
        url: pull.url,
        title: pull.title,
        body: pull.body,
        author_login: pull.author_login,
        state: pull.state,
        draft: pull.draft,
        head_branch: pull.head_branch,
        head_sha: pull.head_sha,
        head_repo: pull.head_repo,
        base_branch: pull.base_branch,
        checks: pull.checks,
        review_decision: pull.review_decision,
        opened_at: pull.opened_at,
        updated_at: pull.updated_at,
        role: role.to_string(),
        review_requested,
        unanswered_comments: 0,
        failed_checks: Vec::new(),
        behind_base: false,
        origin_task_id: None,
        ready: false,
        review_asked: false,
        review_model: None,
        review_effort: None,
        review_skills: Vec::new(),
        session_id: None,
    }
}

pub(crate) fn pull_request_comment_dto(
    comment: ariadne_store::PullRequestComment,
) -> ariadne_api::pull_requests::PullRequestCommentDto {
    ariadne_api::pull_requests::PullRequestCommentDto {
        id: comment.id,
        pull_request_id: comment.pull_request_id,
        thread_id: comment.thread_id,
        kind: comment.kind,
        author_login: comment.author_login,
        author_is_bot: comment.author_is_bot,
        body: comment.body,
        path: comment.path,
        line: comment.line,
        in_reply_to: comment.in_reply_to,
        created_at: comment.created_at,
        answered: comment.answered,
        resolved: comment.resolved,
        told_at: comment.told_at,
        from_review: comment.from_review,
    }
}
