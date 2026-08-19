//! Ariadne domain types.
//!
//! Pure domain layer: no IO, no async. Everything here is shared by the
//! daemon, the store, the API DTOs and the CLI.

pub mod codex_hooks;
pub mod id;
pub mod models;
pub mod spawn_plan;
pub mod state_machine;

pub use state_machine::{Actor, TaskStatus, TransitionError, check_transition};

use serde::{Deserialize, Serialize};

/// The role an agent plays in the orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Planner,
    Engineer,
    Reviewer,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Planner, Role::Engineer, Role::Reviewer];

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Planner => "planner",
            Role::Engineer => "engineer",
            Role::Reviewer => "reviewer",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Role::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown role: {s}"))
    }
}

/// A prompt a profile owns beside its system prompt: the briefing an agent of
/// that role is started or resumed with. Each kind belongs to exactly one role
/// (see [`PromptKind::role`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    /// Initial briefing of a planner session.
    PlannerBriefing,
    /// Initial briefing of an engineer session.
    EngineerBriefing,
    /// Engineer resume briefing carrying the reviewers' change requests.
    ChangesRequested,
    /// Engineer resume briefing telling an approved task to merge.
    MergeInstructions,
    /// Initial briefing of a reviewer session.
    ReviewerBriefing,
    /// Reviewer resume briefing for a later round of the same task.
    ReviewerResume,
}

impl PromptKind {
    pub const ALL: [PromptKind; 6] = [
        PromptKind::PlannerBriefing,
        PromptKind::EngineerBriefing,
        PromptKind::ChangesRequested,
        PromptKind::MergeInstructions,
        PromptKind::ReviewerBriefing,
        PromptKind::ReviewerResume,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            PromptKind::PlannerBriefing => "planner_briefing",
            PromptKind::EngineerBriefing => "engineer_briefing",
            PromptKind::ChangesRequested => "changes_requested",
            PromptKind::MergeInstructions => "merge_instructions",
            PromptKind::ReviewerBriefing => "reviewer_briefing",
            PromptKind::ReviewerResume => "reviewer_resume",
        }
    }

    /// The role whose profiles own this prompt.
    pub fn role(&self) -> Role {
        match self {
            PromptKind::PlannerBriefing => Role::Planner,
            PromptKind::EngineerBriefing
            | PromptKind::ChangesRequested
            | PromptKind::MergeInstructions => Role::Engineer,
            PromptKind::ReviewerBriefing | PromptKind::ReviewerResume => Role::Reviewer,
        }
    }

    /// The prompts a profile of `role` owns, in briefing order.
    pub fn for_role(role: Role) -> &'static [PromptKind] {
        match role {
            Role::Planner => &[PromptKind::PlannerBriefing],
            Role::Engineer => &[
                PromptKind::EngineerBriefing,
                PromptKind::ChangesRequested,
                PromptKind::MergeInstructions,
            ],
            Role::Reviewer => &[PromptKind::ReviewerBriefing, PromptKind::ReviewerResume],
        }
    }
}

impl std::str::FromStr for PromptKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PromptKind::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown prompt kind: {s}"))
    }
}

/// Which coding-agent CLI a profile runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    Opencode,
}

impl AgentKind {
    pub const ALL: [AgentKind; 3] = [AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::Opencode];

    pub fn as_str(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude_code",
            AgentKind::Codex => "codex",
            AgentKind::Opencode => "opencode",
        }
    }

    /// The executable this agent CLI is launched as, and the name anything
    /// looking for it on a `PATH` searches for.
    pub fn binary(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Opencode => "opencode",
        }
    }

    /// The argv flags Ariadne launches this agent CLI with out of the box:
    /// the permission bypass each one spells its own way, so an agent working
    /// unattended in a throwaway worktree is not left waiting at a prompt.
    ///
    /// This is what a fresh database seeds the agent's flag list with and what
    /// restoring the defaults puts back; from there the list is the user's,
    /// edited over `/v1/agents`. Only flags a user may reasonably drop belong
    /// here — the structural ones (session ids, MCP and hook config, the
    /// system prompt, the model) are the adapters' own and are not negotiable.
    pub fn default_flags(&self) -> &'static [&'static str] {
        match self {
            AgentKind::ClaudeCode => &["--dangerously-skip-permissions"],
            AgentKind::Codex => &["--dangerously-bypass-approvals-and-sandbox"],
            // "auto-approve permissions that are not explicitly denied
            // (dangerous!)" — `opencode --help`, v1.18.15. The generated
            // `opencode.json` already allows the tools; this covers whatever
            // asks for approval outside it.
            AgentKind::Opencode => &["--auto"],
        }
    }
}

