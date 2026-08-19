//! Agent-session DTOs.

use ariadne_core::{AgentKind, AttentionReason, Role, SessionStatus};
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
    /// Model requested at launch; null = the agent CLI's default.
    pub model: Option<String>,
    /// Agent-internal id: claude session uuid / codex thread id / opencode session id.
    pub internal_session_id: Option<String>,
    pub tmux_session: String,
    pub worktree_path: Option<String>,
    pub review_round: Option<i64>,
    pub status: SessionStatus,
    /// Why this session needs the user's attention, if it does. Orthogonal to
    /// `status`: an agent blocked on a permission prompt is still running.
    pub attention_reason: Option<AttentionReason>,
    /// When the current `attention_reason` was first raised.
    pub attention_since: Option<String>,
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
    /// Only sessions currently flagged as needing attention.
    pub attention: Option<bool>,
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

/// Payload of the `resize` event of `GET /v1/sessions/{id}/logs/stream`: the
/// grid the pane is drawing against, in cells.
///
/// A terminal stream only means anything at a size. The agent addresses the
/// cursor and erases lines against *this* grid, so a viewer that renders the
/// bytes at any other one has every repaint land on the wrong row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SessionPaneSize {
    pub cols: u16,
    pub rows: u16,
}

/// Payload of the final `end` event of `GET /v1/sessions/{id}/logs/stream`:
/// the session is over and no further output is coming.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionLogEnd {
    pub session_id: String,
}
