//! Store entity -> API DTO conversions.

use ariadne_api::events::AgentEventDto;
use ariadne_api::goals::GoalDto;
use ariadne_api::messages::MessageDto;
use ariadne_api::profiles::{ProfileDto, ProfilePromptDto};
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::reviews::ReviewDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::tasks::{TaskDto, TaskTransitionDto};
use ariadne_store as store;

pub fn profile_dto(p: store::Profile) -> ProfileDto {
    ProfileDto {
        role: p.role(),
        agent_kind: p.agent_kind(),
        extra_flags: p.extra_flags(),
        id: p.id,
        name: p.name,
        model: p.model,
        system_prompt: p.system_prompt,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

pub fn profile_prompt_dto(p: store::ProfilePrompt) -> ProfilePromptDto {
    ProfilePromptDto {
        kind: p.kind(),
        content: p.content,
        updated_at: p.updated_at,
    }
}

pub fn repository_dto(r: store::Repository) -> RepositoryDto {
    RepositoryDto {
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
        id: g.id,
        title: g.title,
        description: g.description,
        max_tasks: g.max_tasks,
        required_approvals: g.required_approvals,
        planner_profile_id: g.planner_profile_id,
        repos: repos.into_iter().map(repository_dto).collect(),
        created_at: g.created_at,
        updated_at: g.updated_at,
    }
}

pub fn task_dto(t: store::Task, reviewers: Vec<String>, depends_on: Vec<String>) -> TaskDto {
    TaskDto {
        status: t.status(),
        stalled: t.is_stalled(),
        id: t.id,
        goal_id: t.goal_id,
        repo_id: t.repo_id,
        title: t.title,
        description: t.description,
        engineer_profile_id: t.engineer_profile_id,
        reviewer_profile_ids: reviewers,
        depends_on,
        branch: t.branch,
        worktree_path: t.worktree_path,
        review_round: t.review_round,
        merge_commit: t.merge_commit,
        created_at: t.created_at,
        updated_at: t.updated_at,
    }
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

pub fn message_dto(m: store::Message) -> MessageDto {
    MessageDto {
        author_role: m.author_role(),
        id: m.id,
        goal_id: m.goal_id,
        task_id: m.task_id,
        author_session_id: m.author_session_id,
        body: m.body,
        created_at: m.created_at,
    }
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
        id: s.id,
        goal_id: s.goal_id,
        task_id: s.task_id,
        profile_id: s.profile_id,
        internal_session_id: s.internal_session_id,
        tmux_session: s.tmux_session,
        worktree_path: s.worktree_path,
        review_round: s.review_round,
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
