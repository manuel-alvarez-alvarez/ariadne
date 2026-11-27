//! What a step is made of: the parts of an event's payload that a step of
//! the trajectory shows (040).

use serde_json::Value;

use ariadne_api::events::AgentEventDto;
use ariadne_api::trajectory::{
    CommandDto, ContentBlockDto, DiffStatDto, LocationDto, MessageStepDto, StepDto, ThoughtStepDto,
    ToolCallStatus, ToolCallStepDto,
};

/// What a tool call reads as when its event names no tool.
pub(super) const UNNAMED_TOOL: &str = "ACP tool";

/// How much of a call's output its step previews: five lines, at most 400
/// characters. A row of a trajectory shows that much; the whole output is
/// the item detail, which a later route serves.
const PREVIEW_CHARS: usize = 400;
const PREVIEW_LINES: usize = 5;

pub(crate) fn step_id(step: &StepDto) -> &str {
    match step {
        StepDto::Message(step) => &step.id,
        StepDto::Thought(step) => &step.id,
        StepDto::ToolCall(step) => &step.id,
        StepDto::Permission(step) => &step.id,
        StepDto::Question(step) => &step.id,
        StepDto::Note(step) => &step.id,
        StepDto::Error(step) => &step.id,
        StepDto::Subagent(step) => &step.id,
        StepDto::Compaction(step) => &step.id,
    }
}

pub(super) fn lane_of(step: &StepDto) -> &str {
    match step {
        StepDto::Message(step) => &step.lane,
        StepDto::Thought(step) => &step.lane,
        StepDto::ToolCall(step) => &step.lane,
        StepDto::Permission(step) => &step.lane,
        StepDto::Question(step) => &step.lane,
        StepDto::Note(step) => &step.lane,
        StepDto::Error(step) => &step.lane,
        StepDto::Subagent(step) => &step.lane,
        StepDto::Compaction(step) => &step.lane,
    }
}

pub(super) fn text_step(
    thought: bool,
    id: String,
    turn_id: String,
    lane: &str,
    event: &AgentEventDto,
    content: Vec<ContentBlockDto>,
    streaming: bool,
) -> StepDto {
    if thought {
        StepDto::Thought(ThoughtStepDto {
            id,
            turn_id,
            lane: lane.to_string(),
            created_at: event.created_at.clone(),
            content,
            streaming,
        })
    } else {
        StepDto::Message(MessageStepDto {
            id,
            turn_id,
            lane: lane.to_string(),
            created_at: event.created_at.clone(),
            content,
            streaming,
            message_id: string_at(&event.payload, "/message_id"),
        })
    }
}

pub(super) fn append_text(content: &mut Vec<ContentBlockDto>, text: &str) {
    match content.last_mut() {
        Some(ContentBlockDto::Text { text: last }) => last.push_str(text),
        _ => content.push(ContentBlockDto::Text {
            text: text.to_string(),
        }),
    }
}

/// The content an event carries: its ACP `content` blocks, else its `text`.
pub(super) fn content_of(
    payload: &Value,
    session_id: &str,
    event_id: &str,
) -> Vec<ContentBlockDto> {
    if let Some(blocks) = payload.get("content").and_then(Value::as_array) {
        return blocks_of(blocks, session_id, event_id);
    }
    string_at(payload, "/text")
        .map(|text| vec![ContentBlockDto::Text { text }])
        .unwrap_or_default()
}

