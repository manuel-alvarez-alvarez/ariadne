//! Agent-session DTOs.

use ariadne_core::{AttentionReason, Seat, SessionStatus};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::usage::TokenUsageDto;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionDto {
    pub id: String,
    pub goal_id: Option<String>,
    /// None for an orchestrator or a loose session.
    pub task_id: Option<String>,
    pub seat: Option<Seat>,
    /// The staffed agent this session runs; None for an orchestrator or loose session.
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
    /// The worktree, or the recorded working directory of a loose session.
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
    /// The context window position the agent most recently reported. Both
    /// fields stay null until the agent sends a `usage_update`.
    pub context_used: Option<u64>,
    pub context_size: Option<u64>,
    pub created_at: String,
    pub ended_at: Option<String>,
    /// A loose session's own title: the first prompt of the conversation it
    /// resumed, or the first one typed into it. Null on a task's or a goal's
    /// session, which goes by its work's title.
    pub title: Option<String>,
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

/// Resume a stored conversation without a goal, task or seat.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResumeOutsideSessionRequest {
    pub agent_id: String,
    pub internal_session_id: String,
}

/// Which half of the listing a row came from: a session Ariadne runs, or a
/// conversation an ACP agent stored that Ariadne did not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Ariadne,
    Outside,
}

/// One row of `GET /v1/sessions`: a session of either kind.
///
/// An outside conversation carries no goal, task, seat or status, because
/// Ariadne runs no work behind it; what it does carry is the agent it belongs
/// to, the id that agent loads it back by, where it ran and when it last did.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionEntryDto {
    pub kind: SessionKind,
    /// Ariadne's session id, or the agent's own session id on an outside row.
    pub id: String,
    /// The registry agent this conversation runs on (`GET /v1/acp-agents`).
    pub agent_id: String,
    /// What this session is about: its task's title, the goal's title behind
    /// an orchestrator, or the first prompt of an outside conversation.
    pub title: Option<String>,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    pub seat: Option<Seat>,
    /// The staffed agent this session runs; None for an orchestrator, a loose
    /// session or an outside one.
    pub task_agent_id: Option<String>,
    /// Model requested at launch, `<agent>:<model>`; None on an outside row.
    pub model: Option<String>,
    /// Effort that model was launched at, off the same pin as `model`.
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// The ACP agent's own session id, which is `id` on an outside row.
    pub internal_session_id: Option<String>,
    /// The worktree, or the recorded working directory.
    pub working_directory: Option<String>,
    /// None on an outside row: Ariadne runs no process behind it.
    pub status: Option<SessionStatus>,
    pub attention_reason: Option<AttentionReason>,
    pub attention_since: Option<String>,
    /// When this session was last active, which the page is ordered by.
    pub last_activity_at: Option<String>,
    /// What this session's agent has spent; None on an outside row.
    pub usage: Option<TokenUsageDto>,
    pub context_used: Option<u64>,
    pub context_size: Option<u64>,
    /// When Ariadne created the row; None on an outside row.
    pub created_at: Option<String>,
    pub ended_at: Option<String>,
}

/// One page of `GET /v1/sessions`, newest activity first.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionPageDto {
    pub sessions: Vec<SessionEntryDto>,
    /// The `cursor` that continues after this page; null on the last one.
    pub next_cursor: Option<String>,
    /// How many sessions the filters leave, over every page.
    pub total: usize,
    /// When the outside snapshot this page was cut from was taken, RFC 3339.
    pub snapshot_at: String,
}

/// Query of the session listing the CLI still sends. `GET /v1/sessions`
/// takes a [`SessionPageQuery`] now, and the CLI moves onto it in a task of
/// its own.
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

/// Query of `GET /v1/sessions`: what the listing of both kinds is narrowed
/// to, and which page of it is wanted.
///
/// The outside half is served off one snapshot, taken on the first request,
/// again on `refresh=true`, and again when it is older than a minute; nothing
/// here asks an agent otherwise.
#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct SessionPageQuery {
    /// Only sessions of this kind. Omitted lists both.
    pub kind: Option<SessionKind>,
    /// Only sessions of this registry agent (`GET /v1/acp-agents`).
    pub agent: Option<String>,
    /// Filter by goal id. No outside session has one.
    pub goal: Option<String>,
    /// Filter by task id. No outside session has one.
    pub task: Option<String>,
    /// Filter by status. No outside session has one. A named status also
    /// lists sessions that have ended, as `all` does.
    pub status: Option<SessionStatus>,
    /// Filter by seat. No outside session has one.
    pub seat: Option<Seat>,
    /// Only sessions currently flagged as needing attention. No outside
    /// session is.
    pub attention: Option<bool>,
    /// Only sessions whose directory is this absolute path, or a path under
    /// it.
    pub dir: Option<String>,
    /// Only sessions last active at or after this moment, RFC 3339.
    pub since: Option<String>,
    /// Only sessions last active at or before this moment, RFC 3339.
    pub until: Option<String>,
    /// Only sessions whose title contains this text, case-insensitive.
    pub q: Option<String>,
    /// List every session, whatever its age and whether it has ended, rather
    /// than the live ones of the last 7 days.
    pub all: Option<bool>,
    /// Max sessions in the page (default 50, cap 200).
    pub limit: Option<usize>,
    /// The `next_cursor` of the page before this one; opaque.
    pub cursor: Option<String>,
    /// Ask every agent again before answering, whatever the snapshot's age.
    pub refresh: Option<bool>,
}

impl SessionPageQuery {
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
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

/// What a client sends over `GET /v1/sessions/{id}/console/terminal`, the
/// session's console as a terminal: one of these per JSON text frame.
///
/// The daemon draws the console into the terminal the client emulates, so
/// the first message is the terminal's size, and every change of it is
/// another. A key and a paste are what the terminal read, as the console
/// takes them; the daemon reads nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TerminalClientMessage {
    /// The size of the terminal the bytes are drawn on, in cells. Sent
    /// first, before the daemon draws anything, and on every change.
    Resize { cols: u16, rows: u16 },
    /// One key press: what crossterm's `KeyEvent` holds, one to one.
    Key {
        code: TerminalKey,
        #[serde(default)]
        modifiers: Vec<TerminalModifier>,
    },
    /// Text pasted whole, line breaks and all.
    Paste { text: String },
}

/// The key a [`TerminalClientMessage::Key`] names: crossterm's `KeyCode`,
/// as far as the console reads it. A printable character is
/// `{"char": "a"}`, a function key `{"f": 5}`, and every other key its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKey {
    Char(char),
    F(u8),
    Enter,
    Backspace,
    Tab,
    BackTab,
    Esc,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
}

/// A modifier held with a key: crossterm's `KeyModifiers`, one flag each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalModifier {
    Shift,
    Control,
    Alt,
    Super,
    Hyper,
    Meta,
}

/// What the daemon sends over `GET /v1/sessions/{id}/console/terminal` as a
/// JSON text frame, beside the binary frames of terminal bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalServerMessage {
    /// The session's status: once as the socket opens, and again as the
    /// console ends, right before the socket closes.
    Status { status: SessionStatus },
}
