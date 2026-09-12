//! A typed agent transcript.
//!
//! The model has no terminal concerns. The inline console in [`crate::tui`]
//! and the text renderer of `ariadne session logs` in the CLI are two
//! consumers of it, and another console can be a third.

use std::str::FromStr;
use std::time::Duration;

use ariadne_api::events::AgentEventDto;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Snapshot filters accepted by both transcript commands.
#[derive(Debug, Clone, Default)]
pub struct Filters {
    pub tail: Option<usize>,
    pub since: Option<Since>,
    pub kinds: Vec<String>,
}

impl Filters {
    pub fn allows_kind(&self, kind: &str) -> bool {
        self.kinds.is_empty() || self.kinds.iter().any(|wanted| wanted == kind)
    }

    pub fn apply(&self, items: Vec<TranscriptItem>) -> Vec<TranscriptItem> {
        let mut items: Vec<_> = items
            .into_iter()
            .filter(|item| {
                self.kinds.is_empty()
                    || item
                        .meta()
                        .kinds
                        .iter()
                        .any(|kind| self.kinds.iter().any(|wanted| wanted == kind))
            })
            .filter(|item| {
                self.since.as_ref().is_none_or(|since| {
                    DateTime::parse_from_rfc3339(&item.meta().created_at)
                        .map(|created| created.with_timezone(&Utc) >= since.0)
                        .unwrap_or(true)
                })
            })
            .collect();
        if let Some(tail) = self.tail {
            items.drain(..items.len().saturating_sub(tail));
        }
        items
    }

    /// Apply event filters without changing any retained DTO.
    pub fn apply_events<'a>(&self, events: &'a [AgentEventDto]) -> Vec<&'a AgentEventDto> {
        let mut events: Vec<_> = events
            .iter()
            .filter(|event| self.allows_kind(&event.kind))
            .filter(|event| {
                self.since.as_ref().is_none_or(|since| {
                    DateTime::parse_from_rfc3339(&event.created_at)
                        .map(|created| created.with_timezone(&Utc) >= since.0)
                        .unwrap_or(true)
                })
            })
            .collect();
        if let Some(tail) = self.tail {
            events.drain(..events.len().saturating_sub(tail));
        }
        events
    }
}

/// An RFC 3339 instant, or an instant relative to command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Since(DateTime<Utc>);

impl FromStr for Since {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Ok(at) = DateTime::parse_from_rfc3339(value) {
            return Ok(Self(at.with_timezone(&Utc)));
        }
        let Some((unit_at, unit)) = value.char_indices().next_back() else {
            return Err("use RFC 3339 or a duration such as 10m".to_string());
        };
        let number = &value[..unit_at];
        let count = number
            .parse::<u64>()
            .map_err(|_| "use RFC 3339 or a duration such as 10m".to_string())?;
        let seconds = match unit {
            's' => count.checked_mul(1),
            'm' => count.checked_mul(60),
            'h' => count.checked_mul(60 * 60),
            'd' => count.checked_mul(24 * 60 * 60),
            'w' => count.checked_mul(7 * 24 * 60 * 60),
            _ => None,
        }
        .ok_or_else(|| "use RFC 3339 or a duration such as 10m".to_string())?;
        let ago = chrono::Duration::from_std(Duration::from_secs(seconds))
            .map_err(|_| "duration is too large".to_string())?;
        Ok(Self(Utc::now() - ago))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemMeta {
    pub created_at: String,
    pub kinds: Vec<String>,
    /// The id of the whole the daemon stored at the end of the turn, once it
    /// has closed this block of agent text or thought (008). The daemon's
    /// ids are monotonic, so a chunk with a lower id is a chunk of that turn,
    /// however late it arrives.
    pub closed_by: Option<String>,
}

impl ItemMeta {
    pub fn from_event(event: &AgentEventDto) -> Self {
        Self {
            created_at: event.created_at.clone(),
            kinds: vec![event.kind.clone()],
            closed_by: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanEntry {
    pub content: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
}

/// A file a tool call touches, as the ACP call names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: String,
    pub line: Option<u64>,
}

/// What a tool call reads as when its event names no tool.
pub const UNNAMED_TOOL: &str = "ACP tool";

/// One tool call, as the ACP call the daemon records under `payload.acp`
/// (021): the same shape whether it opened, was updated, ended, or is the
/// call a permission question asks about.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tool {
    pub id: String,
    /// The agent's title for the call.
    pub name: String,
    /// The ACP kind: `read`, `edit`, `delete`, `move`, `search`, `execute`,
    /// `think`, `fetch`, `switch_mode` or `other`.
    pub kind: Option<String>,
    pub input: Value,
    pub locations: Vec<Location>,
    pub output: Option<String>,
    /// The file change as a unified diff: the agent's patch where it sent
    /// one, and the hunks between the old text and the new otherwise.
    pub diff: Option<String>,
    pub status: Option<String>,
    /// When the call ended: the time of the terminal event that closed the
    /// call an earlier event opened. A terminal event with no opener has no
    /// duration to speak of.
    pub ended_at: Option<String>,
}

impl Tool {
    /// The call an event carries: `tool_name` and `tool_input` over the ACP
    /// call under `acp`, and `fallback` where nothing names the tool.
    fn from_payload(payload: &Value, fallback: &str) -> Self {
        let acp = payload.get("acp").unwrap_or(payload);
        Self {
            id: string_at(acp, "/toolCallId")
                .or_else(|| string_at(payload, "/tool_call_id"))
                .unwrap_or_default(),
            name: string_at(payload, "/tool_name")
                .or_else(|| string_at(acp, "/title"))
                .unwrap_or_else(|| fallback.to_string()),
            kind: string_at(acp, "/kind"),
            input: payload
                .get("tool_input")
                .or_else(|| acp.get("rawInput"))
                .cloned()
                .unwrap_or(Value::Null),
            locations: acp
                .get("locations")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|location| {
                    Some(Location {
                        path: string_at(location, "/path")?,
                        line: location.get("line").and_then(Value::as_u64),
                    })
                })
                .collect(),
            output: tool_output(acp),
            diff: tool_diff(acp),
            status: string_at(acp, "/status"),
            ended_at: None,
        }
    }

