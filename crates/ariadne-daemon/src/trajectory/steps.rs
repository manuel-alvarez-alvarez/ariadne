//! What a step is made of: the parts of an event's payload that a step of
//! the trajectory shows (040).

use serde_json::Value;

use ariadne_core::event_facts::{EventFacts, FactLocation, output_text};

use ariadne_api::events::AgentEventDto;
use ariadne_api::trajectory::{
    CommandDto, ConfigOptionDto, ConfigValueDto, ContentBlockDto, CostDto, DiffStatDto,
    LocationDto, MessageStepDto, ModeDto, ModeStateDto, PermissionOptionDto, SessionStateDto,
    StepDto, SubagentRefDto, ThoughtStepDto, ToolCallStatus, ToolCallStepDto,
};

/// What a tool call reads as when its event names no tool.
pub(super) const UNNAMED_TOOL: &str = "ACP tool";

/// How much of a call's output its step previews: five lines, at most 400
/// characters. A row of a trajectory shows that much; the whole output is
/// the item detail, which a later route serves.
const PREVIEW_CHARS: usize = 400;
const PREVIEW_LINES: usize = 5;

pub(super) fn subagent_ref(payload: &Value) -> Option<SubagentRefDto> {
    let id = string_at(payload, "/subagent/id")?;
    Some(SubagentRefDto {
        title: string_at(payload, "/subagent/title").unwrap_or_else(|| id.clone()),
        id,
    })
}

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
        for (path, _, _) in &diffs {
            if let Some(path) = path
                && !locations.iter().any(|location| &location.path == path)
            {
                locations.push(LocationDto {
                    path: path.clone(),
                    line: None,
                });
            }
        }
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

/// Take what the facts of a call event say, where its payload is not read:
/// the same title, kind, locations, subject and status `merge_call` takes.
pub(super) fn merge_call_facts(call: &mut ToolCallStepDto, facts: &EventFacts, created_at: &str) {
    if let Some(title) = &facts.title
        && title != UNNAMED_TOOL
    {
        call.title = title.clone();
    }
    if let Some(kind) = &facts.tool_kind {
        call.kind = Some(kind.clone());
    }
    let locations: Vec<LocationDto> = if facts.locations.is_empty() {
        facts
            .paths
            .iter()
            .map(|path| LocationDto {
                path: path.clone(),
                line: None,
            })
            .collect()
    } else {
        facts
            .locations
            .iter()
            .map(|location: &FactLocation| LocationDto {
                path: location.path.clone(),
                line: location.line,
            })
            .collect()
    };
    if !locations.is_empty() {
        call.locations = locations;
    }
    if let Some(input) = &facts.input {
        call.subject = subject_of(call.kind.as_deref(), input, &call.locations);
    } else if call.subject.is_none() && !call.locations.is_empty() {
        call.subject = subject_of(call.kind.as_deref(), &Value::Null, &call.locations);
    }
    if let Some(status) = facts.status.as_deref().and_then(status_of) {
        call.status = status;
    }
    if is_terminal(call.status) && call.ended_at.is_none() {
        call.ended_at = Some(created_at.to_string());
    }
}

