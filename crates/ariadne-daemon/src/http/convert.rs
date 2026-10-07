//! Store entity -> API DTO conversions.
//!
//! Every one of them is the same shape: most fields come across unchanged,
//! and a few do not — a column the store keeps as text and the API as an
//! enum, a derived flag, a name the caller loaded. Only the second kind is
//! worth reading, so [`dto!`] is what writes the first.

use ariadne_api::events::AgentEventDto;
use ariadne_api::goals::{GoalDto, GoalRepositoryDto, GoalUsageDto};
use ariadne_api::messages::MessageDto;
use ariadne_api::permissions::{
    LearnedPermissionDto, LearnedPermissionLevel, LearnedPermissionScope, LearnedPermissionTarget,
};
use ariadne_api::repositories::{ForgeDto, RepositoryDto};
use ariadne_api::sessions::{OutsideSessionDto, SessionDto, SessionEntryDto, SessionKind};
use ariadne_api::skills::{SkillDto, SkillSeat};
use ariadne_api::tasks::{
    AgentUsageDto, TaskAgentDto, TaskDto, TaskPickDto, TaskTransitionDto, TaskUsageDto,
};
use ariadne_api::usage::TokenUsageDto;
use ariadne_core::models::agent_of;
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
        },
        summary: s.summary().to_string(),
        document: s.document_text().to_string(),
        document_is_default: s.document_is_default(),
        builtin: s.is_builtin(),
        .. name, created_at, updated_at
    }

pub(crate) fn repository_dto(r: store::Repository) -> RepositoryDto {
        permission_mode: r.permission_mode(),
        default_landing: r.default_landing(),
        forge: r.forge.map(forge_dto),
        .. id, path, base_branch, description, created_at, updated_at
    }

    fn forge_dto(f: store::ForgeIntegration) -> ForgeDto {
        kind: f.kind(),
        .. host, owner, name, remote, enabled, login, babysit_model, babysit_effort,
           review_model, review_effort
    }

    /// `repos` are the goal's repositories and `usage` its rollup, both of
    /// which the caller loads.
    fn goal_dto(
        g: store::Goal,
        repos: Vec<GoalRepositoryDto>,
        usage: GoalUsageDto,
    ) -> GoalDto {
        status: g.status(),
        landing: g.landing(),
        repos: repos,
        usage: usage,
        .. id, title, description, issue_url, orchestrated, model, effort,
           created_at, updated_at
    }

    /// `skills` is the agent's skill names in load order, which the caller
    /// loads beside the row; `branch` is the author's own, worked out from
    /// the task the caller holds, and None for a reviewer; `session_id` is
    /// this agent's live session, which the caller finds among the task's.
    fn task_agent_dto(
        a: store::TaskAgent,
        skills: Vec<String>,
        branch: Option<String>,
        session_id: Option<String>,
    ) -> TaskAgentDto {
        seat: a.seat(),
        skills: skills,
        branch: branch,
        session_id: session_id,
        .. id, model, effort, brief
    }

    pub(crate) fn task_pick_dto(p: store::TaskPick) -> TaskPickDto {
        .. reviewer_agent_id, author_agent_id, created_at
    }

    /// The agents come from the caller, which loads them with their skills,
    /// authors first and the reviewers in review order. So do the picks, and
    /// `reason`, which only an ended task has. `sessions` is the task's own,
    /// which each agent's live one is found among.
    fn task_dto(
        t: store::Task,
        agents: Vec<(store::TaskAgent, Vec<String>)>,
        sessions: Vec<store::AgentSession>,
        depends_on: Vec<String>,
        usage: TaskUsageDto,
        reason: Option<String>,
        picks: Vec<TaskPickDto>,
    ) -> TaskDto {
        status: t.status(),
        landing: t.landing(),
        stalled: t.is_stalled(),
        agents: agents
            .into_iter()
            .map(|(a, skills)| {
                let branch = (a.seat() == Seat::Author)
                    .then(|| store::author_branch(&t.branch, a.ordinal));
                let session_id = live_session_id(&a.id, &sessions);
                task_agent_dto(a, skills, branch, session_id)
            })
            .collect(),
        depends_on: depends_on,
        usage: usage,
        reason: reason,
        picks: picks,
        .. id, goal_id, repo_id, title, description, branch, worktree_path,
           merge_commit, pr_url, picked_agent_id, created_at, updated_at
    }

    pub(crate) fn transition_dto(t: store::TaskTransition) -> TaskTransitionDto {
        .. id, from_status, to_status, actor, reason, created_at
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
           last_activity_at, created_at, ended_at, title, switched_from
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
    let picks = store
        .list_task_picks(&task.id)
        .await?
        .into_iter()
        .map(task_pick_dto)
        .collect();
    Ok(task_dto(
        task, agents, sessions, depends_on, usage, reason, picks,
    ))
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
    }
}

