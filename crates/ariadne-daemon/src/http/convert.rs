//! Store entity -> API DTO conversions.
//!
//! Every one of them is the same shape: most fields come across unchanged,
//! and a few do not — a column the store keeps as text and the API as an
//! enum, a derived flag, a name the caller loaded. Only the second kind is
//! worth reading, so [`dto!`] is what writes the first.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use ariadne_api::agents::AgentConfigDto;
use ariadne_api::events::AgentEventDto;
use ariadne_api::goals::GoalDto;
use ariadne_api::messages::{MessageDto, MessageRecipientDto};
use ariadne_api::profiles::{ProfileDto, ProfilePromptDto};
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::reviews::ReviewDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::tasks::{TaskDto, TaskReviewerDto, TaskTransitionDto};
use ariadne_store::{self as store, Store, StoreError};

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
        agent_kind: p.agent_kind(),
        system_prompt: p.effective_system_prompt().to_string(),
        system_prompt_is_default: p.system_prompt_is_default(),
        .. id, name, model, created_at, updated_at
    }

    pub fn agent_config_dto(c: store::AgentConfig) -> AgentConfigDto {
        agent_kind: c.agent_kind(),
        default_flags: c.default_flags(),
        extra_flags: c.extra_flags(),
        ..
    }

    pub fn profile_prompt_dto(p: store::ProfilePrompt) -> ProfilePromptDto {
        kind: p.kind(),
        .. content, is_default, updated_at
    }

    pub fn repository_dto(r: store::Repository) -> RepositoryDto {
        merge_strategy: r.merge_strategy(),
        .. id, path, base_branch, description, created_at, updated_at
    }

    /// `repos` are the goal's repositories, which the caller loads.
    pub fn goal_dto(g: store::Goal, repos: Vec<store::Repository>) -> GoalDto {
        status: g.status(),
        agent_kind: g.agent_kind(),
        repos: repos.into_iter().map(repository_dto).collect(),
        .. id, title, description, max_tasks, required_approvals,
           planner_profile_id, model, created_at, updated_at
    }

    /// `name` is the reviewer profile's name, which the caller loads.
    fn task_reviewer_dto(r: store::TaskReviewer, name: Option<String>) -> TaskReviewerDto {
        agent_kind: r.agent_kind(),
        profile_name: name,
        .. profile_id, model
    }

    /// The names come from the caller, which loads them: the engineer's, the
    /// planner's of the task's goal, and one per reviewer slot in slot order.
    fn task_dto(
        t: store::Task,
        reviewers: Vec<(store::TaskReviewer, Option<String>)>,
        depends_on: Vec<String>,
        engineer_profile_name: Option<String>,
        planner_profile_name: Option<String>,
    ) -> TaskDto {
        status: t.status(),
        stalled: t.is_stalled(),
        agent_kind: t.agent_kind(),
        reviewers: reviewers
            .into_iter()
            .map(|(r, name)| task_reviewer_dto(r, name))
            .collect(),
        depends_on: depends_on,
        engineer_profile_name: engineer_profile_name,
        planner_profile_name: planner_profile_name,
        .. id, goal_id, repo_id, title, description, engineer_profile_id, model,
           branch, worktree_path, review_round, merge_commit, pr_url,
           created_at, updated_at
    }

    pub fn transition_dto(t: store::TaskTransition) -> TaskTransitionDto {
        .. id, from_status, to_status, actor, reason, created_at
    }

    /// `name` is the addressed profile's name, which the callers below load;
    /// it is ignored for a message addressed to the user or to nobody.
    fn message_dto(m: store::Message, name: Option<String>) -> MessageDto {
        author_role: m.author_role(),
        recipient: m.recipient().map(|r| MessageRecipientDto {
            kind: r.kind(),
            profile_id: r.profile_id().map(str::to_string),
            profile_name: r.profile_id().and(name),
        }),
        .. id, goal_id, task_id, author_session_id, body, created_at
    }

    pub fn review_dto(r: store::Review) -> ReviewDto {
        verdict: r.verdict(),
        .. id, task_id, round, reviewer_profile_id, session_id, body, created_at
    }

    pub fn session_dto(s: store::AgentSession) -> SessionDto {
        role: s.role(),
        agent_kind: s.agent_kind(),
        status: s.status(),
        attention_reason: s.attention_reason(),
        .. id, goal_id, task_id, profile_id, model, internal_session_id,
           tmux_session, worktree_path, review_round, attention_since,
           last_activity_at, created_at, ended_at
    }

    pub fn event_dto(e: store::AgentEvent) -> AgentEventDto {
        payload: serde_json::from_str(&e.payload).unwrap_or(serde_json::Value::Null),
        .. id, session_id, task_id, agent_kind, kind, created_at
    }
}

/// The name a message addresses one profile by, or None where the profile is
/// gone — which a profile a task names cannot be, the store refusing to delete
/// one anything references, so this leaves a task readable rather than failing
/// the read.
async fn profile_name(store: &Store, id: &str) -> Option<String> {
    store.get_profile(id).await.ok().map(|p| p.name)
}

/// [`task_dto`] with everything it needs loaded from the store: the reviewer
/// slots, the dependencies, and the profile names an agent addresses the task's
/// participants by — the engineer's, every reviewer's, and the planner's of its
/// goal, which takes part in the thread without being a field of the task.
///
/// A name beside every id is what a task is read for: `to` takes a name, and no
/// prompt can teach an agent to read an id.
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
    Ok(task_dto(task, reviewers, depends_on, engineer, planner))
}

/// [`message_dto`] with the addressee's name loaded from the store.
pub async fn message_dto_of(store: &Store, m: store::Message) -> Result<MessageDto, StoreError> {
    let name = match &m.recipient_profile_id {
        Some(id) => Some(store.get_profile(id).await?.name),
        None => None,
    };
    Ok(message_dto(m, name))
}

/// A whole thread, resolving each addressed profile once however many of its
/// messages name it.
pub async fn message_dtos(
    store: &Store,
    msgs: Vec<store::Message>,
) -> Result<Vec<MessageDto>, StoreError> {
    let mut names: HashMap<String, String> = HashMap::new();
    for id in msgs.iter().filter_map(|m| m.recipient_profile_id.clone()) {
        if let Entry::Vacant(slot) = names.entry(id) {
            let name = store.get_profile(slot.key()).await?.name;
            slot.insert(name);
        }
    }
    Ok(msgs
        .into_iter()
        .map(|m| {
            let name = m
                .recipient_profile_id
                .as_ref()
                .and_then(|id| names.get(id).cloned());
            message_dto(m, name)
        })
        .collect())
}
