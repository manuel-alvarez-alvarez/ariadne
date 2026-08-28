//! One plausible row of each DTO, for the unit tests.
//!
//! Every test that renders a goal, a task, a session, a profile or a
//! repository needs a whole DTO to render, and only ever cares about two or
//! three of its fields. These build the rest once; the caller names what it is
//! testing with struct-update syntax:
//!
//! ```ignore
//! let stalled = TaskDto { stalled: true, ..fixtures::task("01T", "01G") };
//! ```

use ariadne_api::goals::GoalDto;
use ariadne_api::profiles::ProfileDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{AgentKind, GoalStatus, MergeStrategy, Role, SessionStatus, TaskStatus};

/// A stamp every fixture is created and updated at, so a rendered row is
/// reproducible.
pub const NOW: &str = "2026-08-18T10:00:00Z";

pub fn goal(id: &str, title: &str) -> GoalDto {
    GoalDto {
        id: id.into(),
        title: title.into(),
        description: String::new(),
        status: GoalStatus::Active,
        max_tasks: None,
        required_approvals: 1,
        planner_profile_id: "01PROFILE".into(),
        model: None,
        repos: Vec::new(),
        usage: Default::default(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

/// A task in progress, titled after its own id.
pub fn task(id: &str, goal_id: &str) -> TaskDto {
    TaskDto {
        id: id.into(),
        goal_id: goal_id.into(),
        repo_id: "01REPO".into(),
        title: format!("task {id}"),
        description: String::new(),
        status: TaskStatus::InProgress,
        engineer_profile_id: "01ENG".into(),
        engineer_profile_name: Some("Engineer".into()),
        planner_profile_name: Some("Planner".into()),
        model: None,
        reviewers: Vec::new(),
        depends_on: Vec::new(),
        branch: format!("a-task-{id}"),
        worktree_path: None,
        review_round: 0,
        stalled: false,
        merge_commit: None,
        pr_url: None,
        usage: Default::default(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

/// A running session: an engineer's when it names a task, a planner's when it
/// does not, and one the daemon has raised no attention flag for.
pub fn session(id: &str, goal_id: &str, task_id: Option<&str>) -> SessionDto {
    SessionDto {
        id: id.into(),
        goal_id: goal_id.into(),
        task_id: task_id.map(Into::into),
        role: match task_id {
            Some(_) => Role::Engineer,
            None => Role::Planner,
        },
        profile_id: "01PROF".into(),
        agent_kind: AgentKind::ClaudeCode,
        model: None,
        internal_session_id: None,
        tmux_session: format!("ariadne-{id}"),
        worktree_path: None,
        review_round: None,
        status: SessionStatus::Running,
        attention_reason: None,
        attention_since: None,
        last_activity_at: None,
        usage: Default::default(),
        created_at: NOW.into(),
        ended_at: None,
    }
}

/// A profile with a system prompt of its own, on no particular agent.
pub fn profile(name: &str, role: Role) -> ProfileDto {
    ProfileDto {
        id: format!("01{name}"),
        name: name.into(),
        role,
        model: None,
        system_prompt: "you are an engineer".into(),
        system_prompt_is_default: false,
        created_at: "2026-08-17T08:00:00Z".into(),
        updated_at: "2026-08-17T09:00:00Z".into(),
    }
}

pub fn repository(id: &str, path: &str, base_branch: &str) -> RepositoryDto {
    RepositoryDto {
        id: id.into(),
        path: path.into(),
        base_branch: base_branch.into(),
        merge_strategy: MergeStrategy::Direct,
        description: None,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}