    /// Whether `update` is a later event of this call.
    fn is_same(&self, update: &Self) -> bool {
        (!update.id.is_empty() && self.id == update.id)
            || (update.id.is_empty() && self.name == update.name)
    }

    /// Take what a later event of the call says, and keep what it does not
    /// say again.
    fn merge(&mut self, update: Self) {
        if update.name != UNNAMED_TOOL {
            self.name = update.name;
        }
        if update.kind.is_some() {
            self.kind = update.kind;
        }
        if !update.input.is_null() {
            self.input = update.input;
        }
        if !update.locations.is_empty() {
            self.locations = update.locations;
        }
        if update.output.is_some() {
            self.output = update.output;
        }
        if update.diff.is_some() {
            self.diff = update.diff;
        }
        if update.status.is_some() {
            self.status = update.status;
        }
    }
}

/// One semantic block in a session transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptItem {
    UserPrompt {
        meta: ItemMeta,
        text: String,
        source: Option<String>,
    },
    AgentText {
        meta: ItemMeta,
        text: String,
    },
    Thought {
        meta: ItemMeta,
        text: String,
    },
    Plan {
        meta: ItemMeta,
        entries: Vec<PlanEntry>,
    },
    ToolCall {
        meta: ItemMeta,
        tool: Tool,
    },
    PermissionQuestion {
        meta: ItemMeta,
        question: String,
        /// The call the question asks about.
        tool: Tool,
        options: Vec<PermissionOption>,
        answer: Option<String>,
    },
    SystemNote {
        meta: ItemMeta,
        text: String,
    },
    Error {
        meta: ItemMeta,
        text: String,
    },
    Raw {
        meta: ItemMeta,
        kind: String,
        payload: Value,
    },
}

impl TranscriptItem {
    pub fn meta(&self) -> &ItemMeta {
        match self {
            Self::UserPrompt { meta, .. }
            | Self::AgentText { meta, .. }
            | Self::Thought { meta, .. }
            | Self::Plan { meta, .. }
            | Self::ToolCall { meta, .. }
            | Self::PermissionQuestion { meta, .. }
            | Self::SystemNote { meta, .. }
            | Self::Error { meta, .. }
            | Self::Raw { meta, .. } => meta,
        }
    }
}

/// What a prompt reads as when its event carries no `text`.
pub const UNRECORDED_PROMPT: &str = "(prompt text not recorded)";

