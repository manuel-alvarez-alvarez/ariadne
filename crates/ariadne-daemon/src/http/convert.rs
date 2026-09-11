//! Store entity -> API DTO conversions.
//!
//! Every one of them is the same shape: most fields come across unchanged,
//! and a few do not — a column the store keeps as text and the API as an
//! enum, a derived flag, a name the caller loaded. Only the second kind is
//! worth reading, so [`dto!`] is what writes the first.

use ariadne_api::events::AgentEventDto;
use ariadne_api::goals::{GoalDto, GoalUsageDto};
use ariadne_api::memories::MemoryDto;
use ariadne_api::messages::MessageDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::skills::{SkillDto, SkillSeat};
use ariadne_api::tasks::{
    AgentUsageDto, TaskAgentDto, TaskDto, TaskPickDto, TaskTransitionDto, TaskUsageDto,
};
use ariadne_api::usage::TokenUsageDto;
use ariadne_core::{Actor, MessageKind, Seat, TokenUsage};
use ariadne_store::{self as store, AgentUsage, Store, StoreError};

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
    pub fn skill_dto(s: store::Skill) -> SkillDto {
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

    pub fn repository_dto(r: store::Repository) -> RepositoryDto {
        .. id, path, base_branch, description, created_at, updated_at
    }

    pub fn memory_dto(m: store::Memory) -> MemoryDto {
        .. id, repository_id, text, source_session_id, source_task_id,
           source_goal_id, created_at, expires_at
    }

    /// `repos` are the goal's repositories and `usage` its rollup, both of
    /// which the caller loads.
    fn goal_dto(
        g: store::Goal,
        repos: Vec<store::Repository>,
        usage: GoalUsageDto,
    ) -> GoalDto {
        status: g.status(),
        repos: repos.into_iter().map(repository_dto).collect(),
        usage: usage,
        .. id, title, description, model, effort,
           created_at, updated_at
    }

    /// `skills` is the agent's skill names in load order, which the caller
    /// loads beside the row; `branch` is the author's own, worked out from
    /// the task the caller holds, and None for a reviewer.
    fn task_agent_dto(
        a: store::TaskAgent,
        skills: Vec<String>,
        branch: Option<String>,
    ) -> TaskAgentDto {
        seat: a.seat(),
        skills: skills,
        branch: branch,
        .. id, model, effort, brief
    }

    pub fn task_pick_dto(p: store::TaskPick) -> TaskPickDto {
        .. reviewer_agent_id, author_agent_id, created_at
    }

    /// The agents come from the caller, which loads them with their skills,
    /// authors first and the reviewers in review order. So do the picks, and
    /// `reason`, which only an ended task has.
    fn task_dto(
        t: store::Task,
        agents: Vec<(store::TaskAgent, Vec<String>)>,
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
                task_agent_dto(a, skills, branch)
            })
            .collect(),
        depends_on: depends_on,
        usage: usage,
        reason: reason,
        picks: picks,
        .. id, goal_id, repo_id, title, description, branch, worktree_path,
           merge_commit, pr_url, picked_agent_id, created_at, updated_at
    }

    pub fn transition_dto(t: store::TaskTransition) -> TaskTransitionDto {
        .. id, from_status, to_status, actor, reason, created_at
    }

    pub fn message_dto(m: store::Message) -> MessageDto {
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
        .. id, goal_id, task_id, task_agent_id, model, effort, internal_session_id,
           worktree_path, attention_since,
           last_activity_at, created_at, ended_at
    }
}

/// Not in the [`dto!`] block above: `summary` is built from both `kind` and
/// `payload` together, which the macro's one-expression-per-field shape has
/// no room for.
pub fn event_dto(e: store::AgentEvent) -> AgentEventDto {
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

/// [`task_dto`] with everything it needs loaded from the store: the agents
/// staffed on the task with the skills each one loads, the dependencies, and
/// what has been spent.
///
/// The skills beside every agent are what a task is read for: an agent is its
/// skills, and no prompt can teach a reader to read an id.
pub async fn task_dto_of(store: &Store, task: store::Task) -> Result<TaskDto, StoreError> {
    let mut agents = Vec::new();
    for agent in store.list_task_agents(&task.id).await? {
        let skills = agent_skills(store, &agent.id).await;
        agents.push((agent, skills));
    }
    let depends_on = store.list_task_dependencies(&task.id).await?;
    let usage = task_usage(store, &task.id, &agents).await?;
    let reason = store.ended_reason(&task).await?;
    let picks = store
        .list_task_picks(&task.id)
        .await?
        .into_iter()
        .map(task_pick_dto)
        .collect();
    Ok(task_dto(task, agents, depends_on, usage, reason, picks))
}

/// [`session_dto`] with what the session has spent loaded from the store.
pub async fn session_dto_of(
    store: &Store,
    session: store::AgentSession,
) -> Result<SessionDto, StoreError> {
    let usage = store.session_usage(&session.id).await?;
    Ok(session_dto(session, usage.into()))
}

/// [`goal_dto`] with everything it needs loaded: the repositories the goal
/// references, and what every session under it has spent.
pub async fn goal_dto_of(store: &Store, goal: store::Goal) -> Result<GoalDto, StoreError> {
    let repos = store.list_goal_repositories(&goal.id).await?;
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
