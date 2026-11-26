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
    /// The id of the event this block opened on — empty for a block the
    /// console synthesizes locally rather than folding from a stored event
    /// (a typed prompt shown before its round trip, a send that failed).
    /// What a late `session.diagnosis` correlates against (024).
    pub id: String,
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
            id: event.id.clone(),
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
    /// The ACP kind of the option — `allow_once`, `allow_always`,
    /// `reject_once`, `reject_always` — where the request named one.
    pub kind: Option<String>,
}

/// A file a tool call touches, as the ACP call names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: String,
    pub line: Option<u64>,
}

/// What a tool call reads as when its event names no tool.
const UNNAMED_TOOL: &str = "ACP tool";

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
        /// The lifecycle kind of a daemon prompt that has one, such as
        /// `agent_resume` for a resume nudge.
        prompt_kind: Option<String>,
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
        /// Why the AI permission model left the question to a person, where
        /// it had a part: `AI said ask · allow 62%, deny 5% · danger 21% (allow up to 5%, deny from 65%)`.
        ai_note: Option<String>,
    },
    SystemNote {
        meta: ItemMeta,
        text: String,
    },
    /// A question the agent asked with its own tool, an ACP
    /// `elicitation/create` (021): its form and, once it closed, how.
    FormQuestion {
        meta: ItemMeta,
        question_id: String,
        message: String,
        fields: Vec<FormField>,
        /// How it closed. None while it waits for an answer.
        closed: Option<FormClosed>,
    },
    Error {
        meta: ItemMeta,
        text: String,
        /// A late, optional advisory on this failure's likely category
        /// (024), labeled as the model's own suggestion: `AI suggests:
        /// quota exhaustion (72%)`. The original `text` is never rewritten
        /// to carry it.
        ai_note: Option<String>,
    },
    Raw {
        meta: ItemMeta,
        kind: String,
        /// The event's summary: what it was about, in a line.
        summary: String,
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
            | Self::FormQuestion { meta, .. }
            | Self::Error { meta, .. }
            | Self::Raw { meta, .. } => meta,
        }
    }
}

/// One field of a question's form, read off its `requestedSchema`.
#[derive(Debug, Clone, PartialEq)]
pub struct FormField {
    /// The property name, which the answer's content is keyed by.
    pub key: String,
    /// The field's `title`, else its key.
    pub title: String,
    pub description: Option<String>,
    pub required: bool,
    pub kind: FieldKind,
}

/// What a field takes.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    /// One of the choices: a `string` with `oneOf` or `enum`, or a boolean.
    Single(Vec<FormChoice>),
    /// Any of the choices: an `array` with `items.anyOf` or `items.enum`.
    Multi(Vec<FormChoice>),
    /// Free text.
    Text,
    /// A number, typed.
    Number { integer: bool },
}

/// One choice of a field: the value the answer carries, and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct FormChoice {
    pub value: Value,
    pub title: String,
    pub description: Option<String>,
}

/// How a form question closed, as its `question.resolved` event says.
#[derive(Debug, Clone, PartialEq)]
pub struct FormClosed {
    /// `accept`, `decline` or `cancel`.
    pub action: String,
    pub content: serde_json::Map<String, Value>,
    /// Who closed it: `user`, `agent` or `session`.
    pub closed_by: Option<String>,
}

impl FormClosed {
    /// How a `question.resolved` event says the question closed.
    pub fn of(payload: &Value) -> Self {
        Self {
            action: string_at(payload, "/action").unwrap_or_else(|| "cancel".into()),
            content: payload
                .get("content")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            closed_by: string_at(payload, "/closed_by"),
        }
    }
}

impl FormField {
    /// The text of each choice `value` names, as the agent wrote it.
    fn answer_text(&self, value: &Value) -> String {
        let choices = match &self.kind {
            FieldKind::Single(choices) | FieldKind::Multi(choices) => choices.as_slice(),
            _ => &[],
        };
        let one = |value: &Value| {
            choices
                .iter()
                .find(|choice| &choice.value == value)
                .map(|choice| choice.title.clone())
                .unwrap_or_else(|| match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
        };
        match value {
            Value::Array(values) if values.is_empty() => "(none)".into(),
            Value::Array(values) => values.iter().map(one).collect::<Vec<_>>().join(", "),
            value => one(value),
        }
    }
}

/// Pair only an adjacent plain string companion with its choice question.
pub(crate) fn companion<'a>(fields: &'a [FormField], field: &FormField) -> Option<&'a FormField> {
    let at = fields.iter().position(|f| f.key == field.key)?;
    let next = fields.get(at + 1)?;
    (matches!(field.kind, FieldKind::Single(_) | FieldKind::Multi(_))
        && next.key == format!("{}_custom", field.key)
        && matches!(next.kind, FieldKind::Text))
    .then_some(next)
}

