//! Row types. Enum-typed columns are stored as TEXT and surfaced as `String`;
//! use the typed accessors to convert into `ariadne-core` enums.

use std::str::FromStr;

use ariadne_core::{
    AgentKind, AttentionReason, AuthorRole, GoalStatus, PromptKind, RecipientKind, ReviewVerdict,
    Role, SessionStatus, TaskStatus,
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
    /// Agent CLI the planner of this goal runs on, snapshotted from the
    /// profile when the goal was created. None = auto. Editing the profile
    /// afterwards leaves it alone.
    pub agent_kind: Option<String>,
    /// Model the planner of this goal runs on, snapshotted like `agent_kind`.
    /// None = the agent CLI's own default.
    pub model: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Goal {
    pub fn status(&self) -> GoalStatus {
        GoalStatus::from_str(&self.status).expect("valid goal status in db")
    }
    pub fn agent_kind(&self) -> Option<AgentKind> {
        self.agent_kind
            .as_deref()
            .map(|s| AgentKind::from_str(s).expect("valid agent kind in db"))
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
    /// Profile that lands this task once it is approved, assigned when the
    /// task was created exactly as the engineer above it was.
    pub integrator_profile_id: String,
    /// Agent CLI the engineer of this task runs on, snapshotted from the
    /// profile when the task was created. None = auto. Editing the profile
    /// afterwards leaves it alone.
    pub agent_kind: Option<String>,
    /// Model the engineer of this task runs on, snapshotted like
    /// `agent_kind`. None = the agent CLI's own default.
    pub model: Option<String>,
    pub branch: String,
    pub worktree_path: Option<String>,
    pub review_round: i64,
    pub stalled: i64,
    pub merge_commit: Option<String>,
    /// Number of the pull request this task was published as, once its
    /// integrator has reported one. None while there is none — every task
    /// landed locally, and every task before the integrator opened one.
    pub pr_number: Option<i64>,
    /// Its URL, as the forge spells it; what says which forge it is on.
    pub pr_url: Option<String>,
    /// Ids of the pull request comments already relayed to the engineer, as a
    /// JSON array: what keeps a comment from being relayed twice as the
    /// daemon polls.
    pub pr_relayed_comments: Option<String>,
    /// Whether the user has been told the pull request is approved and ready
    /// for them to merge, so they are told once rather than every poll.
    pub pr_approved_notified: i64,
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
    /// The comment ids already relayed to the engineer. Unreadable JSON reads
    /// as none relayed: a comment repeated is better than one never delivered.
    pub fn pr_relayed_comments(&self) -> Vec<String> {
        self.pr_relayed_comments
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default()
    }
    pub fn pr_approved_notified(&self) -> bool {
        self.pr_approved_notified != 0
    }
    pub fn agent_kind(&self) -> Option<AgentKind> {
        self.agent_kind
            .as_deref()
            .map(|s| AgentKind::from_str(s).expect("valid agent kind in db"))
    }
}

/// One reviewer slot of a task: which profile reviews it, in which order, and
/// what that reviewer was pinned to when the slot was created.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskReviewer {
    pub task_id: String,
    pub profile_id: String,
    /// Planner-assigned order, 0-based.
    pub position: i64,
    /// Agent CLI this reviewer runs on, snapshotted from the profile when the
    /// slot was created. None = auto.
    pub agent_kind: Option<String>,
    /// Model this reviewer runs on, snapshotted like `agent_kind`.
    /// None = the agent CLI's own default.
    pub model: Option<String>,
}

impl TaskReviewer {
    pub fn agent_kind(&self) -> Option<AgentKind> {
        self.agent_kind
            .as_deref()
            .map(|s| AgentKind::from_str(s).expect("valid agent kind in db"))
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
    /// Model this session runs on, as handed to the agent CLI. None = no
    /// model was asked for, i.e. the CLI's own default. Taken from the pin its
    /// role carries (the task, the reviewer slot, the goal) when the session
    /// is created and never rewritten afterwards, so neither a profile edit
    /// nor a resume moves a running conversation onto another model.
    pub model: Option<String>,
    pub internal_session_id: Option<String>,
    pub tmux_session: String,
    pub worktree_path: Option<String>,
    /// Reviewer sessions only: the review round the session is working on.
    /// One session serves every round, so this moves with the task rather
    /// than recording the round the row was created in.
    pub review_round: Option<i64>,
    pub status: String,
    /// Why this session needs the user's attention, if it does. Orthogonal to
    /// `status`: an agent blocked on a permission prompt is still running.
    pub attention_reason: Option<String>,
    /// When the current `attention_reason` was first raised.
    pub attention_since: Option<String>,
    pub last_activity_at: Option<String>,
    /// When this session's agent process was last started. Every launch of
    /// the row — the first spawn and every resume after it — moves it, so it
    /// dates the run the session is in rather than the row.
    pub launched_at: Option<String>,
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
    pub fn attention_reason(&self) -> Option<AttentionReason> {
        self.attention_reason
            .as_deref()
            .map(|r| AttentionReason::from_str(r).expect("valid attention reason in db"))
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Message {
    pub id: String,
    pub goal_id: String,
    pub task_id: Option<String>,
    pub author_role: String,
    pub author_session_id: Option<String>,
    /// Whom the message is addressed to, if anyone. None = the thread.
    pub recipient_kind: Option<String>,
    /// The addressed profile, set exactly when the kind is `profile`.
    pub recipient_profile_id: Option<String>,
    pub body: String,
    pub created_at: String,
}

impl Message {
    pub fn author_role(&self) -> AuthorRole {
        AuthorRole::from_str(&self.author_role).expect("valid author role in db")
    }

    /// The addressee, rebuilt from the two columns that hold it.
    pub fn recipient(&self) -> Option<Recipient> {
        let kind = RecipientKind::from_str(self.recipient_kind.as_deref()?)
            .expect("valid recipient kind in db");
        Some(match kind {
            RecipientKind::Profile => Recipient::Profile(
                self.recipient_profile_id
                    .clone()
                    .expect("a profile recipient in db carries its profile id"),
            ),
            RecipientKind::User => Recipient::User,
        })
    }
}

/// A message's addressee: one agent profile, or the human user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipient {
    Profile(String),
    User,
}

impl Recipient {
    pub fn kind(&self) -> RecipientKind {
        match self {
            Recipient::Profile(_) => RecipientKind::Profile,
            Recipient::User => RecipientKind::User,
        }
    }

    /// The addressed profile's id; None when the user is the addressee.
    pub fn profile_id(&self) -> Option<&str> {
        match self {
            Recipient::Profile(id) => Some(id),
            Recipient::User => None,
        }
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