/// ACP content blocks, with every byte left behind a `blob` URL. A stored
/// run keeps one block per chunk (021, rule 6), and a live run appends its
/// chunks to one: adjacent text blocks read as one, so both are the same.
/// A block too large to store reads by its placeholder's `mime_type`.
pub(super) fn blocks_of(
    blocks: &[Value],
    session_id: &str,
    event_id: &str,
) -> Vec<ContentBlockDto> {
    let blob =
        |index: usize| format!("/v1/sessions/{session_id}/trajectory/blobs/{event_id}/{index}");
    let mut out: Vec<ContentBlockDto> = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let has_data = |pointer: &str| block.pointer(pointer).is_some_and(|data| !data.is_null());
        let mime =
            |pointer: &str| string_at(block, pointer).or_else(|| string_at(block, "/mime_type"));
        let Some(kind) = string_at(block, "/type") else {
            continue;
        };
        let next = match kind.as_str() {
            "text" => {
                let text = string_at(block, "/text").unwrap_or_default();
                if let Some(ContentBlockDto::Text { text: last }) = out.last_mut() {
                    last.push_str(&text);
                    continue;
                }
                ContentBlockDto::Text { text }
            }
            "image" => ContentBlockDto::Image {
                mime_type: mime("/mimeType"),
                blob: has_data("/data").then(|| blob(index)),
                uri: string_at(block, "/uri"),
            },
            "audio" => ContentBlockDto::Audio {
                mime_type: mime("/mimeType"),
                blob: has_data("/data").then(|| blob(index)),
            },
            "resource" => ContentBlockDto::Resource {
                uri: string_at(block, "/resource/uri").unwrap_or_default(),
                mime_type: mime("/resource/mimeType"),
                text: string_at(block, "/resource/text"),
                blob: has_data("/resource/blob").then(|| blob(index)),
            },
            "resource_link" => ContentBlockDto::ResourceLink {
                uri: string_at(block, "/uri").unwrap_or_default(),
                name: string_at(block, "/name").unwrap_or_default(),
                title: string_at(block, "/title"),
                mime_type: mime("/mimeType"),
                size: block.get("size").and_then(Value::as_u64),
            },
            _ => continue,
        };
        out.push(next);
    }
    out
}

pub(super) fn new_call(
    event: &AgentEventDto,
    turn_id: &str,
    lane: &str,
    tool_call_id: &str,
) -> ToolCallStepDto {
    ToolCallStepDto {
        id: event.id.clone(),
        turn_id: turn_id.to_string(),
        lane: lane.to_string(),
        created_at: event.created_at.clone(),
        tool_call_id: tool_call_id.to_string(),
        title: UNNAMED_TOOL.into(),
        kind: None,
        status: ToolCallStatus::Pending,
        subject: None,
        locations: Vec::new(),
        preview: None,
        diff_stat: None,
        started_at: event.created_at.clone(),
        ended_at: None,
        permission: None,
        detail_rev: 0,
    }
}

/// Take what an event of a call says, and keep what it does not say again.
pub(super) fn merge_call(call: &mut ToolCallStepDto, event: &AgentEventDto) {
    let payload = &event.payload;
    let acp = payload.get("acp").unwrap_or(payload);
    if let Some(title) = string_at(payload, "/tool_name").or_else(|| string_at(acp, "/title"))
        && title != UNNAMED_TOOL
    {
        call.title = title;
    }
    if let Some(kind) = string_at(acp, "/kind") {
        call.kind = Some(kind);
    }
    let mut locations = locations_of(acp);
    let diffs = diffs_of(acp);
    if locations.is_empty() {
        locations = diffs
            .iter()
            .filter_map(|(path, _, _)| path.clone())
            .map(|path| LocationDto { path, line: None })
            .collect();
    }
    if !locations.is_empty() {
        call.locations = locations;
    }
    if !diffs.is_empty() {
        call.diff_stat = Some(DiffStatDto {
            files: diffs.len() as u64,
            added: diffs.iter().map(|(_, added, _)| added).sum(),
            removed: diffs.iter().map(|(_, _, removed)| removed).sum(),
        });
    }
    let input = payload
        .get("tool_input")
        .or_else(|| acp.get("rawInput"))
        .filter(|input| !input.is_null());
    if let Some(input) = input {
        call.subject = subject_of(call.kind.as_deref(), input, &call.locations);
    } else if call.subject.is_none() && !call.locations.is_empty() {
        call.subject = subject_of(call.kind.as_deref(), &Value::Null, &call.locations);
    }
    if let Some(preview) = preview_of(acp) {
        call.preview = Some(preview);
    }
    let status = match event.kind.as_str() {
        "tool_call_abandoned" => Some(match string_at(payload, "/reason").as_deref() {
            Some("cancelled") => ToolCallStatus::Cancelled,
            _ => ToolCallStatus::Interrupted,
        }),
        _ => match string_at(acp, "/status").as_deref() {
            Some("pending") => Some(ToolCallStatus::Pending),
            Some("in_progress") => Some(ToolCallStatus::Running),
            Some("completed") => Some(ToolCallStatus::Completed),
            Some("failed") => Some(ToolCallStatus::Failed),
            Some("cancelled") => Some(ToolCallStatus::Cancelled),
            _ => None,
        },
    };
    if let Some(status) = status {
        call.status = status;
    }
    if is_terminal(call.status) && call.ended_at.is_none() {
        call.ended_at = Some(event.created_at.clone());
    }
}