pub(crate) fn questions(fields: &[FormField]) -> Vec<FormField> {
    fields
        .iter()
        .enumerate()
        .filter(|(at, _)| *at == 0 || companion(fields, &fields[*at - 1]).is_none())
        .map(|(_, field)| field.clone())
        .collect()
}

/// The fields of a form, in the order the schema lists them.
fn form_fields(schema: &Value, message: &str) -> Vec<FormField> {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(key, property)| FormField {
            key: key.clone(),
            title: string_at(property, "/title").unwrap_or_else(|| message.to_string()),
            description: string_at(property, "/description"),
            required: required.contains(&key.as_str()),
            kind: field_kind(property),
        })
        .collect()
}

fn field_kind(property: &Value) -> FieldKind {
    match property.get("type").and_then(Value::as_str) {
        Some("array") => FieldKind::Multi(choices(property.get("items").unwrap_or(&Value::Null))),
        Some("boolean") => FieldKind::Single(vec![
            FormChoice {
                value: Value::Bool(true),
                title: "Yes".into(),
                description: None,
            },
            FormChoice {
                value: Value::Bool(false),
                title: "No".into(),
                description: None,
            },
        ]),
        Some(kind @ ("number" | "integer")) => FieldKind::Number {
            integer: kind == "integer",
        },
        _ => match choices(property) {
            choices if choices.is_empty() => FieldKind::Text,
            choices => FieldKind::Single(choices),
        },
    }
}

/// The choices of a schema: `oneOf` or `anyOf` with `const` and `title`, or
/// a plain `enum`, with `enumNames` where it gives them.
fn choices(schema: &Value) -> Vec<FormChoice> {
    let titled = ["oneOf", "anyOf"]
        .iter()
        .find_map(|key| schema.get(*key).and_then(Value::as_array));
    if let Some(titled) = titled {
        return titled
            .iter()
            .filter_map(|choice| {
                let value = choice.get("const")?.clone();
                Some(FormChoice {
                    title: string_at(choice, "/title").unwrap_or_else(|| match &value {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    }),
                    description: string_at(choice, "/description"),
                    value,
                })
            })
            .collect();
    }
    let names = schema.get("enumNames").and_then(Value::as_array);
    schema
        .get("enum")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(at, value)| FormChoice {
            title: names
                .and_then(|names| names.get(at))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                }),
            description: None,
            value: value.clone(),
        })
        .collect()
}

impl TranscriptItem {
    /// A form question as the lines a person reads: the agent's message,
    /// each field with its choices as the agent wrote them, and how to
    /// answer or how it closed.
    pub fn form_lines(&self) -> Vec<String> {
        let Self::FormQuestion {
            message,
            fields,
            closed,
            ..
        } = self
        else {
            return Vec::new();
        };
        if let Some(closed) = closed {
            return closed_lines(closed, fields);
        }
        let mut lines = vec!["question".to_string()];
        lines.extend(message.lines().map(str::to_string));
        for (at, field) in fields.iter().enumerate() {
            lines.extend(field_lines(at, field));
        }
        match closed {
            None => lines.push("Answer it in the console.".into()),
            Some(closed) => lines.extend(closed_lines(closed, fields)),
        }
        lines
    }
}

/// One field: its number and title, its description, and its choices.
fn field_lines(at: usize, field: &FormField) -> Vec<String> {
    let mut head = format!("{}. {}", at + 1, field.title);
    match &field.kind {
        FieldKind::Single(_) => head.push_str(" (pick one)"),
        FieldKind::Multi(_) => head.push_str(" (pick any)"),
        FieldKind::Text | FieldKind::Number { .. } => {}
    }
    if !field.required {
        head.push_str(" (optional)");
    }
    let mut lines = vec![head];
    if let Some(description) = &field.description {
        lines.push(format!("   {description}"));
    }
    if let FieldKind::Single(choices) | FieldKind::Multi(choices) = &field.kind {
        for choice in choices {
            lines.push(match &choice.description {
                Some(description) => format!("   - {} — {description}", choice.title),
                None => format!("   - {}", choice.title),
            });
        }
    }
    lines
}

