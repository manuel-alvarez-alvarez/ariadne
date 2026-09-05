//! Row types. Enum-typed columns are stored as TEXT and surfaced as `String`;
//! [`enum_columns`] below is where each of them is read back as its
//! `ariadne-core` enum.

use std::str::FromStr;

use ariadne_core::{
    AgentKind, AttentionReason, GoalStatus, MergeStrategy, ReviewVerdict, Seat, SessionStatus,
    TaskStatus,
};

use crate::defaults::{default_landing_prompt, default_skill_document, skill_summary};

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
    AgentConfig { agent_kind: AgentKind }
    Repository { merge_strategy: MergeStrategy }
    Goal { status: GoalStatus, agent_kind: [AgentKind] }
    Task { status: TaskStatus }
    TaskAgent { seat: Seat, agent_kind: [AgentKind] }
    AgentSession {
        seat: Seat,
        agent_kind: AgentKind,
        status: SessionStatus,
        attention_reason: [AttentionReason],
    }
    Review { verdict: ReviewVerdict }
}

/// A skill: one document that tells a generic agent how to do one kind of
/// work. Its name is its identity — an agent loads it by name — and the
/// document is the whole `SKILL.md`, frontmatter included.
///
/// A `NULL` document is a built-in still on the text Ariadne ships, which is
/// why rewording a shipped skill reaches every database without a migration.
/// A skill the user wrote has no default behind it and carries its own text.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Skill {
    pub name: String,
    /// The document set on this skill, or NULL while a built-in runs on the
    /// text Ariadne ships. Read through [`Skill::document_text`].
    pub document: Option<String>,
    pub builtin: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl Skill {
    /// Whether Ariadne ships this skill, and so whether it has a default to
    /// be reset to and refuses deletion.
    pub fn is_builtin(&self) -> bool {
        self.builtin != 0
    }

    /// The document an agent loading this skill reads: the one set on it, or
    /// the text Ariadne ships under its name.
    ///
    /// The empty string is unreachable through the store — the schema refuses
    /// a non-built-in with no document, and a built-in always has one to fall
    /// back on — and is here so that a row written by a future build that this
    /// one no longer ships reads as an empty skill rather than a panic.
    pub fn document_text(&self) -> &str {
        self.document
            .as_deref()
            .or_else(|| default_skill_document(&self.name))
            .unwrap_or("")
    }

    /// Whether [`Skill::document_text`] is the shipped text rather than one
    /// somebody wrote.
    pub fn document_is_default(&self) -> bool {
        self.document.is_none()
    }

    /// The one line the index in an agent's system prompt carries: the
    /// `description` of the document's frontmatter, or the name where the
    /// document names none.
    pub fn summary(&self) -> &str {
        skill_summary(self.document_text()).unwrap_or(&self.name)
    }
}

/// How one agent CLI is launched, shared by every agent that runs on it.
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
    /// The landing briefing set on this repository, or NULL while it runs on
    /// the built-in default of its merge strategy. Read through
    /// [`Repository::landing_prompt_text`], which is what the author of an
    /// approved task is briefed with.
    pub landing_prompt: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Repository {
    /// The landing briefing this repository hands its author: the text set
    /// on it, or the built-in default of its merge strategy.
    pub fn landing_prompt_text(&self) -> &str {
        self.landing_prompt
            .as_deref()
            .unwrap_or_else(|| default_landing_prompt(self.merge_strategy()))
    }

    /// Whether [`Repository::landing_prompt_text`] is that strategy default
    /// rather than a text set on this repository.
    pub fn landing_prompt_is_default(&self) -> bool {
        self.landing_prompt.is_none()
    }
}

/// The agent CLI, and optionally the model and the effort, that a goal's
/// orchestrator or one of a task's agents runs on.
///
/// The agent is the choice: a pin with no model runs that CLI on its own
/// default, and a pin with no effort runs the model at whatever the CLI runs
/// it at. There is nothing behind a pin to fall back to: what the
/// orchestrator sized the agent at, or what the user chose instead, is the
/// whole of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPin {
    pub agent_kind: AgentKind,
    /// None = the agent CLI's own default model.
    pub model: Option<String>,
    /// None = whatever the agent CLI runs that model at.
    pub effort: Option<String>,
}