impl std::str::FromStr for AgentKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AgentKind::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown agent kind: {s}"))
    }
}

/// Goal lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    /// Planner session active; tasks being defined.
    Planning,
    /// Plan finalized; tasks executing.
    Active,
    /// All tasks merged (or goal-level completion recorded).
    Completed,
    Cancelled,
}

impl GoalStatus {
    pub const ALL: [GoalStatus; 4] = [
        GoalStatus::Planning,
        GoalStatus::Active,
        GoalStatus::Completed,
        GoalStatus::Cancelled,
    ];

    /// Nothing more will happen to this goal: no session of its own, no task
    /// left to move it. What may be deleted, and what cancelling refuses.
    pub fn is_terminal(&self) -> bool {
        matches!(self, GoalStatus::Completed | GoalStatus::Cancelled)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Planning => "planning",
            GoalStatus::Active => "active",
            GoalStatus::Completed => "completed",
            GoalStatus::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for GoalStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GoalStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown goal status: {s}"))
    }
}

/// Agent session lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Tmux session created, agent booting.
    Starting,
    /// Agent actively working.
    Running,
    /// Agent finished its turn and is waiting (Stop / turn-complete / idle).
    Idle,
    /// Tmux session gone or agent process exited.
    Exited,
    /// Spawn or runtime failure.
    Failed,
}

impl SessionStatus {
    pub const ALL: [SessionStatus; 5] = [
        SessionStatus::Starting,
        SessionStatus::Running,
        SessionStatus::Idle,
        SessionStatus::Exited,
        SessionStatus::Failed,
    ];

    pub fn is_live(&self) -> bool {
        matches!(
            self,
            SessionStatus::Starting | SessionStatus::Running | SessionStatus::Idle
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Starting => "starting",
            SessionStatus::Running => "running",
            SessionStatus::Idle => "idle",
            SessionStatus::Exited => "exited",
            SessionStatus::Failed => "failed",
        }
    }
}

impl std::str::FromStr for SessionStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        SessionStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown session status: {s}"))
    }
}

/// Why a live agent session needs the user's attention.
///
/// Orthogonal to [`SessionStatus`]: a session waiting on a permission prompt
/// is still `running` as far as its lifecycle goes, it just cannot make
/// progress until someone looks at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    /// Blocked on a permission / approval prompt.
    WaitingPermission,
    /// The agent asked the user something and is idle until answered.
    WaitingInput,
    /// The agent reported an error (API error, crash, `session.error`).
    AgentError,
    /// Tmux session or agent process gone while its work is still active.
    Disconnected,
    /// No activity for too long.
    Stalled,
}

impl AttentionReason {
    pub const ALL: [AttentionReason; 5] = [
        AttentionReason::WaitingPermission,
        AttentionReason::WaitingInput,
        AttentionReason::AgentError,
        AttentionReason::Disconnected,
        AttentionReason::Stalled,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            AttentionReason::WaitingPermission => "waiting_permission",
            AttentionReason::WaitingInput => "waiting_input",
            AttentionReason::AgentError => "agent_error",
            AttentionReason::Disconnected => "disconnected",
            AttentionReason::Stalled => "stalled",
        }
    }
}

impl std::str::FromStr for AttentionReason {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AttentionReason::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown attention reason: {s}"))
    }
}

/// Review verdict for one reviewer in one round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
}

impl ReviewVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewVerdict::Approve => "approve",
            ReviewVerdict::RequestChanges => "request_changes",
        }
    }
}

impl std::str::FromStr for ReviewVerdict {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "approve" => Ok(ReviewVerdict::Approve),
            "request_changes" => Ok(ReviewVerdict::RequestChanges),
            other => Err(format!("unknown review verdict: {other}")),
        }
    }
}

/// Author of a conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AuthorRole {
    Planner,
    Engineer,
    Reviewer,
    User,
    System,
}

impl AuthorRole {
    pub const ALL: [AuthorRole; 5] = [
        AuthorRole::Planner,
        AuthorRole::Engineer,
        AuthorRole::Reviewer,
        AuthorRole::User,
        AuthorRole::System,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            AuthorRole::Planner => "planner",
            AuthorRole::Engineer => "engineer",
            AuthorRole::Reviewer => "reviewer",
            AuthorRole::User => "user",
            AuthorRole::System => "system",
        }
    }
}

impl std::str::FromStr for AuthorRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AuthorRole::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown author role: {s}"))
    }
}