/// How a form question closed: each answer under its field's title, or the
/// way it closed without one.
pub fn closed_lines(closed: &FormClosed, fields: &[FormField]) -> Vec<String> {
    match closed.action.as_str() {
        "accept" => {
            let mut lines = vec!["answered".to_string()];
            for field in questions(fields) {
                let mut answers = Vec::new();
                if let Some(value) = closed.content.get(&field.key) {
                    answers.push(field.answer_text(value));
                }
                if let Some(custom) = companion(fields, &field)
                    .and_then(|custom| closed.content.get(&custom.key))
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    answers.push(custom.to_string());
                }
                lines.push(format!(
                    "  {} → {}",
                    field.title,
                    if answers.is_empty() {
                        "(no answer)".into()
                    } else {
                        answers.join(", ")
                    }
                ));
            }
            lines
        }
        "decline" => vec!["question declined".into()],
        _ => vec![match closed.closed_by.as_deref() {
            Some("agent") => "question withdrawn by the agent".into(),
            Some("session") => "question closed: the session ended".into(),
            _ => "question cancelled".into(),
        }],
    }
}

/// What a prompt reads as when its event carries no `text`.
const UNRECORDED_PROMPT: &str = "(prompt text not recorded)";

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
                prompt_kind: string_at(&event.payload, "/prompt_kind"),
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
                // A subagent's question (021) names the child that asks.
                let question = match string_at(&event.payload, "/subagent/title")
                    .or_else(|| string_at(&event.payload, "/subagent/id"))
                {
                    Some(child) => format!("{} · asked by subagent {child}", tool.name),
                    None => tool.name.clone(),
                };
                Self::PermissionQuestion {
                    meta,
                    question,
                    tool,
                    options: permission_options(&event.payload),
                    answer: None,
                    ai_note: ariadne_api::permissions::ai_permission_note(&event.payload),
                }
            }
            "session.error" => Self::Error {
                meta,
                text: error_text(event),
                ai_note: None,
            },
            "session_start" => Self::SystemNote {
                meta,
                text: "session started".into(),
            },
            "stop" => Self::SystemNote {
                meta,
                text: stop_note(string_at(&event.payload, "/stop_reason").as_deref()),
            },
            "compaction_update" => Self::SystemNote {
                meta,
                text: "context compacted".into(),
            },
            "question" => Self::FormQuestion {
                meta,
                question_id: string_at(&event.payload, "/question/id").unwrap_or_default(),
                message: string_at(&event.payload, "/question/message").unwrap_or_default(),
                fields: form_fields(
                    event
                        .payload
                        .pointer("/question/schema")
                        .unwrap_or(&Value::Null),
                    &string_at(&event.payload, "/question/message").unwrap_or_default(),
                ),
                closed: None,
            },
            "session_end" => Self::SystemNote {
                meta,
                text: "session ended".into(),
            },
            _ => Self::Raw {
                meta,
                kind: event.kind.clone(),
                summary: event.summary.clone(),
                payload: event.payload.clone(),
            },
        }
    }
}

/// The ACP stop reason of a turn that ended the way turns end.
const END_TURN: &str = "end_turn";