/// [`goal_dto`] with everything it needs loaded: the repositories the goal
/// references, and what every session under it has spent.
pub(crate) async fn goal_dto_of(store: &Store, goal: store::Goal) -> Result<GoalDto, StoreError> {
    let repos = store
        .list_goal_repositories_with_branches(&goal.id)
        .await?
        .into_iter()
        .map(|(repo, goal_branch)| GoalRepositoryDto {
            repository: repository_dto(repo),
            goal_branch,
        })
        .collect();
    let usage = goal_usage(store, &goal.id).await?;
    Ok(goal_dto(goal, repos, usage))
}

/// What a task has spent, arranged the way it is read: the author's own, one
/// entry per reviewer in review order, and the total of every session on the
/// task.
///
/// The author's is every author-seat session, not only the agent the task is
/// staffed with today — a task re-staffed keeps what the first author spent,
/// and a total that did not count it would not add up. A reviewer no longer
/// staffed is listed after those that are, for the same reason.
async fn task_usage(
    store: &Store,
    task_id: &str,
    agents: &[(store::TaskAgent, Vec<String>)],
) -> Result<TaskUsageDto, StoreError> {
    let spent = store.task_usage(task_id).await?;
    let total: TokenUsage = spent.iter().map(|p| p.usage).sum();
    let author: TokenUsage = spent
        .iter()
        .filter(|p| p.seat == Seat::Author)
        .map(|p| p.usage)
        .sum();
    let mut left: Vec<&AgentUsage> = spent.iter().filter(|p| p.seat == Seat::Reviewer).collect();

    let mut listed = Vec::new();
    for (agent, skills) in agents.iter().filter(|(a, _)| a.seat() == Seat::Reviewer) {
        if let Some(at) = left.iter().position(|p| p.agent_id == agent.id) {
            let spent = left.remove(at);
            listed.push(AgentUsageDto {
                agent_id: spent.agent_id.clone(),
                skills: skills.clone(),
                usage: spent.usage.into(),
            });
        }
    }
    for spent in left {
        listed.push(AgentUsageDto {
            agent_id: spent.agent_id.clone(),
            skills: agent_skills(store, &spent.agent_id).await,
            usage: spent.usage.into(),
        });
    }
    Ok(TaskUsageDto {
        total: total.into(),
        author: author.into(),
        reviewers: listed,
    })
}

/// What a goal has spent, by seat: its orchestrator, the authors of its tasks,
/// their reviewers, and the total of all three.
async fn goal_usage(store: &Store, goal_id: &str) -> Result<GoalUsageDto, StoreError> {
    let spent = store.goal_usage(goal_id).await?;
    let of = |seat: Seat| -> TokenUsageDto {
        spent
            .iter()
            .filter(|r| r.seat == seat)
            .map(|r| r.usage)
            .sum::<TokenUsage>()
            .into()
    };
    Ok(GoalUsageDto {
        total: spent.iter().map(|r| r.usage).sum::<TokenUsage>().into(),
        orchestrator: of(Seat::Orchestrator),
        authors: of(Seat::Author),
        reviewers: of(Seat::Reviewer),
    })
}
