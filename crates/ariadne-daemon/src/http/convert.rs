//! Store entity -> API DTO conversions.
//!
//! Every one of them is the same shape: most fields come across unchanged,
//! and a few do not — a column the store keeps as text and the API as an
//! enum, a derived flag, a name the caller loaded. Only the second kind is
//! worth reading, so [`dto!`] is what writes the first.


use ariadne_api::agents::AgentConfigDto;
use ariadne_api::events::AgentEventDto;
use ariadne_api::goals::{GoalDto, GoalUsageDto};
use ariadne_api::profiles::ProfileDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::reviews::ReviewDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::tasks::{
    ProfileUsageDto, TaskDto, TaskReviewerDto, TaskTransitionDto, TaskUsageDto,
};
use ariadne_api::usage::TokenUsageDto;
use ariadne_core::{Role, TokenUsage};
use ariadne_store::{self as store, ProfileUsage, Store, StoreError};

use super::pins::spelled;

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
    pub fn profile_dto(p: store::Profile) -> ProfileDto {
        role: p.role(),
        model: spelled(p.agent_kind(), p.model.as_deref()),
        system_prompt: p.effective_system_prompt().to_string(),
        system_prompt_is_default: p.system_prompt_is_default(),
        .. id, name, effort, created_at, updated_at
    }

    pub fn agent_config_dto(c: store::AgentConfig) -> AgentConfigDto {
        agent_kind: c.agent_kind(),
        default_flags: c.default_flags(),
        extra_flags: c.extra_flags(),
        ..
    }

    pub fn repository_dto(r: store::Repository) -> RepositoryDto {
        merge_strategy: r.merge_strategy(),
        landing_prompt: r.landing_prompt_text().to_string(),
        landing_prompt_is_default: r.landing_prompt_is_default(),
        .. id, path, base_branch, description, created_at, updated_at
    }

    /// `repos` are the goal's repositories and `usage` its rollup, both of
    /// which the caller loads.
    fn goal_dto(
        g: store::Goal,
        repos: Vec<store::Repository>,
        usage: GoalUsageDto,
    ) -> GoalDto {
        status: g.status(),
        model: spelled(g.agent_kind(), g.model.as_deref()),
        repos: repos.into_iter().map(repository_dto).collect(),
        usage: usage,
        .. id, title, description, max_tasks, required_approvals,
           planner_profile_id, effort, created_at, updated_at
    }

    /// `name` is the reviewer profile's name, which the caller loads.
    fn task_reviewer_dto(r: store::TaskReviewer, name: Option<String>) -> TaskReviewerDto {
        model: spelled(r.agent_kind(), r.model.as_deref()),
        profile_name: name,
        .. profile_id, effort
    }

    /// The names come from the caller, which loads them: the engineer's, the
    /// planner's of the task's goal, and one per reviewer slot in slot order.
    /// So does `reason`, which only an ended task has.
    fn task_dto(
        t: store::Task,
        reviewers: Vec<(store::TaskReviewer, Option<String>)>,
        depends_on: Vec<String>,
        engineer_profile_name: Option<String>,
        planner_profile_name: Option<String>,
        usage: TaskUsageDto,
        reason: Option<String>,
    ) -> TaskDto {
        status: t.status(),
        stalled: t.is_stalled(),
        model: spelled(t.agent_kind(), t.model.as_deref()),
        reviewers: reviewers
            .into_iter()
            .map(|(r, name)| task_reviewer_dto(r, name))
            .collect(),
        depends_on: depends_on,
        engineer_profile_name: engineer_profile_name,
        planner_profile_name: planner_profile_name,
        usage: usage,
        reason: reason,
        .. id, goal_id, repo_id, title, description, engineer_profile_id,
           effort, branch, worktree_path, review_round, merge_commit, pr_url,
           created_at, updated_at
    }

    pub fn transition_dto(t: store::TaskTransition) -> TaskTransitionDto {
        .. id, from_status, to_status, actor, reason, created_at
    }

    pub fn review_dto(r: store::Review) -> ReviewDto {
        verdict: r.verdict(),
        .. id, task_id, round, reviewer_profile_id, session_id, body, created_at
    }

    /// `usage` is what this session has spent, which the caller loads.
    fn session_dto(s: store::AgentSession, usage: TokenUsageDto) -> SessionDto {
        role: s.role(),
        agent_kind: s.agent_kind(),
        status: s.status(),
        attention_reason: s.attention_reason(),
        usage: usage,
        .. id, goal_id, task_id, profile_id, model, effort, internal_session_id,
           tmux_session, worktree_path, review_round, attention_since,
           last_activity_at, created_at, ended_at
    }

    pub fn event_dto(e: store::AgentEvent) -> AgentEventDto {
        payload: serde_json::from_str(&e.payload).unwrap_or(serde_json::Value::Null),
        .. id, session_id, task_id, agent_kind, kind, created_at
    }
}

