//! Agent-session DTOs.

use ariadne_core::{AgentKind, Role, SessionStatus};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionDto {
    pub id: String,
    pub goal_id: String,
    /// None = planner session.
    pub task_id: Option<String>,
    pub role: Role,
    pub profile_id: String,
    pub agent_kind: AgentKind,
    /// Agent-internal id: claude session uuid / codex thread id / opencode session id.
    pub internal_session_id: Option<String>,
    pub tmux_session: String,
    pub worktree_path: Option<String>,
    pub review_round: Option<i64>,
    pub status: SessionStatus,
    pub last_activity_at: Option<String>,
    pub created_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct SessionListQuery {
    /// Filter by goal id.
    pub goal: Option<String>,
    /// Filter by task id.
    pub task: Option<String>,
    /// Filter by status.
    pub status: Option<SessionStatus>,
}

/// Response of `GET /v1/sessions/{id}/logs`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionLogsResponse {
    pub session_id: String,
    pub tmux_session: String,
    /// Recent pane contents captured from tmux.
    pub logs: String,
}

/// Body of `POST /v1/sessions/{id}/input`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionInputRequest {
    /// Keystrokes to type into the pane, exactly as the terminal produced
    /// them: `\r` for Return, `\x03` for Ctrl-C, `\x1b[A` for Up. Sent
    /// verbatim — nothing is appended, so a submit has to carry its own `\r`.
    pub data: String,
}

/// Payload of the `snapshot` and `delta` events of
/// `GET /v1/sessions/{id}/logs/stream`.
///
/// Terminal output is raw bytes — newlines, escape sequences, control
/// characters — none of which survive SSE's line-oriented `data:` framing on
/// their own, so every chunk travels as JSON.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionLogChunk {
    /// Terminal output as written, decoded lossily from UTF-8.
    pub chunk: String,
}

/// Payload of the final `end` event of `GET /v1/sessions/{id}/logs/stream`:
/// the session is over and no further output is coming.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionLogEnd {
    pub session_id: String,
}