impl AgentPin {
    /// The `(agent_kind, model, effort)` a row is written with: the pin's own
    /// where there is one, and all-auto where there is none — auto CLI, the
    /// CLI's default model, and whatever it runs that model at.
    pub(crate) fn columns(
        pin: Option<&AgentPin>,
    ) -> (Option<String>, Option<String>, Option<String>) {
        match pin {
            Some(pin) => (
                Some(pin.agent_kind.as_str().to_string()),
                pin.model.clone(),
                pin.effort.clone(),
            ),
            None => (None, None, None),
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
    /// Agent CLI this goal's orchestrator runs on. None = auto, resolved at
    /// spawn time to the first installed CLI.
    pub agent_kind: Option<String>,
    /// Model this goal's orchestrator runs on. None = the agent CLI's own
    /// default.
    pub model: Option<String>,
    /// Effort that model is run at. None = whatever the agent CLI runs it at.
    pub effort: Option<String>,
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
    pub branch: String,
    pub worktree_path: Option<String>,
    pub review_round: i64,
    pub stalled: i64,
    pub merge_commit: Option<String>,
    /// URL of the pull or merge request this task was published as, once its
    /// author has reported one. None for a task landed directly.
    pub pr_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Task {
    pub fn is_stalled(&self) -> bool {
        self.stalled != 0
    }
}

/// One agent staffed on a task: where it sits, what it runs on, what it was
/// told, and — through [`crate::Store::agent_skills`] — what it knows.
///
/// The agent has no identity of its own. `seat` says only whether it authors
/// the task or reviews it, which is what the state machine and the launcher
/// need; everything about the work itself comes from its skills.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskAgent {
    pub id: String,
    pub task_id: String,
    /// `author` or `reviewer`; never `orchestrator`, which belongs to a goal.
    pub seat: String,
    /// The order the orchestrator listed this agent in, 0-based within a seat.
    pub ordinal: i64,
    /// Agent CLI this agent runs on. None = auto.
    pub agent_kind: Option<String>,
    /// Model it runs on. None = the agent CLI's own default.
    pub model: Option<String>,
    /// Effort that model is run at. None = whatever the CLI runs it at.
    pub effort: Option<String>,
    /// What the orchestrator told this agent beyond the task itself, where it
    /// had anything to add. None = the task is the whole of it.
    pub brief: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentSession {
    pub id: String,
    pub goal_id: String,
    pub task_id: Option<String>,
    pub seat: String,
    /// The staffed agent this session runs, or None for an orchestrator,
    /// which no task staffs.
    pub task_agent_id: Option<String>,
    pub agent_kind: String,
    /// Model this session runs on. None = the CLI's own default. Taken from
    /// the pin its seat carries — the goal for an orchestrator, the staffed
    /// agent otherwise — when the session is created, and never rewritten, so
    /// no later edit moves a running conversation onto another model.
    pub model: Option<String>,
    /// Effort this session's model is run at, copied off the same pin as
    /// `model` and never rewritten either. None = the CLI's own.
    pub effort: Option<String>,
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
    /// Which launch that is: a fresh id per agent process, carried by the
    /// process itself (`ARIADNE_LAUNCH_ID`) so that what it reports can be
    /// told from what the process it replaced is still reporting. None where
    /// the session has never been launched.
    pub launch_id: Option<String>,
    /// When this session was last found to owe a compaction of its agent
    /// conversation, or None where nothing is owed: set at every hand-off
    /// and cleared once the compaction is done or given up on.
    pub compact_owed_at: Option<String>,
    pub created_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Review {
    pub id: String,
    pub task_id: String,
    pub round: i64,
    /// The task agent whose verdict this is.
    pub reviewer_agent_id: String,
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
