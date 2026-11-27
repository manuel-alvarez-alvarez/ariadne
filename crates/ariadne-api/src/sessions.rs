//! Agent-session DTOs.

use ariadne_core::{AttentionReason, Seat, SessionStatus};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::usage::TokenUsageDto;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionDto {
    pub id: String,
    pub goal_id: Option<String>,
    /// None for an orchestrator or a PR session.
    pub task_id: Option<String>,
    pub seat: Option<Seat>,
    /// The staffed agent this session runs; None for an orchestrator or PR session.
    pub task_agent_id: Option<String>,
    /// The workflow column `task_agent_id` works, where the session still has
    /// one; null for an orchestrator, a PR session, or an agent its task
    /// no longer staffs.
    pub step: Option<String>,
    /// Model requested at launch, `<agent>:<model>`: the registry agent the
    /// session runs on, and the model of it.
    pub model: String,
    /// Effort that model was launched at, off the same pin as `model`; null =
    /// whatever the agent runs it at.
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// The ACP agent's own session id.
    pub internal_session_id: Option<String>,
    /// The worktree or repository working directory.
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
    /// The session this one replaced on its seat, when a switch started it
    /// (`POST /v1/sessions/{id}/switch`); null otherwise.
    pub switched_from: Option<String>,
    /// The pull request this session watches (026); null for every other
    /// session.
    pub pull_request_id: Option<String>,
}

/// Switch a session to another model or agent: the old session ends, and a
/// new one starts on the same seat, on this pin, in a new conversation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SwitchSessionRequest {
    /// The model to run, `<agent>:<model>`: a registry agent (`GET
    /// /v1/acp-agents`) and a model of it (`GET /v1/models`).
    #[schema(example = "claude-acp:sonnet")]
    pub model: String,
    /// The effort to run that model at; omitted = the agent's own.
    #[serde(default)]
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionEntryDto {
    /// Ariadne's session id.
    pub id: String,
    /// The registry agent this conversation runs on (`GET /v1/acp-agents`).
    pub agent_id: String,
    /// What this session is about: its task's title, the goal's title behind
    /// an orchestrator, or the PR review's title.
    pub title: Option<String>,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    pub seat: Option<Seat>,
    /// The staffed agent this session runs; None for an orchestrator or PR session.
    pub task_agent_id: Option<String>,
    /// Model requested at launch, `<agent>:<model>`.
    pub model: Option<String>,
    /// Effort that model was launched at, off the same pin as `model`.
    #[schema(example = "high")]
    pub effort: Option<String>,
    /// The ACP agent's own session id.
    pub internal_session_id: Option<String>,
    /// The worktree, or the recorded working directory.
    pub working_directory: Option<String>,
    pub status: Option<SessionStatus>,
    pub attention_reason: Option<AttentionReason>,
    pub attention_since: Option<String>,
    /// When this session was last active, which the page is ordered by.
    pub last_activity_at: Option<String>,
    /// What this session's agent has spent.
    pub usage: Option<TokenUsageDto>,
    pub context_used: Option<u64>,
    pub context_size: Option<u64>,
    /// When Ariadne created the row.
    pub created_at: Option<String>,
    pub ended_at: Option<String>,
    /// The pull request this session watches; None for every other session.
    pub pull_request_id: Option<String>,
}

/// One page of `GET /v1/sessions`, newest activity first.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionPageDto {
    pub sessions: Vec<SessionEntryDto>,
    /// The `cursor` that continues after this page; null on the last one.
    pub next_cursor: Option<String>,
    /// How many sessions the filters leave, over every page.
    pub total: usize,
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

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct SessionPageQuery {
    /// Only sessions of this registry agent (`GET /v1/acp-agents`).
    pub agent: Option<String>,
    /// Filter by goal id.
    pub goal: Option<String>,
    /// Filter by task id.
    pub task: Option<String>,
    /// Filter by pull request id.
    pub pull_request: Option<String>,
    /// Filter by status. A named status also
    /// lists sessions that have ended, as `all` does.
    pub status: Option<SessionStatus>,
    /// Filter by seat.
    pub seat: Option<Seat>,
    /// Only sessions currently flagged as needing attention.
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

/// One open question an agent asks with ACP `elicitation/create` in form
/// mode, as `GET /v1/sessions/{id}/questions` lists it (021).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct QuestionDto {
    pub id: String,
    /// The Ariadne session whose agent asks.
    pub session_id: String,
    /// The agent's own text over the form.
    pub message: String,
    /// The `requestedSchema` of the ACP request, unchanged.
    #[schema(value_type = Object)]
    pub schema: serde_json::Value,
    pub created_at: String,
}

/// What the person did with a question, as ACP names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuestionAction {
    /// The person answered with the form content.
    Accept,
    /// The person refused to answer.
    Decline,
    /// The person dismissed the question.
    Cancel,
}

impl QuestionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Decline => "decline",
            Self::Cancel => "cancel",
        }
    }
}

/// Body of `POST /v1/sessions/{id}/questions/{question_id}/answer`.
/// `content` is present with `accept` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AnswerQuestionRequest {
    pub action: QuestionAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub content: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Body of `POST /v1/sessions/{id}/console/cancel`. Omitted, or omitting
/// `subagent_id`, cancels the session's own running turn, as before (021).
/// Naming a child cancels that child's turn instead, and is refused with
/// `409` where the child is unknown, has already ended, or never advertised
/// the capability.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsoleCancelRequest {
    pub subagent_id: Option<String>,
}

/// What a client sends over `GET /v1/sessions/{id}/console/terminal`, the
/// session's console as a terminal: one of these per JSON text frame.
///
/// The daemon draws the console into the terminal the client emulates, so
/// the first message is the terminal's size, and every change of it is
/// another. A key, a paste and a mouse event are what the terminal read, as
/// the console takes them; the daemon reads nothing else. The console asks
/// the terminal for the mouse as it opens, so a terminal that reports it
/// sends `mouse` messages.
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
    /// One mouse event on the terminal's cell `col`, `row`, counted from
    /// zero at the top left: a step of the wheel, or a click of the left
    /// button.
    Mouse {
        kind: TerminalMouse,
        col: u16,
        row: u16,
    },
}

/// What a [`TerminalClientMessage::Mouse`] did: one step of the wheel up or
/// down, or a press of the left button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalMouse {
    WheelUp,
    WheelDown,
    Click,
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
