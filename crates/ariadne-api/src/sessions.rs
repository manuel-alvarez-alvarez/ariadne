//! Agent-session DTOs.

use ariadne_core::{AttentionReason, Seat, SessionStatus};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::usage::TokenUsageDto;

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

/// The stored ACP session to adopt as a task author.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AdoptOutsideSessionRequest {
    /// Which registry agent the session belongs to.
    pub agent_id: String,
    pub internal_session_id: String,
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
