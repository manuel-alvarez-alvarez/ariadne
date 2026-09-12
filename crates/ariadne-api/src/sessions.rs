//! Agent-session DTOs.

use ariadne_core::{AttentionReason, Landing, PermissionMode, Seat, SessionStatus};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::usage::TokenUsageDto;
use crate::{
    goals::GoalDto,
    tasks::{AgentAssignment, TaskDto},
};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionDto {
    pub id: String,
    pub goal_id: String,
    /// None = orchestrator session.
    pub task_id: Option<String>,
    pub seat: Seat,
    /// The staffed agent this session runs; None for an orchestrator,
    /// which no task staffs.
    pub task_agent_id: Option<String>,
    /// Model requested at launch, `<agent>:<model>`: the registry agent the
    /// session runs on, and the model of it.
    pub model: String,
    /// Effort that model was launched at, off the same pin as `model`; null =
    /// whatever the agent runs it at.
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// The ACP agent's own session id.
    pub internal_session_id: Option<String>,
    pub worktree_path: Option<String>,
    pub status: SessionStatus,
    /// Why this session needs the user's attention, if it does. Orthogonal to
    /// `status`: an agent blocked on a permission prompt is still running.
    pub attention_reason: Option<AttentionReason>,
    /// When the current `attention_reason` was first raised.
    pub attention_since: Option<String>,
    pub last_activity_at: Option<String>,
    /// What this session's agent has spent, summed over every transcript it
    /// reported under. Zeros while nothing has been reported.
    pub usage: TokenUsageDto,
    pub created_at: String,
    pub ended_at: Option<String>,
}

/// A stored session of an ACP agent that Ariadne did not start, listed over
/// `session/list`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OutsideSessionDto {
    /// Which ACP registry agent this session belongs to (`GET
    /// /v1/acp-agents`).
    pub agent_id: String,
    /// The id the agent loads this conversation back by.
    pub internal_session_id: String,
    pub working_directory: String,
    pub last_activity_at: String,
    pub first_prompt: String,
}

/// Query of `GET /v1/outside-sessions`: what the daemon's snapshot of the
/// outside sessions is narrowed to, and which page of it is wanted.
///
/// The snapshot is taken on the first request, again on `refresh=true`, and
/// again when it is older than a minute; nothing here asks an agent otherwise.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct OutsideSessionListQuery {
    /// Only sessions of this registry agent (`GET /v1/acp-agents`).
    pub agent: Option<String>,
    /// Only sessions whose working directory is this absolute path, or a
    /// path under it.
    pub dir: Option<String>,
    /// Only sessions last active at or after this moment, RFC 3339.
    pub since: Option<String>,
    /// Only sessions last active at or before this moment, RFC 3339.
    pub until: Option<String>,
    /// Only sessions whose first prompt contains this text, case-insensitive.
    pub q: Option<String>,
    /// Max sessions in the page (default 50, cap 200).
    pub limit: Option<usize>,
    /// The `next_cursor` of the page before this one; opaque.
    pub cursor: Option<String>,
    /// Ask every agent again before answering, whatever the snapshot's age.
    pub refresh: Option<bool>,
}

impl OutsideSessionListQuery {
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

/// One page of `GET /v1/outside-sessions`, newest activity first.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OutsideSessionPageDto {
    pub sessions: Vec<OutsideSessionDto>,
    /// The `cursor` that continues after this page; null on the last one.
    pub next_cursor: Option<String>,
    /// How many sessions the filters leave, over every page.
    pub total: usize,
    /// When the snapshot this page was cut from was taken, RFC 3339.
    pub snapshot_at: String,
}

/// The stored ACP session to assign to an existing ready task.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssignOutsideSessionRequest {
    /// Which registry agent the session belongs to.
    pub agent_id: String,
    pub internal_session_id: String,
}

/// Adopt one outside session into a new task and goal, or an active goal.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdoptOutsideSessionRequest {
    /// Which registry agent the session belongs to.
    pub agent_id: String,
    pub internal_session_id: String,
    pub goal: OutsideSessionGoal,
    /// Omitted uses the session's first prompt, cut at 120 characters.
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    /// Id of one of the goal's repositories. Omit it when one repository or
    /// the session's working directory settles the choice.
    pub repo_id: Option<String>,
    /// The author first, then the reviewers.
    pub agents: Vec<AgentAssignment>,
    #[serde(default)]
    pub landing: Option<Landing>,
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
}

/// The goal that receives an adopted outside session.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum OutsideSessionGoal {
    Existing(ExistingOutsideSessionGoal),
    New(NewOutsideSessionGoal),
}

/// An active goal that receives the new task.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExistingOutsideSessionGoal {
    pub id: String,
}

/// A new active, unorchestrated goal for the adopted session.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewOutsideSessionGoal {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Registered repository ids. Omitted infers one from the session's
    /// working directory.
    pub repository_ids: Option<Vec<String>>,
}

/// The resources created or joined by an outside-session adoption.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AdoptOutsideSessionResponse {
    pub goal: GoalDto,
    pub task: TaskDto,
    pub session: SessionDto,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct SessionListQuery {
    /// Filter by goal id.
    pub goal: Option<String>,
    /// Filter by task id.
    pub task: Option<String>,
    /// Filter by status.
    pub status: Option<SessionStatus>,
    /// Only sessions currently flagged as needing attention.
    pub attention: Option<bool>,
}

/// Body of `POST /v1/sessions/{id}/console/input`.
///
/// While a permission request is pending, the text selects that request's
/// option; otherwise it becomes a fresh `session/prompt`, sent at once or
/// queued behind the turn still running.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsoleInputRequest {
    pub text: String,
}