impl From<&AgentEventDto> for TranscriptItem {
    fn from(event: &AgentEventDto) -> Self {
        let meta = ItemMeta::from_event(event);
        match event.kind.as_str() {
            // `text` is the prompt alone. `prompt` is the seat's system
            // prompt over it (021), and the summary is that whole cut short,
            // so neither stands in for it: an event with no `text` is from a
            // daemon that predates the field, and its text is unknown.
            "user_prompt_submit" => Self::UserPrompt {
                meta,
                text: string_at(&event.payload, "/text")
                    .unwrap_or_else(|| UNRECORDED_PROMPT.into()),
                source: string_at(&event.payload, "/source"),
            },
            "agent_message" | "agent_message_chunk" => Self::AgentText {
                meta,
                text: text(event),
            },
            "agent_thought" | "agent_thought_chunk" => Self::Thought {
                meta,
                text: text(event),
            },
            "plan" => Self::Plan {
                meta,
                entries: event
                    .payload
                    .get("entries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|entry| PlanEntry {
                        content: string_at(entry, "/content").unwrap_or_default(),
                        status: string_at(entry, "/status").unwrap_or_else(|| "pending".into()),
                    })
                    .collect(),
            },
            "pre_tool_use" | "post_tool_use" | "tool_call_update" => Self::ToolCall {
                meta,
                tool: Tool::from_payload(&event.payload, UNNAMED_TOOL),
            },
            "permission_request" => {
                let tool = Tool::from_payload(&event.payload, &event.summary);
                Self::PermissionQuestion {
                    meta,
                    question: tool.name.clone(),
                    tool,
                    options: permission_options(&event.payload),
                    answer: None,
                }
            }
            "session.error" => Self::Error {
                meta,
                text: error_text(event),
            },
            "session_start" => Self::SystemNote {
                meta,
                text: "session started".into(),
            },
            "stop" => Self::SystemNote {
                meta,
                text: string_at(&event.payload, "/stop_reason")
                    .map(|reason| format!("turn stopped: {reason}"))
                    .unwrap_or_else(|| "turn stopped".into()),
            },
            "compaction_update" => Self::SystemNote {
                meta,
                text: "context compacted".into(),
            },
            "session_end" => Self::SystemNote {
                meta,
                text: "session ended".into(),
            },
            _ => Self::Raw {
                meta,
                kind: event.kind.clone(),
                payload: event.payload.clone(),
            },
        }
    }
}

/// Fold paired events into the blocks a person reads.
pub fn fold(events: &[AgentEventDto]) -> Vec<TranscriptItem> {
    let mut items = Vec::new();
    for event in events {
        fold_into(&mut items, event);
    }
    items
}

/// Fold one event into the blocks built so far.
///
/// [`fold`] is this over a whole snapshot; the inline console is this over a
/// live stream, which is why the one event is a seam of its own.
pub fn fold_into(items: &mut Vec<TranscriptItem>, event: &AgentEventDto) {
    if event.kind == "permission.replied"
        && let Some(TranscriptItem::PermissionQuestion {
            meta,
            options,
            answer,
            ..
        }) = items.iter_mut().rev().find(|item| {
            matches!(
                item,
                TranscriptItem::PermissionQuestion { answer: None, .. }
            )
        })
    {
        *answer = Some(permission_answer(&event.payload, options));
        meta.kinds.push(event.kind.clone());
        return;
    }

    // A later event of an open call — a live `tool_call_update`, or the
    // `post_tool_use` that ends it — is that call, not a call of its own:
    // one block per `toolCallId`, however many updates stream into it.
    let mut item = TranscriptItem::from(event);
    if matches!(event.kind.as_str(), "post_tool_use" | "tool_call_update")
        && let TranscriptItem::ToolCall { tool: update, .. } = &mut item
        && let Some(TranscriptItem::ToolCall { meta, tool }) =
            items.iter_mut().rev().find(|previous| {
                matches!(previous,
                    TranscriptItem::ToolCall { tool, .. }
                        if !tool_is_terminal(tool.status.as_deref()) && tool.is_same(update))
            })
    {
        tool.merge(std::mem::take(update));
        if tool_is_terminal(tool.status.as_deref()) {
            tool.ended_at = Some(event.created_at.clone());
        }
        meta.kinds.push(event.kind.clone());
        return;
    }
    if event.kind == "permission.replied" {
        item = TranscriptItem::SystemNote {
            meta: ItemMeta::from_event(event),
            text: format!(
                "permission answered: {}",
                string_at(&event.payload, "/option_id").unwrap_or_else(|| "cancelled".into())
            ),
        };
    }
    items.push(item);
}

pub fn tool_is_terminal(status: Option<&str>) -> bool {
    matches!(status, Some("completed" | "failed"))
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_string)
}

/// The text an event carries, and its summary where it carries none.
pub fn text(event: &AgentEventDto) -> String {
    string_at(&event.payload, "/text").unwrap_or_else(|| event.summary.clone())
}

