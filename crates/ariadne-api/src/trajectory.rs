//! A session's trajectory: its events folded into typed turns (040).
//!
//! `GET /v1/sessions/{id}/trajectory` answers a [`TrajectorySnapshotDto`],
//! and `/trajectory/stream` opens on one and follows it with
//! [`TrajectoryUpdateDto`] frames. Every item names the stored event that
//! opened it, so an update finds what it changes by id.

use ariadne_core::{AttentionReason, SessionStatus};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::usage::TokenUsageDto;

/// The lane of the session's own agent. A subagent's lane is its id.
pub const MAIN_LANE: &str = "main";

/// Query of `GET /v1/sessions/{id}/trajectory`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TrajectoryQuery {
    /// A turn id: answer the whole turns older than it, newest last.
    #[serde(default)]
    pub before: Option<String>,
    /// How many turns to answer: 20 when omitted, at most 50.
    #[serde(default)]
    pub limit: Option<u32>,
    /// A subagent id: each turn carries that subagent's steps alone.
    #[serde(default)]
    pub subagent: Option<String>,
}

/// Query of `GET /v1/sessions/{id}/trajectory/stream`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TrajectoryStreamQuery {
    /// How many turns the snapshot carries: 20 when omitted, at most 50.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Query of `GET /v1/sessions/{id}/trajectory/turns/{turn_id}/steps`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TrajectoryStepsQuery {
    /// A step id: answer the steps older than it.
    #[serde(default)]
    pub before: Option<String>,
    /// How many steps to answer: 50 when omitted, at most 200.
    #[serde(default)]
    pub limit: Option<u32>,
    /// A subagent id: page that subagent's steps in the turn.
    #[serde(default)]
    pub subagent: Option<String>,
}

/// A session's trajectory: what `GET /trajectory` answers, and the
/// `snapshot` frame of `/trajectory/stream`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TrajectorySnapshotDto {
    /// The revision this snapshot stands at. The first update of a stream
    /// carries the next one.
    pub rev: u64,
    pub session: SessionStateDto,
    /// Oldest first.
    pub turns: Vec<TurnDto>,
    /// Whether older turns exist: `?before=<turns[0].id>` reads them.
    pub has_older: bool,
    pub subagents: Vec<SubagentStepDto>,
    pub plans: Vec<PlanDto>,
}

/// The state of the session a trajectory belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SessionStateDto {
    pub status: SessionStatus,
    pub attention_reason: Option<AttentionReason>,
    /// The turn that runs now, if one does.
    pub running_turn_id: Option<String>,
    /// The title the agent gave the session, where it gave one.
    pub title: Option<String>,
    pub mode: Option<ModeStateDto>,
    pub config_options: Vec<ConfigOptionDto>,
    pub context: Option<ContextWindowDto>,
    pub usage: TokenUsageDto,
    pub cost: Option<CostDto>,
    /// The slash commands the agent offers now.
    pub commands: Vec<CommandDto>,
}

/// The session's mode and the modes the agent offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModeStateDto {
    pub current_id: Option<String>,
    pub available: Vec<ModeDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ModeDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// One ACP session config option, such as the model or the effort.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ConfigOptionDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// The ACP category, such as `model` or `thought_level`.
    pub category: Option<String>,
    pub current_value: Option<String>,
    pub values: Vec<ConfigValueDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ConfigValueDto {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

/// The context window position the agent last reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContextWindowDto {
    pub used: u64,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CostDto {
    pub amount: f64,
    pub currency: String,
}

/// A slash command the agent offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CommandDto {
    /// The name, without its `/`.
    pub name: String,
    pub description: String,
    /// What its argument is, where the agent says.
    pub hint: Option<String>,
}

/// One turn: a prompt and everything the agent did for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TurnDto {
    /// The id of the stored event that opened the turn: its prompt, or the
    /// first event of a session before its first prompt.
    pub id: String,
    pub lane: String,
    /// None for the events a session has before its first prompt.
    pub prompt: Option<PromptDto>,
    pub state: TurnState,
    /// The ACP stop reason, once a `stop` ended the turn.
    pub stop_reason: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub activity: ActivityDto,
    /// The last steps of a running turn, oldest first. A closed turn
    /// carries none.
    pub steps: Vec<StepDto>,
    /// How many steps come before `steps`: the steps route pages them.
    pub earlier_steps: u64,
    /// What waits on a person, and the errors, whatever the state of the
    /// turn: the pending permissions and open questions the read loaded,
    /// newest first, then the newest 20 errors, newest first.
    pub attention: Vec<StepDto>,
    /// How many attention steps of the turn `attention` does not hold: the
    /// waiting steps past the read's limit, and the errors past 20. The
    /// steps route of the turn returns each of them.
    pub attention_more: u64,
    /// The last message of the session's own agent in the turn.
    pub answer: Vec<ContentBlockDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PromptDto {
    pub id: String,
    /// `console` or `daemon`.
    pub source: Option<String>,
    /// The lifecycle kind of a daemon prompt, such as `agent_resume`.
    pub prompt_kind: Option<String>,
    pub content: Vec<ContentBlockDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Running,
    Completed,
    Cancelled,
    Failed,
    /// The agent went away before the turn ended.
    Interrupted,
}

