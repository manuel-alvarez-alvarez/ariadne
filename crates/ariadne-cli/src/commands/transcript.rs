//! A typed agent transcript and its terminal rendering.
//!
//! The model in the first half has no terminal concerns. The renderer in the
//! second half is a separate consumer, so another console can reuse the model.

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
}

impl ItemMeta {
    fn from_event(event: &AgentEventDto) -> Self {
        Self {
            created_at: event.created_at.clone(),
            kinds: vec![event.kind.clone()],
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
        id: String,
        name: String,
        input: Value,
        output: Option<String>,
        diff: Option<String>,
        status: Option<String>,
    },
    PermissionQuestion {
        meta: ItemMeta,
        question: String,
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

    fn meta_mut(&mut self) -> &mut ItemMeta {
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

impl From<&AgentEventDto> for TranscriptItem {
    fn from(event: &AgentEventDto) -> Self {
        let meta = ItemMeta::from_event(event);
        match event.kind.as_str() {
            "user_prompt_submit" => Self::UserPrompt {
                meta,
                text: string_at(&event.payload, "/text")
                    .or_else(|| string_at(&event.payload, "/prompt"))
                    .unwrap_or_else(|| event.summary.clone()),
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
            "pre_tool_use" | "post_tool_use" | "tool_call_update" => tool_item(event, meta),
            "permission_request" => Self::PermissionQuestion {
                meta,
                question: string_at(&event.payload, "/tool_name")
                    .or_else(|| string_at(&event.payload, "/acp/title"))
                    .unwrap_or_else(|| event.summary.clone()),
                options: permission_options(&event.payload),
                answer: None,
            },
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
            continue;
        }

        let mut item = TranscriptItem::from(event);
        if event.kind == "post_tool_use"
            && let TranscriptItem::ToolCall { id, name, .. } = &item
            && let Some(previous) = items.iter_mut().rev().find(|previous| {
                matches!(previous,
                    TranscriptItem::ToolCall {
                        id: old_id,
                        name: old_name,
                        status,
                        ..
                    } if !tool_is_terminal(status.as_deref())
                        && ((!id.is_empty() && old_id == id)
                            || (id.is_empty() && old_name == name)))
            })
        {
            let created_at = previous.meta().created_at.clone();
            let mut kinds = previous.meta().kinds.clone();
            kinds.push(event.kind.clone());
            *previous = item;
            previous.meta_mut().created_at = created_at;
            previous.meta_mut().kinds = kinds;
            continue;
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
    items
}

fn tool_is_terminal(status: Option<&str>) -> bool {
    matches!(status, Some("completed" | "failed"))
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_string)
}

fn text(event: &AgentEventDto) -> String {
    string_at(&event.payload, "/text").unwrap_or_else(|| event.summary.clone())
}

fn error_text(event: &AgentEventDto) -> String {
    string_at(&event.payload, "/error/data/message")
        .or_else(|| string_at(&event.payload, "/error/message"))
        .or_else(|| string_at(&event.payload, "/message"))
        .unwrap_or_else(|| event.summary.clone())
}

fn tool_item(event: &AgentEventDto, meta: ItemMeta) -> TranscriptItem {
    let acp = event.payload.get("acp").unwrap_or(&event.payload);
    TranscriptItem::ToolCall {
        meta,
        id: string_at(acp, "/toolCallId")
            .or_else(|| string_at(&event.payload, "/tool_call_id"))
            .unwrap_or_default(),
        name: string_at(&event.payload, "/tool_name")
            .or_else(|| string_at(acp, "/title"))
            .unwrap_or_else(|| "ACP tool".into()),
        input: event
            .payload
            .get("tool_input")
            .or_else(|| acp.get("rawInput"))
            .cloned()
            .unwrap_or(Value::Null),
        output: tool_output(acp),
        diff: tool_diff(acp),
        status: string_at(acp, "/status"),
    }
}

fn tool_output(acp: &Value) -> Option<String> {
    if let Some(raw) = acp.get("rawOutput").filter(|value| !value.is_null()) {
        if let Some(text) = raw.as_str() {
            return Some(text.to_string());
        }
        if let Some(object) = raw.as_object() {
            let streams: Vec<_> = ["stdout", "stderr"]
                .into_iter()
                .filter_map(|key| object.get(key)?.as_str())
                .filter(|text| !text.is_empty())
                .collect();
            if !streams.is_empty() {
                return Some(streams.join("\n"));
            }
        }
        return serde_json::to_string(raw).ok();
    }
    let text: Vec<_> = acp
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("content"))
        .filter_map(|content| string_at(content, "/content/text"))
        .collect();
    (!text.is_empty()).then(|| text.join("\n"))
}

fn tool_diff(acp: &Value) -> Option<String> {
    let diff = acp
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .find(|content| content.get("type").and_then(Value::as_str) == Some("diff"))?;
    if let Some(patch) = string_at(diff, "/patch/text") {
        return Some(patch);
    }
    let path = string_at(diff, "/path")?;
    let old = string_at(diff, "/oldText").unwrap_or_default();
    let new = string_at(diff, "/newText").unwrap_or_default();
    let mut rendered = format!("--- {path}\n+++ {path}\n");
    rendered.extend(old.lines().map(|line| format!("-{line}\n")));
    rendered.extend(new.lines().map(|line| format!("+{line}\n")));
    Some(rendered)
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

fn permission_answer(payload: &Value, options: &[PermissionOption]) -> String {
    let Some(id) = string_at(payload, "/option_id") else {
        return "cancelled".into();
    };
    options
        .iter()
        .find(|option| option.id == id)
        .map(|option| option.name.clone())
        .unwrap_or(id)
}

/// Terminal rendering for [`TranscriptItem`].
pub mod render {
    use std::collections::HashSet;

    use anstyle::{AnsiColor, Color, Style};
    use ariadne_api::events::AgentEventDto;

    use super::{Filters, ItemMeta, PermissionOption, TranscriptItem, fold};
    use crate::output::{pager, style};

    const USER: Style = foreground(AnsiColor::Cyan);
    const AGENT: Style = foreground(AnsiColor::Green);
    const PLAN: Style = foreground(AnsiColor::Blue);
    const TOOL: Style = foreground(AnsiColor::Yellow);
    const PERMISSION: Style = foreground(AnsiColor::Magenta);
    const ERROR: Style = foreground(AnsiColor::Red).bold();
    const DIM: Style = Style::new().dimmed();

    const fn foreground(color: AnsiColor) -> Style {
        Style::new().fg_color(Some(Color::Ansi(color)))
    }

    pub fn transcript(items: &[TranscriptItem], width: Option<usize>, color: bool) -> String {
        items.iter().map(|item| block(item, width, color)).collect()
    }

    fn block(item: &TranscriptItem, width: Option<usize>, color: bool) -> String {
        match item {
            TranscriptItem::UserPrompt { meta, text, source } => {
                let label = source
                    .as_deref()
                    .filter(|source| *source != "console")
                    .map_or_else(|| "USER".to_string(), |source| format!("USER/{source}"));
                text_block(meta, &label, USER, text, Style::new(), width, color)
            }
            TranscriptItem::AgentText { meta, text } => {
                text_block(meta, "AGENT", AGENT, text, Style::new(), width, color)
            }
            TranscriptItem::Thought { meta, text } => {
                text_block(meta, "THOUGHT", DIM, text, DIM, width, color)
            }
            TranscriptItem::Plan { meta, entries } => {
                let mut out = header(meta, "PLAN", PLAN, color);
                out.push('\n');
                for entry in entries {
                    let (entry_style, glyph) = style::status(&entry.status);
                    out.push_str("  ");
                    out.push_str(&style::paint(
                        color,
                        entry_style,
                        &glyph.unwrap_or('•').to_string(),
                    ));
                    out.push(' ');
                    out.push_str(&entry.content);
                    out.push('\n');
                }
                out.push('\n');
                out
            }
            TranscriptItem::ToolCall {
                meta,
                name,
                input,
                output,
                diff,
                status,
                ..
            } => tool_block(
                meta,
                name,
                input,
                output.as_deref(),
                diff.as_deref(),
                status.as_deref(),
                color,
            ),
            TranscriptItem::PermissionQuestion {
                meta,
                question,
                options,
                answer,
            } => permission_block(meta, question, options, answer.as_deref(), color),
            TranscriptItem::SystemNote { meta, text } => {
                text_block(meta, "SYSTEM", DIM, text, DIM, width, color)
            }
            TranscriptItem::Error { meta, text } => {
                text_block(meta, "ERROR", ERROR, text, ERROR, width, color)
            }
            TranscriptItem::Raw {
                meta,
                kind,
                payload,
            } => text_block(
                meta,
                &kind.to_ascii_uppercase(),
                DIM,
                &serde_json::to_string_pretty(payload).unwrap_or_else(|_| payload.to_string()),
                Style::new(),
                width,
                color,
            ),
        }
    }

    fn text_block(
        meta: &ItemMeta,
        label: &str,
        label_style: Style,
        text: &str,
        content_style: Style,
        width: Option<usize>,
        color: bool,
    ) -> String {
        let prefix = format!("{}  ", header(meta, label, label_style, color));
        let indent = " ".repeat(8 + 2 + label.chars().count() + 2);
        let available = width.map(|width| width.saturating_sub(indent.len()).max(1));
        let lines = wrap(text, available);
        let mut out = String::new();
        for (index, line) in lines.iter().enumerate() {
            out.push_str(if index == 0 { &prefix } else { &indent });
            out.push_str(&style::paint(color, content_style, line));
            out.push('\n');
        }
        out.push('\n');
        out
    }

    fn header(meta: &ItemMeta, label: &str, label_style: Style, color: bool) -> String {
        format!(
            "{}  {}",
            style::paint(color, style::META, &clock(&meta.created_at)),
            style::paint(color, label_style, label)
        )
    }

    fn clock(timestamp: &str) -> String {
        chrono::DateTime::parse_from_rfc3339(timestamp)
            .map(|time| {
                time.with_timezone(&chrono::Local)
                    .format("%H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|_| timestamp.to_string())
    }

    fn wrap(text: &str, width: Option<usize>) -> Vec<String> {
        let Some(width) = width else {
            return text.split('\n').map(str::to_string).collect();
        };
        let mut out = Vec::new();
        for source in text.split('\n') {
            if source.is_empty() {
                out.push(String::new());
                continue;
            }
            let mut line = String::new();
            for word in source.split_whitespace() {
                let extra = usize::from(!line.is_empty()) + word.chars().count();
                if !line.is_empty() && line.chars().count() + extra > width {
                    out.push(std::mem::take(&mut line));
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(word);
            }
            out.push(line);
        }
        if out.is_empty() {
            out.push(String::new());
        }
        out
    }

    fn tool_line(name: &str, input: &serde_json::Value, status: Option<&str>) -> String {
        let input = match input {
            serde_json::Value::Null => String::new(),
            serde_json::Value::String(text) => format!(" {text}"),
            input => format!(" {}", serde_json::to_string(input).unwrap_or_default()),
        };
        let status = status.map_or_else(String::new, |status| format!(" [{status}]"));
        format!("{name}{input}{status}")
    }

    fn tool_block(
        meta: &ItemMeta,
        name: &str,
        input: &serde_json::Value,
        output: Option<&str>,
        diff: Option<&str>,
        status: Option<&str>,
        color: bool,
    ) -> String {
        let mut out = format!(
            "{}  {}\n",
            header(meta, "TOOL", TOOL, color),
            tool_line(name, input, status)
        );
        out.push_str(&tool_result(output, diff, color));
        out.push('\n');
        out
    }

    fn tool_result(output: Option<&str>, diff: Option<&str>, color: bool) -> String {
        let mut out = String::new();
        if let Some(output) = output {
            for line in output.trim_end_matches('\n').split('\n') {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
        if let Some(diff) = diff {
            let diff = pager::diff(diff, color);
            for line in diff.trim_end_matches('\n').split('\n') {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    fn permission_block(
        meta: &ItemMeta,
        question: &str,
        options: &[PermissionOption],
        answer: Option<&str>,
        color: bool,
    ) -> String {
        let mut out = format!(
            "{}  {}\n",
            header(meta, "PERMISSION", PERMISSION, color),
            question
        );
        for (index, option) in options.iter().enumerate() {
            out.push_str(&format!("  {}. {}\n", index + 1, option.name));
        }
        if let Some(answer) = answer {
            out.push_str(&format!("  answer: {answer}\n"));
        }
        out.push('\n');
        out
    }

    /// Stateful rendering for the console stream.
    pub struct StreamRenderer {
        width: Option<usize>,
        color: bool,
        active_agent: Option<&'static str>,
        open_tools: HashSet<String>,
        pending_permission: Option<Vec<PermissionOption>>,
    }

    impl StreamRenderer {
        pub fn new(width: Option<usize>, color: bool) -> Self {
            Self {
                width,
                color,
                active_agent: None,
                open_tools: HashSet::new(),
                pending_permission: None,
            }
        }

        pub fn snapshot(&mut self, events: &[AgentEventDto], filters: &Filters) -> String {
            let mut items = filters.apply(fold(events));
            self.active_agent = None;
            self.open_tools.clear();
            self.pending_permission = None;
            for item in &items {
                match item {
                    TranscriptItem::ToolCall {
                        id, name, status, ..
                    } if !super::tool_is_terminal(status.as_deref()) => {
                        self.open_tools.insert(Self::tool_key(id, name));
                    }
                    TranscriptItem::PermissionQuestion {
                        options,
                        answer: None,
                        ..
                    } => self.pending_permission = Some(options.clone()),
                    _ => {}
                }
            }
            let Some(last) = items.last() else {
                return String::new();
            };
            if !Self::stays_open(last) {
                return transcript(&items, self.width, self.color);
            }
            let last = items.pop().expect("the last transcript item exists");
            let mut out = transcript(&items, self.width, self.color);
            let open = block(&last, self.width, self.color);
            out.push_str(open.trim_end_matches('\n'));
            if matches!(
                &last,
                TranscriptItem::ToolCall { .. } | TranscriptItem::PermissionQuestion { .. }
            ) {
                out.push('\n');
            }
            self.active_agent = match &last {
                TranscriptItem::AgentText { .. } => Some("agent"),
                TranscriptItem::Thought { .. } => Some("thought"),
                _ => None,
            };
            out
        }

        pub fn event(&mut self, event: &AgentEventDto, filters: &Filters) -> String {
            match event.kind.as_str() {
                "agent_message_chunk" => {
                    self.chunk(event, "agent", "AGENT", AGENT, Style::new(), filters)
                }
                "agent_thought_chunk" => self.chunk(event, "thought", "THOUGHT", DIM, DIM, filters),
                "agent_message" if self.active_agent == Some("agent") => {
                    self.active_agent = None;
                    "\n\n".into()
                }
                "agent_thought" if self.active_agent == Some("thought") => {
                    self.active_agent = None;
                    "\n\n".into()
                }
                "pre_tool_use" if filters.allows_kind(&event.kind) => self.tool_start(event),
                "tool_call_update" if filters.allows_kind(&event.kind) => self.tool_update(event),
                "post_tool_use" if filters.allows_kind(&event.kind) => self.tool_end(event),
                "permission_request" if filters.allows_kind(&event.kind) => {
                    self.permission_start(event)
                }
                "permission.replied" if filters.allows_kind(&event.kind) => {
                    if let Some(options) = self.pending_permission.take() {
                        let answer = super::permission_answer(&event.payload, &options);
                        format!("  answer: {answer}\n\n")
                    } else {
                        self.render_if_allowed(event, filters)
                    }
                }
                _ => self.render_if_allowed(event, filters),
            }
        }

        pub fn finish(&mut self) -> String {
            let had_open_block = self.active_agent.take().is_some()
                || !self.open_tools.is_empty()
                || self.pending_permission.is_some();
            self.open_tools.clear();
            self.pending_permission = None;
            if had_open_block {
                "\n\n".into()
            } else {
                String::new()
            }
        }

        fn render_if_allowed(&mut self, event: &AgentEventDto, filters: &Filters) -> String {
            if !filters.allows_kind(&event.kind) {
                return String::new();
            }
            let mut out = self.close_agent();
            out.push_str(&transcript(
                &fold(std::slice::from_ref(event)),
                self.width,
                self.color,
            ));
            out
        }

        fn stays_open(item: &TranscriptItem) -> bool {
            match item {
                TranscriptItem::AgentText { meta, .. } => meta
                    .kinds
                    .last()
                    .is_some_and(|kind| kind == "agent_message_chunk"),
                TranscriptItem::Thought { meta, .. } => meta
                    .kinds
                    .last()
                    .is_some_and(|kind| kind == "agent_thought_chunk"),
                TranscriptItem::ToolCall { meta, status, .. } => {
                    meta.kinds
                        .last()
                        .is_some_and(|kind| kind == "pre_tool_use" || kind == "tool_call_update")
                        && !matches!(status.as_deref(), Some("completed" | "failed"))
                }
                TranscriptItem::PermissionQuestion { answer, .. } => answer.is_none(),
                _ => false,
            }
        }

        fn chunk(
            &mut self,
            event: &AgentEventDto,
            active: &'static str,
            label: &str,
            label_style: Style,
            content_style: Style,
            filters: &Filters,
        ) -> String {
            if !filters.allows_kind(&event.kind) {
                return String::new();
            }
            let text = super::text(event);
            if self.active_agent == Some(active) {
                return style::paint(self.color, content_style, &text);
            }
            let mut out = self.close_agent();
            out.push_str(&header(
                &ItemMeta::from_event(event),
                label,
                label_style,
                self.color,
            ));
            out.push_str("  ");
            out.push_str(&style::paint(self.color, content_style, &text));
            self.active_agent = Some(active);
            out
        }

        fn tool_start(&mut self, event: &AgentEventDto) -> String {
            let TranscriptItem::ToolCall {
                meta,
                id,
                name,
                input,
                status,
                ..
            } = TranscriptItem::from(event)
            else {
                return String::new();
            };
            let mut out = self.close_agent();
            if !self.open_tools.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!(
                "{}  {}\n",
                header(&meta, "TOOL", TOOL, self.color),
                tool_line(&name, &input, status.as_deref())
            ));
            self.open_tools.insert(Self::tool_key(&id, &name));
            out
        }

        fn tool_end(&mut self, event: &AgentEventDto) -> String {
            let item = TranscriptItem::from(event);
            let TranscriptItem::ToolCall {
                id,
                name,
                output,
                diff,
                ..
            } = &item
            else {
                return String::new();
            };
            if self.open_tools.remove(&Self::tool_key(id, name)) {
                return format!(
                    "{}\n",
                    tool_result(output.as_deref(), diff.as_deref(), self.color)
                );
            }
            let mut out = self.close_agent();
            out.push_str(&block(&item, self.width, self.color));
            out
        }

        fn tool_update(&mut self, event: &AgentEventDto) -> String {
            let item = TranscriptItem::from(event);
            let TranscriptItem::ToolCall {
                id,
                name,
                output,
                diff,
                status,
                ..
            } = &item
            else {
                return String::new();
            };
            if self.open_tools.contains(&Self::tool_key(id, name)) {
                let mut out = String::new();
                if let Some(status) = status {
                    out.push_str(&format!("  status: {status}\n"));
                }
                out.push_str(&tool_result(output.as_deref(), diff.as_deref(), self.color));
                return out;
            }
            self.tool_start(event)
        }

        fn permission_start(&mut self, event: &AgentEventDto) -> String {
            let TranscriptItem::PermissionQuestion {
                meta,
                question,
                options,
                ..
            } = TranscriptItem::from(event)
            else {
                return String::new();
            };
            let mut out = self.close_agent();
            out.push_str(&format!(
                "{}  {}\n",
                header(&meta, "PERMISSION", PERMISSION, self.color),
                question
            ));
            for (index, option) in options.iter().enumerate() {
                out.push_str(&format!("  {}. {}\n", index + 1, option.name));
            }
            self.pending_permission = Some(options);
            out
        }

        fn close_agent(&mut self) -> String {
            self.active_agent
                .take()
                .map_or_else(String::new, |_| "\n\n".into())
        }

        fn tool_key(id: &str, name: &str) -> String {
            if id.is_empty() {
                format!("name:{name}")
            } else {
                id.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ariadne_api::events::AgentEventDto;
    use serde_json::json;

    use super::{Since, fold, render};

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
    fn a_transcript_renders_one_full_block_per_item() {
        let long = "full agent text ".repeat(20);
        let events = vec![
            event(
                "prompt",
                "user_prompt_submit",
                json!({"text": "Run the tests", "source": "console"}),
            ),
            event(
                "thought",
                "agent_thought",
                json!({"text": "Check the seam"}),
            ),
            event("message", "agent_message", json!({"text": long})),
            event(
                "pre",
                "pre_tool_use",
                json!({
                    "tool_name": "Bash",
                    "tool_input": {"command": "cargo test"},
                    "acp": {"toolCallId": "call-1", "title": "Bash", "status": "pending",
                            "rawInput": {"command": "cargo test"}}
                }),
            ),
            event(
                "post",
                "post_tool_use",
                json!({
                    "tool_name": "Bash",
                    "tool_input": {"command": "cargo test"},
                    "acp": {"toolCallId": "call-1", "title": "Bash", "status": "completed",
                            "rawInput": {"command": "cargo test"},
                            "rawOutput": {"stdout": "all tests passed\n"}}
                }),
            ),
            event(
                "ask",
                "permission_request",
                json!({
                    "tool_name": "Write",
                    "tool_input": {"path": "src/main.rs"},
                    "options": [{"optionId": "yes", "name": "Allow"}]
                }),
            ),
            event("answer", "permission.replied", json!({"option_id": "yes"})),
        ];

        let items = fold(&events);
        let output = render::transcript(&items, Some(48), false);
        let local_time = chrono::DateTime::parse_from_rfc3339("2026-09-11T12:34:56Z")
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%H:%M:%S")
            .to_string();

        assert_eq!(
            items.len(),
            5,
            "the tool pair and permission pair each fold once"
        );
        assert_eq!(output.matches(&local_time).count(), 5, "{output}");
        assert!(output.contains("full agent text"), "{output}");
        assert!(output.contains("all tests passed"), "{output}");
        assert!(output.contains("answer: Allow"), "{output}");
        assert!(!output.contains('…'), "nothing is truncated: {output}");
    }

    #[test]
    fn a_tool_call_diff_uses_diff_colouring() {
        let patch = "@@ -1 +1 @@\n-old\n+new\n";
        let events = [event(
            "tool",
            "post_tool_use",
            json!({
                "tool_name": "Edit",
                "tool_input": {"path": "a.txt"},
                "acp": {"toolCallId": "call-1", "title": "Edit", "status": "completed",
                        "content": [{"type": "diff", "patch": {
                            "format": "git_patch", "text": patch
                        }}]}
            }),
        )];

        let output = render::transcript(&fold(&events), None, true);

        assert!(output.contains("\u{1b}[32m+new\u{1b}[0m"), "{output:?}");
        assert!(output.contains("\u{1b}[31m-old\u{1b}[0m"), "{output:?}");
    }

    #[test]
    fn a_plan_lists_each_entry_with_its_status_glyph() {
        let events = [event(
            "plan",
            "plan",
            json!({"entries": [
                {"content": "Inspect the code", "status": "completed"},
                {"content": "Run the tests", "status": "pending"}
            ]}),
        )];

        let output = render::transcript(&fold(&events), None, false);

        assert!(output.contains("✓ Inspect the code"), "{output}");
        assert!(output.contains("○ Run the tests"), "{output}");
    }

    #[test]
    fn no_color_gives_plain_transcript_text() {
        let events = [event("message", "agent_message", json!({"text": "plain"}))];

        let output = render::transcript(&fold(&events), None, false);

        assert!(!output.contains('\u{1b}'), "{output:?}");
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

    #[test]
    fn interleaved_tool_calls_keep_one_header_each() {
        let events = [
            event(
                "pre-1",
                "pre_tool_use",
                json!({"acp": {"toolCallId": "one", "title": "First", "status": "pending"}}),
            ),
            event(
                "pre-2",
                "pre_tool_use",
                json!({"acp": {"toolCallId": "two", "title": "Second", "status": "pending"}}),
            ),
            event(
                "post-1",
                "post_tool_use",
                json!({"acp": {"toolCallId": "one", "title": "First", "status": "completed",
                    "rawOutput": "first output"}}),
            ),
            event(
                "post-2",
                "post_tool_use",
                json!({"acp": {"toolCallId": "two", "title": "Second", "status": "completed",
                    "rawOutput": "second output"}}),
            ),
        ];
        let mut renderer = render::StreamRenderer::new(None, false);
        let mut output = String::new();
        for event in events {
            output.push_str(&renderer.event(&event, &super::Filters::default()));
        }

        assert_eq!(output.matches("TOOL").count(), 2, "{output}");
        assert!(output.contains("first output"), "{output}");
        assert!(output.contains("second output"), "{output}");
    }

    #[test]
    fn a_permission_reply_does_not_end_an_agent_chunk() {
        let events = [
            event(
                "permission",
                "permission_request",
                json!({"tool_name": "Write", "options": [{"optionId": "yes", "name": "Allow"}]}),
            ),
            event("chunk-1", "agent_message_chunk", json!({"text": "first "})),
            event("answer", "permission.replied", json!({"option_id": "yes"})),
            event("chunk-2", "agent_message_chunk", json!({"text": "second"})),
        ];
        let mut renderer = render::StreamRenderer::new(None, false);
        let mut output = String::new();
        for event in events {
            output.push_str(&renderer.event(&event, &super::Filters::default()));
        }

        assert_eq!(output.matches("AGENT").count(), 1, "{output}");
        assert!(output.contains("answer: Allow"), "{output}");
    }
}
