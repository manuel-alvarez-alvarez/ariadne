//! Store entity -> API DTO conversions.

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

pub fn profile_dto(p: store::Profile) -> ProfileDto {
    ProfileDto {
        role: p.role(),
        agent_kind: p.agent_kind(),
        system_prompt: p.effective_system_prompt().to_string(),
        system_prompt_is_default: p.system_prompt_is_default(),
        id: p.id,
        name: p.name,
        model: p.model,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

pub fn agent_config_dto(c: store::AgentConfig) -> AgentConfigDto {
    AgentConfigDto {
        agent_kind: c.agent_kind(),
        default_flags: c.default_flags(),
        extra_flags: c.extra_flags(),
    }
}

pub fn profile_prompt_dto(p: store::ProfilePrompt) -> ProfilePromptDto {
    ProfilePromptDto {
        kind: p.kind(),
        content: p.content,
        is_default: p.is_default,
        updated_at: p.updated_at,
    }
}

pub fn repository_dto(r: store::Repository) -> RepositoryDto {
    RepositoryDto {
        merge_strategy: r.merge_strategy(),
        id: r.id,
        path: r.path,
        base_branch: r.base_branch,
        description: r.description,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

pub fn goal_dto(g: store::Goal, repos: Vec<store::Repository>) -> GoalDto {
    GoalDto {
        status: g.status(),
        agent_kind: g.agent_kind(),
        id: g.id,
        title: g.title,
        description: g.description,
        max_tasks: g.max_tasks,
        required_approvals: g.required_approvals,
        planner_profile_id: g.planner_profile_id,
        model: g.model,
        repos: repos.into_iter().map(repository_dto).collect(),
        created_at: g.created_at,
        updated_at: g.updated_at,
    }
}

/// `profile_name` is the reviewer profile's name, which the caller loads.
fn task_reviewer_dto(r: store::TaskReviewer, profile_name: Option<String>) -> TaskReviewerDto {
    TaskReviewerDto {
        agent_kind: r.agent_kind(),
        profile_id: r.profile_id,
        profile_name,
        model: r.model,
    }
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
    TaskDto {
        status: t.status(),
        stalled: t.is_stalled(),
        agent_kind: t.agent_kind(),
        id: t.id,
        goal_id: t.goal_id,
        repo_id: t.repo_id,
        title: t.title,
        description: t.description,
        engineer_profile_id: t.engineer_profile_id,
        engineer_profile_name,
        planner_profile_name,
        model: t.model,
        reviewers: reviewers
            .into_iter()
            .map(|(r, name)| task_reviewer_dto(r, name))
            .collect(),
        depends_on,
        branch: t.branch,
        worktree_path: t.worktree_path,
        review_round: t.review_round,
        merge_commit: t.merge_commit,
        pr_url: t.pr_url,
        created_at: t.created_at,
        updated_at: t.updated_at,
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

pub fn transition_dto(t: store::TaskTransition) -> TaskTransitionDto {
    TaskTransitionDto {
        id: t.id,
        from_status: t.from_status,
        to_status: t.to_status,
        actor: t.actor,
        reason: t.reason,
        created_at: t.created_at,
    }
}

/// `recipient_profile_name` is the name of the addressed profile, which the
/// callers below load; it is ignored for a message addressed to the user or to
/// nobody.
fn message_dto(m: store::Message, recipient_profile_name: Option<String>) -> MessageDto {
    let recipient = m.recipient().map(|r| MessageRecipientDto {
        kind: r.kind(),
        profile_id: r.profile_id().map(str::to_string),
        profile_name: r.profile_id().and(recipient_profile_name),
    });
    MessageDto {
        author_role: m.author_role(),
        id: m.id,
        goal_id: m.goal_id,
        task_id: m.task_id,
        author_session_id: m.author_session_id,
        recipient,
        body: m.body,
        created_at: m.created_at,
    }
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

pub fn review_dto(r: store::Review) -> ReviewDto {
    ReviewDto {
        verdict: r.verdict(),
        id: r.id,
        task_id: r.task_id,
        round: r.round,
        reviewer_profile_id: r.reviewer_profile_id,
        session_id: r.session_id,
        body: r.body,
        created_at: r.created_at,
    }
}

pub fn session_dto(s: store::AgentSession) -> SessionDto {
    SessionDto {
        role: s.role(),
        agent_kind: s.agent_kind(),
        status: s.status(),
        attention_reason: s.attention_reason(),
        id: s.id,
        goal_id: s.goal_id,
        task_id: s.task_id,
        profile_id: s.profile_id,
        model: s.model,
        internal_session_id: s.internal_session_id,
        tmux_session: s.tmux_session,
        worktree_path: s.worktree_path,
        review_round: s.review_round,
        attention_since: s.attention_since,
        last_activity_at: s.last_activity_at,
        created_at: s.created_at,
        ended_at: s.ended_at,
    }
}

pub fn event_dto(e: store::AgentEvent) -> AgentEventDto {
    AgentEventDto {
        payload: serde_json::from_str(&e.payload).unwrap_or(serde_json::Value::Null),
        id: e.id,
        session_id: e.session_id,
        task_id: e.task_id,
        agent_kind: e.agent_kind,
        kind: e.kind,
        created_at: e.created_at,
    }
}