pub(super) fn is_terminal(status: ToolCallStatus) -> bool {
    !matches!(status, ToolCallStatus::Pending | ToolCallStatus::Running)
}

pub(super) fn locations_of(acp: &Value) -> Vec<LocationDto> {
    acp.get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| {
            Some(LocationDto {
                path: string_at(location, "/path")?,
                line: location.get("line").and_then(Value::as_u64),
            })
        })
        .collect()
}

/// Each diff a call carries: its path, and the lines it adds and removes.
/// A patch counts its `+` and `-` lines; an old and a new text count the
/// lines between their common start and their common end.
pub(super) fn diffs_of(acp: &Value) -> Vec<(Option<String>, u64, u64)> {
    acp.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| string_at(entry, "/type").as_deref() == Some("diff"))
        .map(|entry| {
            let path = string_at(entry, "/path");
            if let Some(patch) = string_at(entry, "/patch/text") {
                let lines = patch.lines();
                let added = lines
                    .clone()
                    .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
                    .count() as u64;
                let removed = lines
                    .filter(|line| line.starts_with('-') && !line.starts_with("---"))
                    .count() as u64;
                return (path, added, removed);
            }
            let old = string_at(entry, "/oldText").unwrap_or_default();
            let new = string_at(entry, "/newText").unwrap_or_default();
            let old: Vec<&str> = old.lines().collect();
            let new: Vec<&str> = new.lines().collect();
            let start = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
            let end = old[start..]
                .iter()
                .rev()
                .zip(new[start..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            (
                path,
                (new.len() - start - end) as u64,
                (old.len() - start - end) as u64,
            )
        })
        .collect()
}

/// The start of what a call gave back: the text of its content, else its
/// raw output.
pub(super) fn preview_of(acp: &Value) -> Option<String> {
    let text: Vec<String> = acp
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| string_at(entry, "/type").as_deref() == Some("content"))
        .filter_map(|entry| string_at(entry, "/content/text"))
        .filter(|text| !text.is_empty())
        .collect();
    let whole = if !text.is_empty() {
        text.join("\n")
    } else {
        match acp.get("rawOutput") {
            None | Some(Value::Null) => return None,
            Some(Value::String(text)) => text.clone(),
            Some(Value::Object(object))
                if object.contains_key("stdout") || object.contains_key("stderr") =>
            {
                ["stdout", "stderr"]
                    .into_iter()
                    .filter_map(|key| object.get(key)?.as_str())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Some(raw) => raw.to_string(),
        }
    };
    let lines: Vec<&str> = whole.lines().take(PREVIEW_LINES).collect();
    let preview: String = lines.join("\n").chars().take(PREVIEW_CHARS).collect();
    (!preview.is_empty()).then_some(preview)
}

/// What a call is about: the command, the path, the pattern in its path or
/// the URL, by its kind.
pub(super) fn subject_of(
    kind: Option<&str>,
    input: &Value,
    locations: &[LocationDto],
) -> Option<String> {
    let field = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| string_at(input, &format!("/{key}")))
    };
    let first = locations.first();
    match kind {
        Some("execute") => match input.get("command") {
            Some(Value::String(command)) => command.lines().next().map(str::to_string),
            Some(Value::Array(parts)) => Some(
                parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        },
        Some("read" | "edit" | "delete" | "move") => {
            let path = field(&["path", "file_path", "filePath", "file"])
                .or_else(|| first.map(|location| location.path.clone()))?;
            let line = input.get("line").and_then(Value::as_u64).or_else(|| {
                first
                    .filter(|location| location.path == path)
                    .and_then(|location| location.line)
            });
            Some(match line {
                Some(line) => format!("{path}:{line}"),
                None => path,
            })
        }
        Some("search") => {
            let pattern = field(&["pattern", "query", "regex"])?;
            match field(&["path", "file_path", "directory", "cwd"])
                .or_else(|| first.map(|location| location.path.clone()))
            {
                Some(path) => Some(format!("{pattern} in {path}")),
                None => Some(pattern),
            }
        }
        Some("fetch") => field(&["url"]),
        _ => first.map(|location| location.path.clone()),
    }
}

pub(super) fn commands_of(payload: &Value) -> Vec<CommandDto> {
    payload
        .get("available_commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|command| {
            Some(CommandDto {
                name: string_at(command, "/name")?,
                description: string_at(command, "/description").unwrap_or_default(),
                hint: string_at(command, "/input/hint"),
            })
        })
        .collect()
}

pub(super) fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_string)
}