/// Why a turn stopped, in words: a cancel as `turn cancelled`, a limit or a
/// refusal as what happened, and any other reason with its underscores as
/// spaces.
fn stop_note(reason: Option<&str>) -> String {
    match reason {
        None => "turn stopped".into(),
        Some(END_TURN) => "turn ended".into(),
        Some("cancelled") => "turn cancelled".into(),
        Some("max_tokens") => "turn stopped: token limit reached".into(),
        Some("max_turn_requests") => "turn stopped: request limit reached".into(),
        Some("refusal") => "turn stopped: the agent refused".into(),
        Some(other) => format!("turn stopped: {}", other.replace('_', " ")),
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
    // A turn that ended the way turns end says nothing a reader needs: the
    // next block is the next turn.
    if event.kind == "stop"
        && string_at(&event.payload, "/stop_reason").as_deref() == Some(END_TURN)
    {
        return;
    }
    if event.kind == "permission.replied"
        && let Some(at) = items.iter().rposition(|item| {
            matches!(
                item,
                TranscriptItem::PermissionQuestion { answer: None, .. }
            )
        })
    {
        // A permission on one of Ariadne's own tools that the daemon allowed
        // by itself — its mode, the AI model or a learned rule, nobody at the
        // console — asked the person nothing: the call's own block already
        // says what the agent did, and a frame saying "permission · Allow"
        // around it is noise in the conversation. One a person did answer
        // stays, as the record of what they chose; and so does one the
        // daemon denied or cancelled, since the call's block then reads as a
        // failure with no cause, and the answer here is the cause.
        if let TranscriptItem::PermissionQuestion { tool, options, .. } = &items[at]
            && is_ariadne_tool(&tool.name)
            && daemon_allowed(&event.payload, options)
        {
            items.remove(at);
            return;
        }
        if let TranscriptItem::PermissionQuestion {
            meta,
            options,
            answer,
            ..
        } = &mut items[at]
        {
            *answer = Some(permission_answer(&event.payload, options));
            meta.kinds.push(event.kind.clone());
        }
        return;
    }
    // An advisory diagnosis (024) carries no block of its own: it reads
    // onto the `Error` block of the failure it names by id, however much
    // later it arrives, and onto nothing where that block has already
    // scrolled out of this snapshot.
    if event.kind == "session.diagnosis" {
        if let Some(error_event_id) = string_at(&event.payload, "/error_event_id")
            && let Some(TranscriptItem::Error { meta, ai_note, .. }) = items
                .iter_mut()
                .rev()
                .find(|item| matches!(item, TranscriptItem::Error { meta, .. } if meta.id == error_event_id))
        {
            *ai_note = ariadne_api::events::diagnosis_note(&event.payload);
            meta.kinds.push(event.kind.clone());
        }
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
    // A `wait_for_user` request that an older release recorded adds no
    // block: the request is gone, and its lines are history only.
    if matches!(
        event.kind.as_str(),
        "agent_request" | "agent_request.resolved"
    ) {
        return;
    }
    // A question that closed closes its form, and adds no block: a replay
    // draws it closed, never open again.
    if event.kind == "question.resolved" {
        let question = string_at(&event.payload, "/question_id");
        if let Some(TranscriptItem::FormQuestion { closed, meta, .. }) =
            items.iter_mut().rev().find(|previous| {
                matches!(previous, TranscriptItem::FormQuestion { question_id, .. }
                    if Some(question_id) == question.as_ref())
            })
        {
            *closed = Some(FormClosed::of(&event.payload));
            meta.kinds.push(event.kind.clone());
        }
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

/// Whether a reply is the daemon's own allow: an option chosen without a
/// person at the console, that is not a denial. A cancel (no option), a
/// console answer, an AI `deny`, and a chosen option whose kind is a reject
/// are each something a reader needs to see.
fn daemon_allowed(payload: &Value, options: &[PermissionOption]) -> bool {
    let Some(id) = string_at(payload, "/option_id") else {
        return false;
    };
    if string_at(payload, "/decided_by").as_deref() == Some("console")
        || string_at(payload, "/label").as_deref() == Some("deny")
    {
        return false;
    }
    match options
        .iter()
        .find(|option| option.id == id)
        .and_then(|option| option.kind.as_deref())
    {
        Some(kind) => kind.starts_with("allow"),
        None => true,
    }
}

/// Whether a call is one of Ariadne's own MCP tools, by the name the agent's
/// client gives it: `mcp__ariadne__complete_step` from Claude Code, and the
/// server's name in some other spelling from another client.
fn is_ariadne_tool(name: &str) -> bool {
    name.to_ascii_lowercase().contains("ariadne")
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

/// What a call gave back, in words: the text of its stored `content` entries,
/// then its `rawOutput` where that is text or a stdout and stderr pair, and
/// the structure as JSON only where there is neither.
fn tool_output(acp: &Value) -> Option<String> {
    let text: Vec<_> = acp
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("content"))
        .filter_map(|content| string_at(content, "/content/text"))
        .filter(|text| !text.is_empty())
        .collect();
    if !text.is_empty() {
        return Some(text.join("\n"));
    }
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

pub fn permission_options(payload: &Value) -> Vec<PermissionOption> {
    payload
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let id = string_at(option, "/optionId")?;
            let name = string_at(option, "/name").unwrap_or_else(|| id.clone());
            let kind = string_at(option, "/kind");
            Some(PermissionOption { id, name, kind })
        })
        .collect()
}

/// The name of the option a reply picked, its id where no option carries
/// that id, and `cancelled` where the reply picked none.
pub fn permission_answer(payload: &Value, options: &[PermissionOption]) -> String {
    let Some(id) = string_at(payload, "/option_id") else {
        return "cancelled".into();
    };
    if payload.get("decided_by").and_then(Value::as_str) == Some("ai")
        && let Some(note) = ariadne_api::permissions::ai_permission_note(payload)
    {
        return note;
    }
    let choice_id = string_at(payload, "/console_option_id").unwrap_or_else(|| id.clone());
    let answer = options
        .iter()
        .find(|option| option.id == choice_id)
        .map(|option| option.name.clone())
        .unwrap_or(id);
    if payload.get("decided_by").and_then(Value::as_str) == Some("learned") {
        match string_at(payload, "/learned_level").as_deref() {
            Some("command") => return format!("{answer}, learned · command"),
            Some("family") => {
                let family = string_at(payload, "/learned_key").unwrap_or_default();
                return format!("{answer}, learned · family {family}");
            }
            _ => {}
        }
    }
    answer
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

    /// A finished structured call keeps the text the person reads in its
    /// `content` entries, not its discarded raw structure.
    #[test]
    fn a_finished_structured_call_reads_its_content_text() {
        let events = [event(
            "tool",
            "post_tool_use",
            json!({
                "acp": {"toolCallId": "search", "title": "ToolSearch", "kind": "other",
                        "status": "completed",
                        "content": [
                            {"type": "content", "content": {"type": "text", "text": "Tool: list_tasks"}},
                            {"type": "content", "content": {"type": "text", "text": "Tool: finalize_plan"}}]}
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

    /// A finished call keeps its text in `content`, which takes precedence
    /// over the raw output that an old stored event can still carry.
    #[test]
    fn content_text_takes_precedence_over_raw_output() {
        let events = [event(
            "tool",
            "post_tool_use",
            json!({
                "acp": {"toolCallId": "shell", "title": "Bash", "kind": "execute",
                        "status": "completed", "rawOutput": {"stdout": "old output"},
                        "content": [{"type": "content", "content": {
                            "type": "text", "text": "stored output"}}]}
            }),
        )];

        let items = fold(&events);
        let TranscriptItem::ToolCall { tool, .. } = &items[0] else {
            panic!("{items:?}");
        };

        assert_eq!(tool.output.as_deref(), Some("stored output"));
    }

    /// Empty content does not hide the raw output that a finished call kept.
    #[test]
    fn empty_content_text_gives_way_to_raw_output() {
        let events = [event(
            "tool",
            "post_tool_use",
            json!({
                "acp": {"toolCallId": "shell", "title": "Bash", "kind": "execute",
                        "status": "completed", "rawOutput": {"stdout": "raw output"},
                        "content": [{"type": "content", "content": {
                            "type": "text", "text": ""}}]}
            }),
        )];

        let items = fold(&events);
        let TranscriptItem::ToolCall { tool, .. } = &items[0] else {
            panic!("{items:?}");
        };

        assert_eq!(tool.output.as_deref(), Some("raw output"));
    }

    /// The opener holds the input of a finished call after its terminal event
    /// has dropped `rawInput`.
    #[test]
    fn a_finished_call_reads_its_input_from_its_opener() {
        let events = [
            event(
                "pre",
                "pre_tool_use",
                json!({"tool_name": "Bash", "tool_input": {"command": "make"},
                       "acp": {"toolCallId": "call-1", "status": "pending"}}),
            ),
            event(
                "post",
                "post_tool_use",
                json!({"tool_name": "Bash", "acp": {"toolCallId": "call-1",
                       "status": "completed", "content": [{"type": "content",
                       "content": {"type": "text", "text": "built"}}]}}),
            ),
        ];

        let items = fold(&events);
        let TranscriptItem::ToolCall { tool, .. } = &items[0] else {
            panic!("{items:?}");
        };

        assert_eq!(tool.input, json!({"command": "make"}));
        assert_eq!(tool.output.as_deref(), Some("built"));
    }

    /// A `wait_for_user` request and its close, as an older release
    /// recorded them, draw no block between the lines around them.
    #[test]
    fn an_old_agent_request_draws_no_block() {
        let events = [
            event("before", "agent_message", json!({"text": "Before."})),
            event(
                "request",
                "agent_request",
                json!({"request_id": "01RQ", "hint": "Pick a base", "question": {
                    "text": "Which base branch?",
                    "options": [{"id": "main", "label": "Main"}]
                }}),
            ),
            event(
                "resolved",
                "agent_request.resolved",
                json!({"request_id": "01RQ", "resolution": "answered",
                       "answer": {"option_id": "main", "text": "Main"}}),
            ),
            event("after", "agent_message", json!({"text": "After."})),
        ];
        let items = fold(&events);
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(
            items
                .iter()
                .all(|item| matches!(item, TranscriptItem::AgentText { .. })),
            "{items:?}"
        );
    }

    /// A permission on one of Ariadne's own tools that the daemon allowed
    /// by itself leaves no block: the call's block says what happened. One
    /// a person answered at the console keeps its frame and their choice;
    /// so does one the daemon denied — the AI model's `deny`, or a learned
    /// rule's reject option — or cancelled, since that answer is the cause
    /// the call's failure otherwise lacks; and a permission on any other
    /// tool is untouched either way.
    #[test]
    fn an_automatic_permission_on_an_ariadne_tool_leaves_no_block() {
        let asked = |id: &str, tool: &str| {
            event(
                id,
                "permission_request",
                json!({"tool_name": tool, "waiting": true,
                       "options": [{"optionId": "allow-once", "name": "Allow", "kind": "allow_once"},
                                   {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"}],
                       "acp": {"toolCallId": id, "title": tool, "kind": "other"}}),
            )
        };
        let replied = |id: &str, decided_by: &str| {
            event(
                id,
                "permission.replied",
                json!({"option_id": "allow-once", "decided_by": decided_by}),
            )
        };

        let auto = fold(&[
            asked("ask-1", "mcp__ariadne__complete_step"),
            replied("reply-1", "auto"),
        ]);
        assert!(auto.is_empty(), "{auto:?}");

        let by_hand = fold(&[
            asked("ask-2", "mcp__ariadne__complete_step"),
            replied("reply-2", "console"),
        ]);
        assert!(
            matches!(
                &by_hand[0],
                TranscriptItem::PermissionQuestion {
                    answer: Some(_),
                    ..
                }
            ),
            "{by_hand:?}"
        );

        let other = fold(&[asked("ask-3", "Bash"), replied("reply-3", "auto")]);
        assert!(
            matches!(
                &other[0],
                TranscriptItem::PermissionQuestion {
                    answer: Some(_),
                    ..
                }
            ),
            "{other:?}"
        );

        let kept_answer = |items: &[TranscriptItem]| match &items[0] {
            TranscriptItem::PermissionQuestion {
                answer: Some(answer),
                ..
            } => answer.clone(),
            other => panic!("{other:?}"),
        };
        let ai_denied = fold(&[
            asked("ask-4", "mcp__ariadne__fail_task"),
            event(
                "reply-4",
                "permission.replied",
                json!({"option_id": "reject-once", "decided_by": "ai", "label": "deny",
                       "allow_threshold": 0.05, "deny_threshold": 0.65,
                       "probabilities": {"0": 0.02, "1": 0.9}, "danger": 0.7}),
            ),
        ]);
        assert!(
            kept_answer(&ai_denied).starts_with("AI denied"),
            "{ai_denied:?}"
        );

        let learned_reject = fold(&[
            asked("ask-5", "mcp__ariadne__fail_task"),
            event(
                "reply-5",
                "permission.replied",
                json!({"option_id": "reject-once", "decided_by": "learned"}),
            ),
        ]);
        assert_eq!(kept_answer(&learned_reject), "Reject");

        let cancelled = fold(&[
            asked("ask-6", "mcp__ariadne__complete_step"),
            event(
                "reply-6",
                "permission.replied",
                json!({"option_id": null, "decided_by": "auto"}),
            ),
        ]);
        assert_eq!(kept_answer(&cancelled), "cancelled");
    }

    /// A permission question from a subagent names the child that asks.
    #[test]
    fn a_subagents_question_names_the_child() {
        let asked = |payload| fold(&[event("ask", "permission_request", payload)]);
        let question = |items: Vec<TranscriptItem>| match &items[0] {
            TranscriptItem::PermissionQuestion { question, .. } => question.clone(),
            other => panic!("{other:?}"),
        };

        assert_eq!(
            question(asked(json!({"tool_name": "Bash",
                                  "subagent": {"id": "task-1", "title": "Explore the repo"}}))),
            "Bash · asked by subagent Explore the repo"
        );
        assert_eq!(question(asked(json!({"tool_name": "Bash"}))), "Bash");
    }

    #[test]
    fn a_stop_reads_in_words_and_an_ended_turn_is_no_block() {
        let stop = |reason: &str| event(reason, "stop", json!({"stop_reason": reason}));
        let note = |reason: &str| match fold(&[stop(reason)]).as_slice() {
            [TranscriptItem::SystemNote { text, .. }] => text.clone(),
            items => panic!("{items:?}"),
        };

        assert!(fold(&[stop("end_turn")]).is_empty());
        assert_eq!(note("cancelled"), "turn cancelled");
        assert_eq!(note("max_tokens"), "turn stopped: token limit reached");
        assert_eq!(note("some_new_reason"), "turn stopped: some new reason");
        assert_eq!(
            fold(&[event("stop", "stop", json!({}))]),
            [TranscriptItem::SystemNote {
                meta: super::ItemMeta::from_event(&event("stop", "stop", json!({}))),
                text: "turn stopped".into(),
            }]
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

    /// A late diagnosis (024) attaches to the error it names, as an
    /// `ai_note` beside the original text — never replacing it, and never a
    /// block of its own.
    #[test]
    fn a_late_diagnosis_attaches_to_the_error_it_names() {
        let events = [
            event(
                "err-1",
                "session.error",
                json!({"error": {"data": {"message": "rate limited"}}}),
            ),
            event(
                "diag-1",
                "session.diagnosis",
                json!({"error_event_id": "err-1", "category": "exhausted",
                       "probabilities": {"exhausted": 0.9}}),
            ),
        ];

        let items = fold(&events);

        assert_eq!(items.len(), 1, "the diagnosis opens no block of its own");
        let TranscriptItem::Error { text, ai_note, .. } = &items[0] else {
            panic!("{items:?}");
        };
        assert_eq!(text, "rate limited");
        assert_eq!(
            ai_note.as_deref(),
            Some("AI suggests: quota exhaustion (90%)")
        );
    }

    /// Two errors, each later diagnosed: the right note lands on the right
    /// one, by id, whichever order the diagnoses arrive in.
    #[test]
    fn duplicate_errors_each_keep_their_own_diagnosis() {
        let events = [
            event(
                "err-1",
                "session.error",
                json!({"error": {"message": "first"}}),
            ),
            event(
                "err-2",
                "session.error",
                json!({"error": {"message": "second"}}),
            ),
            event(
                "diag-2",
                "session.diagnosis",
                json!({"error_event_id": "err-2", "category": "task_error"}),
            ),
            event(
                "diag-1",
                "session.diagnosis",
                json!({"error_event_id": "err-1", "category": "temporary"}),
            ),
        ];

        let items = fold(&events);

        assert_eq!(items.len(), 2);
        let notes: Vec<Option<&str>> = items
            .iter()
            .map(|item| match item {
                TranscriptItem::Error { ai_note, .. } => ai_note.as_deref(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            notes,
            [
                Some("AI suggests: a temporary failure"),
                Some("AI suggests: a task failure")
            ]
        );
    }

    /// A diagnosis that names no error already in this snapshot — the
    /// block it is about has scrolled out, or it never arrived — attaches
    /// to nothing, and opens no block of its own either.
    #[test]
    fn a_diagnosis_with_no_matching_error_in_the_snapshot_attaches_to_nothing() {
        let events = [event(
            "diag-1",
            "session.diagnosis",
            json!({"error_event_id": "unknown", "category": "exhausted"}),
        )];

        assert_eq!(fold(&events), []);
    }
}
