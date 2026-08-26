//! Row types. Enum-typed columns are stored as TEXT and surfaced as `String`;
//! [`enum_columns`] below is where each of them is read back as its
//! `ariadne-core` enum.

use std::str::FromStr;

use ariadne_core::{
    AgentKind, AttentionReason, AuthorRole, GoalStatus, MergeStrategy, PromptKind, RecipientKind,
    ReviewVerdict, Role, SessionStatus, TaskStatus,
};

use crate::defaults::default_system_prompt;

/// The typed reading of a TEXT column that holds a core enum. The accessor
/// and the column share a name; brackets mark a nullable column, which reads
/// back as `Option`.
///
/// A spelling the enum does not know is a schema violation rather than an
/// input error — nothing outside this crate writes these columns — so it
/// panics instead of widening every caller's error type.
macro_rules! enum_columns {
    ($($entity:ident { $($name:ident: $ty:tt),+ $(,)? })+) => {
        $(impl $entity {
            $(enum_columns!(@one $name: $ty);)+
        })+
    };
    (@one $name:ident: [$ty:ty]) => {
        pub fn $name(&self) -> Option<$ty> {
            self.$name.as_deref().map(|v| {
                <$ty>::from_str(v)
                    .unwrap_or_else(|_| panic!(concat!("invalid ", stringify!($name), " in db")))
            })
        }
    };
    (@one $name:ident: $ty:ty) => {
        pub fn $name(&self) -> $ty {
            <$ty>::from_str(&self.$name)
                .unwrap_or_else(|_| panic!(concat!("invalid ", stringify!($name), " in db")))
        }
    };
}

enum_columns! {
    Profile { role: Role, agent_kind: [AgentKind] }
    AgentConfig { agent_kind: AgentKind }
    ProfilePrompt { kind: PromptKind }
    Repository { merge_strategy: MergeStrategy }
    Goal { status: GoalStatus, agent_kind: [AgentKind] }
    Task { status: TaskStatus, agent_kind: [AgentKind] }
    TaskReviewer { agent_kind: [AgentKind] }
    AgentSession {
        role: Role,
        agent_kind: AgentKind,
        status: SessionStatus,
        attention_reason: [AttentionReason],
    }
    Message { author_role: AuthorRole }
    Review { verdict: ReviewVerdict }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub role: String,
    /// NULL = auto-resolve at spawn time (first installed agent CLI).
    pub agent_kind: Option<String>,
    pub model: Option<String>,
    /// The system prompt set on this profile, or NULL while it runs on the
    /// default of its role. Read through [`Profile::effective_system_prompt`],
    /// which is what the agent is spawned with.
    pub system_prompt: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Profile {
    /// The system prompt this profile is spawned with: the one set on it, or
    /// the default of its role.
    pub fn effective_system_prompt(&self) -> &str {
        self.system_prompt
            .as_deref()
            .unwrap_or_else(|| default_system_prompt(self.role()))
    }
    /// Whether [`Profile::effective_system_prompt`] is that role default rather
    /// than a text set on this profile.
    pub fn system_prompt_is_default(&self) -> bool {
        self.system_prompt.is_none()
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

/// One briefing of a profile as it takes effect: the text set on the profile,
/// or — while nothing is set for that kind — the default the kind ships with.
#[derive(Debug, Clone)]
pub struct ProfilePrompt {
    pub profile_id: String,
    pub kind: String,
    /// Template text with `{placeholder}` tokens the daemon fills in.
    pub content: String,
    /// Whether `content` is the kind's default rather than a text set on this
    /// profile.
    pub is_default: bool,
    /// When the text set on the profile was last written; `None` while the
    /// default stands, which nothing here dates.
    pub updated_at: Option<String>,
}

/// A git repository registered once, globally, and named by id from there on.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Repository {
    pub id: String,
    /// Absolute path of the checkout.
    pub path: String,
    pub base_branch: String,
    pub description: Option<String>,
    /// How a task lands on `base_branch` here: squashed onto it directly, or
    /// published as a pull or merge request.
    pub merge_strategy: String,
    pub created_at: String,
    pub updated_at: String,
}

/// The agent CLI, and optionally the model, a goal, a task or a reviewer slot
/// is pinned to because the user chose them, rather than because its profile
/// was on them.
///
/// The agent is the choice: a pin with no model runs that CLI on its own
/// default. Which choice it is belongs to whoever took the request — the store
/// is handed a pin already resolved, and writes it exactly where the profile's
/// own pins would have gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPin {
    pub agent_kind: AgentKind,
    /// None = the agent CLI's own default model.
    pub model: Option<String>,
}

impl AgentPin {
    /// The `(agent_kind, model)` a row is written with: the override where the
    /// caller gave one, and `profile`'s own where it did not.
    pub(crate) fn or_profile(
        pin: Option<&AgentPin>,
        profile: &Profile,
    ) -> (Option<String>, Option<String>) {
        match pin {
            Some(pin) => (Some(pin.agent_kind.as_str().to_string()), pin.model.clone()),
            None => (profile.agent_kind.clone(), profile.model.clone()),
        }
    }
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

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Task {
    pub id: String,
    pub goal_id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub engineer_profile_id: String,
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
    /// URL of the pull or merge request this task was published as, once its
    /// engineer has reported one. None for a task landed directly.
    pub pr_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Task {
    pub fn is_stalled(&self) -> bool {
        self.stalled != 0
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

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentSession {
    pub id: String,
    pub goal_id: String,
    pub task_id: Option<String>,
    pub role: String,
    pub profile_id: String,
    pub agent_kind: String,
    /// Model this session runs on. None = the CLI's own default. Taken from
    /// the pin its role carries — the task, the reviewer slot, the goal — when
    /// the session is created and never rewritten, so neither a profile edit
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
    /// When this session's agent process was last started. Every launch moves
    /// it, so it dates the run the session is in rather than the row.
    pub launched_at: Option<String>,
    pub created_at: String,
    pub ended_at: Option<String>,
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
    /// The reviewer of the round whose verdict this is.
    pub reviewer_profile_id: String,
    pub session_id: Option<String>,
    pub verdict: String,
    pub body: Option<String>,
    pub created_at: String,
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