fn error_text(event: &AgentEventDto) -> String {
    string_at(&event.payload, "/error/data/message")
        .or_else(|| string_at(&event.payload, "/error/message"))
        .or_else(|| string_at(&event.payload, "/message"))
        .unwrap_or_else(|| event.summary.clone())
}

/// What a call gave back, in words: its `rawOutput` where that is text or a
/// stdout and stderr pair, the text of its `content` entries otherwise, and
/// the structure as JSON only where there is neither.
fn tool_output(acp: &Value) -> Option<String> {
    let raw = acp.get("rawOutput").filter(|value| !value.is_null());
    if let Some(text) = raw.and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(object) = raw.and_then(Value::as_object) {
        let streams: Vec<_> = ["stdout", "stderr"]
            .into_iter()
            .filter_map(|key| object.get(key)?.as_str())
            .filter(|text| !text.is_empty())
            .collect();
        if !streams.is_empty() {
            return Some(streams.join("\n"));
        }
    }
    let text: Vec<_> = acp
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("content"))
        .filter_map(|content| string_at(content, "/content/text"))
        .collect();
    if !text.is_empty() {
        return Some(text.join("\n"));
    }
    raw.and_then(|raw| serde_json::to_string(raw).ok())
}

fn tool_diff(acp: &Value) -> Option<String> {
    let diff = acp
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .find(|content| content.get("type").and_then(Value::as_str) == Some("diff"))?;
    if let Some(patch) = string_at(diff, "/patch/text") {
        // A patch is not bound to carry file headers: one that starts at its
        // first hunk takes them from the entry's `path`, which names the file
        // the patch does not.
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
    Some(unified(&path, &old, &new)).filter(|diff| !diff.is_empty())
}

/// The hunks between two texts of one file, with three lines of context
/// around each, under the file header a patch would carry. Empty where the
/// two texts are the same.
fn unified(path: &str, old: &str, new: &str) -> String {
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .missing_newline_hint(false)
        .header(path, path)
        .to_string()
}

fn permission_options(payload: &Value) -> Vec<PermissionOption> {
    payload
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let id = string_at(option, "/optionId")?;
            let name = string_at(option, "/name").unwrap_or_else(|| id.clone());
            Some(PermissionOption { id, name })
        })
        .collect()
}