/// The name a task names one profile by, or None where the profile is gone —
/// which a profile a task names cannot be, the store refusing to delete one
/// anything references, so this leaves a task readable rather than failing
/// the read.
async fn profile_name(store: &Store, id: &str) -> Option<String> {
    store.get_profile(id).await.ok().map(|p| p.name)
}

/// [`task_dto`] with everything it needs loaded from the store: the reviewer
/// slots, the dependencies, and the profile names the task's participants are
/// known by — the engineer's, every reviewer's, and the planner's of its goal.
///
/// A name beside every id is what a task is read for: no prompt can teach an
/// agent to read an id.
pub async fn task_dto_of(store: &Store, task: store::Task) -> Result<TaskDto, StoreError> {
    let mut reviewers = Vec::new();
    for pin in store.list_task_reviewer_pins(&task.id).await? {
        let name = profile_name(store, &pin.profile_id).await;
        reviewers.push((pin, name));
    }
    let depends_on = store.list_task_dependencies(&task.id).await?;
    let engineer = profile_name(store, &task.engineer_profile_id).await;
    let planner_id = store.get_goal(&task.goal_id).await?.planner_profile_id;
    let planner = profile_name(store, &planner_id).await;
    let usage = task_usage(store, &task.id, &reviewers).await?;
    let reason = store.ended_reason(&task).await?;
    Ok(task_dto(
        task, reviewers, depends_on, engineer, planner, usage, reason,
    ))
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

/// What a task has spent, arranged the way it is read: the engineer's own,
/// one entry per reviewer profile in slot order, and the total of every
/// session on the task.
///
/// The engineer's is every engineer-role session, not only the profile the
/// task names today — a task moved to another engineer keeps what the first
/// one spent, and a total that did not count it would not add up. A reviewer
/// no longer holding a slot is listed after those that do, for the same
/// reason.
async fn task_usage(
    store: &Store,
    task_id: &str,
    reviewers: &[(store::TaskReviewer, Option<String>)],
) -> Result<TaskUsageDto, StoreError> {
    let spent = store.task_usage(task_id).await?;
    let total: TokenUsage = spent.iter().map(|p| p.usage).sum();
    let engineer: TokenUsage = spent
        .iter()
        .filter(|p| p.role == Role::Engineer)
        .map(|p| p.usage)
        .sum();
    let mut left: Vec<&ProfileUsage> = spent.iter().filter(|p| p.role == Role::Reviewer).collect();

    let mut listed = Vec::new();
    for (slot, name) in reviewers {
        if let Some(at) = left.iter().position(|p| p.profile_id == slot.profile_id) {
            let spent = left.remove(at);
            listed.push(ProfileUsageDto {
                profile_id: spent.profile_id.clone(),
                profile_name: name.clone(),
                usage: spent.usage.into(),
            });
        }
    }
    for spent in left {
        listed.push(ProfileUsageDto {
            profile_id: spent.profile_id.clone(),
            profile_name: profile_name(store, &spent.profile_id).await,
            usage: spent.usage.into(),
        });
    }
    Ok(TaskUsageDto {
        total: total.into(),
        engineer: engineer.into(),
        reviewers: listed,
    })
}

/// What a goal has spent, by role: its planner, the engineers of its tasks,
/// their reviewers, and the total of all three.
async fn goal_usage(store: &Store, goal_id: &str) -> Result<GoalUsageDto, StoreError> {
    let spent = store.goal_usage(goal_id).await?;
    let of = |role: Role| -> TokenUsageDto {
        spent
            .iter()
            .filter(|r| r.role == role)
            .map(|r| r.usage)
            .sum::<TokenUsage>()
            .into()
    };
    Ok(GoalUsageDto {
        total: spent.iter().map(|r| r.usage).sum::<TokenUsage>().into(),
        planner: of(Role::Planner),
        engineers: of(Role::Engineer),
        reviewers: of(Role::Reviewer),
    })
}

