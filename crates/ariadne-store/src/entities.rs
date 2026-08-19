//! Row types. Enum-typed columns are stored as TEXT and surfaced as `String`;
//! use the typed accessors to convert into `ariadne-core` enums.

use std::str::FromStr;

use ariadne_core::{
    AgentKind, AuthorRole, GoalStatus, PromptKind, ReviewVerdict, Role, SessionStatus, TaskStatus,
};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub role: String,
    /// NULL = auto-resolve at spawn time (first installed agent CLI).
    pub agent_kind: Option<String>,
    pub model: Option<String>,
    pub system_prompt: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Profile {
    pub fn role(&self) -> Role {
        Role::from_str(&self.role).expect("valid role in db")
    }
    pub fn agent_kind(&self) -> Option<AgentKind> {
        self.agent_kind
            .as_deref()
            .map(|s| AgentKind::from_str(s).expect("valid agent kind in db"))
    }
}

/// How one agent CLI is launched, shared by every profile that runs on it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentConfig {
    pub agent_kind: String,
    /// JSON array of argv strings.
    pub extra_flags: String,
    pub updated_at: String,
}

impl AgentConfig {
    pub fn agent_kind(&self) -> AgentKind {
        AgentKind::from_str(&self.agent_kind).expect("valid agent kind in db")
    }
    pub fn extra_flags(&self) -> Vec<String> {
        serde_json::from_str(&self.extra_flags).unwrap_or_default()
    }
    /// What this agent kind ships with, and what restoring the defaults puts
    /// back.
    pub fn default_flags(&self) -> Vec<String> {
        self.agent_kind()
            .default_flags()
            .iter()
            .map(|f| f.to_string())
            .collect()
    }
}

/// One editable briefing of a profile, keyed by [`PromptKind`].
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProfilePrompt {
    pub profile_id: String,
    pub kind: String,
    /// Template text with `{placeholder}` tokens the daemon fills in.
    pub content: String,
    pub updated_at: String,
}

impl ProfilePrompt {
    pub fn kind(&self) -> PromptKind {
        PromptKind::from_str(&self.kind).expect("valid prompt kind in db")
    }
}

/// A git repository registered once, globally, and named by id from there on.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Repository {
    pub id: String,
    /// Absolute path of the checkout.
    pub path: String,
    pub base_branch: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub max_tasks: Option<i64>,
    pub required_approvals: i64,
    pub planner_profile_id: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Goal {
    pub fn status(&self) -> GoalStatus {
        GoalStatus::from_str(&self.status).expect("valid goal status in db")
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Task {
    pub id: String,
    pub goal_id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub engineer_profile_id: String,
    pub branch: String,
    pub worktree_path: Option<String>,
    pub review_round: i64,
    pub stalled: i64,
    pub merge_commit: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Task {
    pub fn status(&self) -> TaskStatus {
        TaskStatus::from_str(&self.status).expect("valid task status in db")
    }
    pub fn is_stalled(&self) -> bool {
        self.stalled != 0
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentSession {
    pub id: String,
    pub goal_id: String,
    pub task_id: Option<String>,
    pub role: String,
    pub profile_id: String,
    pub agent_kind: String,
    /// Model this session was launched with, as handed to the agent CLI.
    /// None = no model was asked for, i.e. the CLI's own default. Snapshotted
    /// at launch rather than read off the profile, so editing a profile does
    /// not rewrite what a running session is actually using.
    pub model: Option<String>,
    pub internal_session_id: Option<String>,
    pub tmux_session: String,
    pub worktree_path: Option<String>,
    /// Reviewer sessions only: the review round the session is working on.
    /// One session serves every round, so this moves with the task rather
    /// than recording the round the row was created in.
    pub review_round: Option<i64>,
    pub status: String,
    pub last_activity_at: Option<String>,
    pub created_at: String,
    pub ended_at: Option<String>,
}

impl AgentSession {
    pub fn role(&self) -> Role {
        Role::from_str(&self.role).expect("valid role in db")
    }
    pub fn agent_kind(&self) -> AgentKind {
        AgentKind::from_str(&self.agent_kind).expect("valid agent kind in db")
    }
    pub fn status(&self) -> SessionStatus {
        SessionStatus::from_str(&self.status).expect("valid session status in db")
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Message {
    pub id: String,
    pub goal_id: String,
    pub task_id: Option<String>,
    pub author_role: String,
    pub author_session_id: Option<String>,
    pub body: String,
    pub created_at: String,
}

impl Message {
    pub fn author_role(&self) -> AuthorRole {
        AuthorRole::from_str(&self.author_role).expect("valid author role in db")
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Review {
    pub id: String,
    pub task_id: String,
    pub round: i64,
    pub reviewer_profile_id: String,
    pub session_id: Option<String>,
    pub verdict: String,
    pub body: Option<String>,
    pub created_at: String,
}

impl Review {
    pub fn verdict(&self) -> ReviewVerdict {
        ReviewVerdict::from_str(&self.verdict).expect("valid verdict in db")
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentEvent {
    pub id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub agent_kind: Option<String>,
    pub kind: String,
    pub payload: String,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskTransition {
    pub id: String,
    pub task_id: String,
    pub from_status: String,
    pub to_status: String,
    pub actor: String,
    pub reason: Option<String>,
    pub created_at: String,
}