/// The name of the option a reply picked, its id where no option carries
/// that id, and `cancelled` where the reply picked none.
pub fn permission_answer(payload: &Value, options: &[PermissionOption]) -> String {
    let Some(id) = string_at(payload, "/option_id") else {
        return "cancelled".into();
    };
    options
        .iter()
        .find(|option| option.id == id)
        .map(|option| option.name.clone())
        .unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use ariadne_api::events::AgentEventDto;
    use serde_json::json;

    use super::{Since, TranscriptItem, fold};

    fn event(id: &str, kind: &str, payload: serde_json::Value) -> AgentEventDto {
        AgentEventDto {
            id: id.into(),
            session_id: Some("session".into()),
            task_id: None,
            kind: kind.into(),
            payload,
            summary: "summary".into(),
            created_at: "2026-09-11T12:34:56Z".into(),
        }
    }

    #[test]
    fn a_patch_without_file_headers_takes_them_from_the_entry_path() {
        let patch = "@@ -1 +1 @@\n-old\n+new\n";
        let events = [event(
            "tool",
            "post_tool_use",
            json!({
                "acp": {"toolCallId": "call-1", "title": "Edit", "kind": "edit",
                        "status": "completed",
                        "content": [{"type": "diff", "path": "a.txt",
                                     "patch": {"format": "git_patch", "text": patch}}]}
            }),
        )];

        let items = fold(&events);
        let TranscriptItem::ToolCall { tool, .. } = &items[0] else {
            panic!("{items:?}");
        };

        assert_eq!(
            tool.diff.as_deref(),
            Some("--- a.txt\n+++ a.txt\n@@ -1 +1 @@\n-old\n+new\n")
        );
    }

    #[test]
    fn an_old_and_a_new_text_fold_to_hunks_with_context() {
        let old = "one\ntwo\nthree\nfour\nfive\nsix\nseven\n";
        let new = "one\ntwo\nthree\n4\nfive\nsix\nseven\n";
        let events = [event(
            "tool",
            "post_tool_use",
            json!({
                "acp": {"toolCallId": "call-1", "title": "Edit", "kind": "edit",
                        "status": "completed",
                        "content": [{"type": "diff", "path": "count.txt",
                                     "oldText": old, "newText": new}]}
            }),
        )];

        let items = fold(&events);
        let TranscriptItem::ToolCall { tool, .. } = &items[0] else {
            panic!("{items:?}");
        };

        assert_eq!(
            tool.diff.as_deref(),
            Some(
                "--- count.txt\n+++ count.txt\n@@ -1,7 +1,7 @@\n one\n two\n three\n-four\n+4\n five\n six\n seven\n"
            )
        );
    }

    #[test]
    fn updates_of_one_call_fold_into_it_and_the_last_dates_its_end() {
        let update = |id: &str, status: &str, output: &str| {
            let mut event = event(
                &format!("{id}-{output}"),
                "tool_call_update",
                json!({"tool_call_id": id,
                       "acp": {"toolCallId": id, "title": "Bash", "kind": "execute",
                               "status": status, "rawInput": {"command": "make"},
                               "rawOutput": output}}),
            );
            event.created_at = "2026-09-11T12:35:00Z".into();
            event
        };
        let events = [
            event(
                "pre",
                "pre_tool_use",
                json!({"tool_name": "Bash",
                       "acp": {"toolCallId": "one", "kind": "execute", "status": "pending",
                               "rawInput": {"command": "make"}}}),
            ),
            update("one", "in_progress", "first"),
            update("one", "in_progress", "second"),
            update("one", "in_progress", "third"),
            event(
                "pre-2",
                "pre_tool_use",
                json!({"acp": {"toolCallId": "two", "kind": "read", "status": "pending"}}),
            ),
        ];

        let items = fold(&events);

        assert_eq!(items.len(), 2, "{items:?}");
        let TranscriptItem::ToolCall { meta, tool } = &items[0] else {
            panic!("{items:?}");
        };
        assert_eq!(tool.output.as_deref(), Some("third"));
        assert_eq!(tool.kind.as_deref(), Some("execute"));
        assert_eq!(meta.created_at, "2026-09-11T12:34:56Z", "the opener's time");
        assert_eq!(tool.ended_at, None, "still open");

        let mut items = items;
        let mut end = update("one", "completed", "done");
        end.kind = "post_tool_use".into();
        super::fold_into(&mut items, &end);

        assert_eq!(items.len(), 2, "{items:?}");
        let TranscriptItem::ToolCall { tool, .. } = &items[0] else {
            panic!("{items:?}");
        };
        assert_eq!(tool.status.as_deref(), Some("completed"));
        assert_eq!(tool.ended_at.as_deref(), Some("2026-09-11T12:35:00Z"));
    }

    /// A call whose `rawOutput` is a structure rather than text — a tool
    /// search's references, for one — says what it found in its `content`
    /// entries, and that is what is drawn, not the structure as JSON.
    #[test]
    fn a_structured_raw_output_gives_way_to_the_content_text() {
        let events = [event(
            "tool",
            "post_tool_use",
            json!({
                "acp": {"toolCallId": "search", "title": "ToolSearch", "kind": "other",
                        "status": "completed",
                        "content": [
                            {"type": "content", "content": {"type": "text", "text": "Tool: list_tasks"}},
                            {"type": "content", "content": {"type": "text", "text": "Tool: finalize_plan"}}],
                        "rawOutput": [{"tool_name": "list_tasks", "type": "tool_reference"}]}
            }),
        )];

        let items = fold(&events);
        let TranscriptItem::ToolCall { tool, .. } = &items[0] else {
            panic!("{items:?}");
        };

        assert_eq!(
            tool.output.as_deref(),
            Some("Tool: list_tasks\nTool: finalize_plan")
        );
    }

    #[test]
    fn a_multibyte_since_suffix_is_a_parse_error() {
        assert!("1é".parse::<Since>().is_err());
    }

    #[test]
    fn completed_same_named_tools_are_not_folded_again_without_an_open_call() {
        let events = [
            event(
                "pre",
                "pre_tool_use",
                json!({"tool_name": "Bash", "acp": {"title": "Bash", "status": "pending"}}),
            ),
            event(
                "post-1",
                "post_tool_use",
                json!({"tool_name": "Bash", "acp": {"title": "Bash", "status": "completed",
                    "rawOutput": "first"}}),
            ),
            event(
                "post-2",
                "post_tool_use",
                json!({"tool_name": "Bash", "acp": {"title": "Bash", "status": "completed",
                    "rawOutput": "second"}}),
            ),
        ];

        let items = fold(&events);

        assert_eq!(items.len(), 2, "an ended call is not open for another post");
    }
}