pub(super) fn status_of(status: &str) -> Option<ToolCallStatus> {
    Some(match status {
        "pending" => ToolCallStatus::Pending,
        "in_progress" => ToolCallStatus::Running,
        "completed" => ToolCallStatus::Completed,
        "failed" => ToolCallStatus::Failed,
        "cancelled" => ToolCallStatus::Cancelled,
        "interrupted" => ToolCallStatus::Interrupted,
        _ => return None,
    })
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

/// The file change of a call as a unified diff: the agent's patch where it
/// sent one, else the hunks between its old and new text. A handoff reads
/// it whole where the step holds only its size.
pub(crate) fn diff_text_of(acp: &Value) -> Option<String> {
    let diff = acp
        .get("content")?
        .as_array()?
        .iter()
        .find(|entry| string_at(entry, "/type").as_deref() == Some("diff"))?;
    if let Some(patch) = string_at(diff, "/patch/text") {
        let headed = patch
            .lines()
            .take_while(|line| !line.starts_with("@@"))
            .any(|line| line.starts_with("--- ") || line.starts_with("+++ "));
        return Some(match string_at(diff, "/path") {
            Some(path) if !headed => format!("--- {path}\n+++ {path}\n{patch}"),
            _ => patch,
        });
    }
    let path = string_at(diff, "/path")?;
    let old = string_at(diff, "/oldText").unwrap_or_default();
    let new = string_at(diff, "/newText").unwrap_or_default();
    let text = similar::TextDiff::from_lines(&old, &new)
        .unified_diff()
        .context_radius(3)
        .missing_newline_hint(false)
        .header(&path, &path)
        .to_string();
    (!text.is_empty()).then_some(text)
}

/// The start of what a call gave back: the text of its content, else its
/// raw output.
pub(super) fn preview_of(acp: &Value) -> Option<String> {
    let whole = output_text(acp)?;
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

pub(super) fn permission_options(payload: &Value) -> Vec<PermissionOptionDto> {
    payload
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let id = string_at(option, "/optionId")?;
            Some(PermissionOptionDto {
                name: string_at(option, "/name").unwrap_or_else(|| id.clone()),
                kind: string_at(option, "/kind"),
                id,
            })
        })
        .collect()
}

pub(super) fn commands_of(payload: &Value) -> Vec<CommandDto> {
    commands_in(payload.get("available_commands"))
}

/// Each ACP command of a list.
fn commands_in(list: Option<&Value>) -> Vec<CommandDto> {
    list.and_then(Value::as_array)
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

/// Put each field of a display state (021, rule 24) that `display` holds
/// into the session's state: the whole state the row keeps, or the fields
/// a `display_state_update` says changed. Each mode, option and command is
/// the ACP JSON the agent sent. Answers whether `display` held a list of
/// commands, an empty one included.
pub(super) fn apply_display(session: &mut SessionStateDto, display: &Value) -> bool {
    if let Some(mode) = display.get("mode") {
        session.mode = mode.is_object().then(|| ModeStateDto {
            current_id: string_at(mode, "/current_id"),
            available: mode
                .get("available")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|mode| {
                    Some(ModeDto {
                        id: string_at(mode, "/id")?,
                        name: string_at(mode, "/name").unwrap_or_default(),
                        description: string_at(mode, "/description"),
                    })
                })
                .collect(),
        });
    }
    if let Some(options) = display.get("config_options") {
        session.config_options = options
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(config_option_of)
            .collect();
    }
    if let Some(title) = display.get("title") {
        session.title = title.as_str().map(str::to_string);
    }
    if let Some(cost) = display.get("cost") {
        session.cost = cost
            .get("amount")
            .and_then(Value::as_f64)
            .map(|amount| CostDto {
                amount,
                currency: string_at(cost, "/currency").unwrap_or_default(),
            });
    }
    let commands = display.get("commands");
    if commands.is_some() {
        session.commands = commands_in(commands);
    }
    commands.is_some_and(Value::is_array)
}

/// One ACP config option. A grouped select lists the values of every group,
/// and a boolean option has no values and its value as text.
fn config_option_of(option: &Value) -> Option<ConfigOptionDto> {
    let value = |value: &Value| {
        Some(ConfigValueDto {
            value: string_at(value, "/value")?,
            name: string_at(value, "/name").unwrap_or_default(),
            description: string_at(value, "/description"),
        })
    };
    let values = option
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(
            |entry| match entry.get("options").and_then(Value::as_array) {
                Some(group) => group.iter().filter_map(value).collect::<Vec<_>>(),
                None => value(entry).into_iter().collect(),
            },
        )
        .collect();
    Some(ConfigOptionDto {
        id: string_at(option, "/id")?,
        name: string_at(option, "/name").unwrap_or_default(),
        description: string_at(option, "/description"),
        category: string_at(option, "/category"),
        current_value: match option.get("currentValue") {
            Some(Value::String(text)) => Some(text.clone()),
            None | Some(Value::Null) => None,
            Some(other) => Some(other.to_string()),
        },
        values,
    })
}

pub(super) fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_string)
}