/// What a turn or a subagent did, counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ActivityDto {
    /// Tool calls and subagents.
    pub steps: u64,
    pub tool_calls: u64,
    /// The distinct paths of completed edit, delete and move calls.
    pub files_changed: u64,
    /// Failed calls.
    pub failed: u64,
    /// Cancelled calls.
    pub cancelled: u64,
    pub thoughts: u64,
    pub subagents: u64,
}

/// One step of a turn, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepDto {
    Message(MessageStepDto),
    Thought(ThoughtStepDto),
    ToolCall(ToolCallStepDto),
    Permission(PermissionStepDto),
    Question(QuestionStepDto),
    Note(NoteStepDto),
    Error(ErrorStepDto),
    Subagent(SubagentStepDto),
    Compaction(CompactionStepDto),
}

/// Text the agent wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MessageStepDto {
    pub id: String,
    pub turn_id: String,
    pub lane: String,
    pub created_at: String,
    pub content: Vec<ContentBlockDto>,
    /// Still taking text: `text_append` ops extend it.
    pub streaming: bool,
    /// The ACP `messageId`, where the agent sent one.
    pub message_id: Option<String>,
}

/// Text the agent thought.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ThoughtStepDto {
    pub id: String,
    pub turn_id: String,
    pub lane: String,
    pub created_at: String,
    pub content: Vec<ContentBlockDto>,
    pub streaming: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ToolCallStepDto {
    pub id: String,
    pub turn_id: String,
    pub lane: String,
    pub created_at: String,
    pub tool_call_id: String,
    pub title: String,
    /// The ACP kind: `read`, `edit`, `delete`, `move`, `search`, `execute`,
    /// `think`, `fetch`, `switch_mode` or `other`.
    pub kind: Option<String>,
    pub status: ToolCallStatus,
    /// What the call is about: the command, the path, the pattern or the URL.
    pub subject: Option<String>,
    pub locations: Vec<LocationDto>,
    /// The start of what the call gave back.
    pub preview: Option<String>,
    pub diff_stat: Option<DiffStatDto>,
    pub started_at: String,
    pub ended_at: Option<String>,
    /// The id of the permission step that asked about this call.
    pub permission: Option<String>,
    /// Grows each time the call's detail changes.
    pub detail_rev: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    /// The turn ended, or the agent went away, before the call did.
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LocationDto {
    pub path: String,
    pub line: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DiffStatDto {
    pub files: u64,
    pub added: u64,
    pub removed: u64,
}

/// A permission the agent asked for a tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PermissionStepDto {
    pub id: String,
    pub turn_id: String,
    pub lane: String,
    pub created_at: String,
    pub tool_call_id: String,
    pub tool: PermissionToolDto,
    pub question: String,
    pub options: Vec<PermissionOptionDto>,
    pub state: PermissionState,
    pub answer: Option<PermissionAnswerDto>,
    /// Why the AI permission model had a part, where it did.
    pub ai: Option<String>,
    /// The learned rule that answered, as `command` or `family <key>`.
    pub learned: Option<String>,
    /// The subagent that asks, where one does.
    pub subagent: Option<SubagentRefDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PermissionToolDto {
    pub title: String,
    pub kind: Option<String>,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PermissionOptionDto {
    pub id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once` or `reject_always`.
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Pending,
    Answered,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PermissionAnswerDto {
    pub option_id: Option<String>,
    /// The name of the option chosen, or `cancelled`.
    pub label: String,
    /// `console`, `ai`, `learned` or `auto`.
    pub decided_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SubagentRefDto {
    pub id: String,
    pub title: String,
}

/// A question the agent asked through `wait_for_user` (031).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct QuestionStepDto {
    pub id: String,
    pub turn_id: String,
    pub lane: String,
    pub created_at: String,
    pub request_id: String,
    pub hint: String,
    pub question: String,
    pub options: Vec<QuestionOptionDto>,
    pub recommended: Option<String>,
    pub recommendation_reason: Option<String>,
    pub state: QuestionState,
    /// The answer, once it was answered.
    pub answer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct QuestionOptionDto {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    Open,
    /// The person answered with the form content.
    Answered,
    /// The person refused to answer.
    Declined,
    /// The question closed without an answer: dismissed, cancelled with its
    /// turn, or withdrawn.
    Cancelled,
}

/// A line the daemon says about the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NoteStepDto {
    pub id: String,
    pub turn_id: String,
    pub lane: String,
    pub created_at: String,
    /// The kind of the event the note is about.
    pub origin: String,
    pub severity: NoteSeverity,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteSeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErrorStepDto {
    pub id: String,
    pub turn_id: String,
    pub lane: String,
    pub created_at: String,
    pub text: String,
    /// The advisory diagnosis of the error (024), once one arrived.
    pub diagnosis: Option<String>,
}

/// A subagent the agent started, in the turn of its parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SubagentStepDto {
    pub id: String,
    pub turn_id: String,
    pub lane: String,
    pub created_at: String,
    pub subagent_id: String,
    pub title: String,
    pub task: Option<String>,
    /// `running`, `completed`, `failed` or `cancelled`, where the agent says.
    pub state: Option<String>,
    pub can_cancel: bool,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub activity: ActivityDto,
}

/// The agent compacting its context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CompactionStepDto {
    pub id: String,
    pub turn_id: String,
    pub lane: String,
    pub created_at: String,
    pub compaction_id: String,
    pub status: CompactionStatus,
    pub summary: Vec<ContentBlockDto>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStatus {
    Running,
    Completed,
    Failed,
}

/// A plan of the agent or of a subagent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlanDto {
    /// The id of the stored event that opened the plan.
    pub plan_id: String,
    pub variant: PlanVariant,
    pub entries: Vec<PlanEntryDto>,
    pub markdown: Option<String>,
    /// The file the plan is written in, for a `markdown` plan.
    pub path: Option<String>,
    /// The turn that last changed the plan.
    pub turn_id: Option<String>,
    pub removed: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanVariant {
    Entries,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlanEntryDto {
    pub content: String,
    pub status: String,
    pub priority: Option<String>,
}

/// One ACP content block. A block with bytes carries a `blob` URL, never
/// the bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockDto {
    Text {
        text: String,
    },
    Image {
        mime_type: Option<String>,
        blob: Option<String>,
        uri: Option<String>,
    },
    Audio {
        mime_type: Option<String>,
        blob: Option<String>,
    },
    Resource {
        uri: String,
        mime_type: Option<String>,
        text: Option<String>,
        blob: Option<String>,
    },
    ResourceLink {
        uri: String,
        name: String,
        title: Option<String>,
        mime_type: Option<String>,
        size: Option<u64>,
    },
}

/// One `update` frame of `/trajectory/stream`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TrajectoryUpdateDto {
    /// One more than the revision before it.
    pub rev: u64,
    pub ops: Vec<TrajectoryOpDto>,
}

/// One change of a trajectory, tagged by `op`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TrajectoryOpDto {
    /// A turn opened or changed. `steps` is empty: the steps change by
    /// `item_upsert`. A closed turn drops the steps a client holds for it.
    TurnUpsert {
        turn: TurnDto,
    },
    /// A step opened or changed, in the turn and lane it names.
    ItemUpsert {
        step: StepDto,
    },
    /// Text streamed onto the end of a `message` or `thought` step.
    TextAppend {
        turn_id: String,
        item_id: String,
        text: String,
    },
    /// A step is gone.
    ItemRemove {
        turn_id: String,
        item_id: String,
    },
    PlanUpsert {
        plan: PlanDto,
    },
    SubagentUpsert {
        subagent: SubagentStepDto,
    },
    /// The session's state, whole.
    SessionPatch {
        session: SessionStateDto,
    },
}

/// One page of the steps of a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TrajectoryStepsDto {
    /// Oldest first.
    pub steps: Vec<StepDto>,
    /// How many steps come before these.
    pub earlier_steps: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A step and an op are tagged on the wire, so a client tells them apart
    /// by one field.
    #[test]
    fn steps_and_ops_are_tagged_on_the_wire() {
        let op = TrajectoryOpDto::TextAppend {
            turn_id: "t".into(),
            item_id: "i".into(),
            text: "more".into(),
        };
        assert_eq!(
            serde_json::to_value(&op).unwrap(),
            serde_json::json!({"op": "text_append", "turn_id": "t", "item_id": "i", "text": "more"})
        );
        let step = StepDto::Note(NoteStepDto {
            id: "n".into(),
            turn_id: "t".into(),
            lane: MAIN_LANE.into(),
            created_at: "2026-11-26T00:00:00Z".into(),
            origin: "session_start".into(),
            severity: NoteSeverity::Info,
            text: "session started".into(),
        });
        assert_eq!(serde_json::to_value(&step).unwrap()["type"], "note");
    }
}
