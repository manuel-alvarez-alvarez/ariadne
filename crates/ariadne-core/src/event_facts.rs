//! The facts of an agent event that a trajectory read needs without its
//! payload (spec 040).
//!
//! A payload is stored deflated, so no query can look inside it. The store
//! keeps these few fields beside it, in plain text, so that a read can count
//! a turn's calls, find its answer and pair its permissions from the facts
//! alone, and load the payloads of a bounded few events. The daemon's fold
//! takes its own step decisions from the same facts, so the two agree.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The facts of one event. A field an event does not have is absent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventFacts {
    /// The ACP `toolCallId` of a tool call event or a permission request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<String>,
    /// A tool call's ACP status. A `tool_call_abandoned` is `cancelled` for
    /// the reason `cancelled`, and `interrupted` for every other reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// A tool call's ACP kind, such as `edit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_kind: Option<String>,
    /// The paths a tool call names: its locations, and its diffs' paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// A tool call's title, its ACP locations with their lines, and the
    /// fields of its input that name its subject. Input that is present
    /// holds an object, empty where it names none of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<FactLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// Whether a tool call event carries output text to preview, and diffs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub output: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub diffs: bool,
    /// The `item_id` of a message or a thought.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// The subagent a `subagent_update` is about, and its state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// The question a `question` or `question.resolved` is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// The request of a legacy `agent_request` or its resolution, and
    /// whether the request held a question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub asks: bool,
    /// The error event a `session.diagnosis` is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<String>,
    /// Whether a permission request is about one of Ariadne's own tools,
    /// and the ACP kind of each option it offers, by option id.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ariadne: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    /// What a permission reply chose, who chose it, and the AI's label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// One ACP location of a tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactLocation {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
}

/// The input fields a call's subject reads: a command, a path, a line, a
/// search pattern and a URL. A command keeps its first line.
const SUBJECT_KEYS: &[&str] = &[
    "path",
    "file_path",
    "filePath",
    "file",
    "line",
    "pattern",
    "query",
    "regex",
    "directory",
    "cwd",
    "url",
];

/// The kinds that stay in the session's own lane whoever they name: the
/// session answers a permission and a question, and announces a subagent.
const MAIN_KINDS: &[&str] = &[
    "permission_request",
    "permission.replied",
    "question",
    "question.resolved",
    "subagent_update",
];

/// The lane of an event: the subagent its payload names, or none for the
/// session's own.
pub fn lane_of(kind: &str, payload: &Value) -> Option<String> {
    if MAIN_KINDS.contains(&kind) {
        return None;
    }
    text(payload, "/subagent/id")
}

/// The facts of an event.
pub fn facts_of(kind: &str, payload: &Value) -> EventFacts {
    let acp = payload.get("acp").unwrap_or(payload);
    let mut facts = EventFacts::default();
    match kind {
        "pre_tool_use" | "post_tool_use" | "tool_call_update" | "tool_call_abandoned" => {
            facts.call = text(acp, "/toolCallId").or_else(|| text(payload, "/tool_call_id"));
            facts.status = match kind {
                "tool_call_abandoned" => Some(match text(payload, "/reason").as_deref() {
                    Some("cancelled") => "cancelled".into(),
                    _ => "interrupted".into(),
                }),
                _ => text(acp, "/status"),
            };
            facts.tool_kind = text(acp, "/kind");
            facts.paths = paths_of(acp);
            facts.title = text(payload, "/tool_name").or_else(|| text(acp, "/title"));
            facts.locations = locations_of(acp);
            facts.input = payload
                .get("tool_input")
                .or_else(|| acp.get("rawInput"))
                .filter(|input| !input.is_null())
                .map(subject_input);
            facts.output = has_output(acp);
            facts.diffs =
                content_of(acp).any(|entry| text(entry, "/type").as_deref() == Some("diff"));
        }
        "agent_message" | "agent_thought" | "agent_message_chunk" | "agent_thought_chunk" => {
            facts.item = text(payload, "/item_id");
        }
        "subagent_update" => {
            facts.subagent = text(payload, "/subagent/id");
            facts.state = text(payload, "/state");
        }
        "question" => facts.question = text(payload, "/question/id"),
        "question.resolved" => facts.question = text(payload, "/question_id"),
        "agent_request" => {
            facts.request = text(payload, "/request_id");
            facts.asks = payload.get("question").is_some_and(Value::is_object);
        }
        "agent_request.resolved" => facts.request = text(payload, "/request_id"),
        "session.diagnosis" => facts.error = text(payload, "/error_event_id"),
        "stop" => facts.stop_reason = text(payload, "/stop_reason"),
        "compaction_update" => facts.compaction = text(payload, "/compaction_id"),
        "permission_request" => {
            facts.call = text(acp, "/toolCallId");
            let title = text(payload, "/tool_name").or_else(|| text(acp, "/title"));
            facts.ariadne =
                title.is_some_and(|title| title.to_ascii_lowercase().contains("ariadne"));
            facts.options = payload
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    Some((
                        text(option, "/optionId")?,
                        text(option, "/kind").unwrap_or_default(),
                    ))
                })
                .collect();
        }
        "permission.replied" => {
            facts.option = text(payload, "/option_id");
            facts.decided_by = text(payload, "/decided_by");
            facts.label = text(payload, "/label");
        }
        _ => {}
    }
    facts
}

impl EventFacts {
    /// Whether a permission reply is the daemon's own allow of the request
    /// with these facts: an option chosen with no person at the console that
    /// is not a denial.
    pub fn daemon_allowed(&self, request: &EventFacts) -> bool {
        let Some(option) = &self.option else {
            return false;
        };
        if self.decided_by.as_deref() == Some("console") || self.label.as_deref() == Some("deny") {
            return false;
        }
        match request.options.get(option).map(String::as_str) {
            Some("") | None => true,
            Some(kind) => kind.starts_with("allow"),
        }
    }
}

/// The paths a tool call names: its locations, then the paths of its diffs.
fn paths_of(acp: &Value) -> Vec<String> {
    let mut paths: Vec<String> = acp
        .get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| text(location, "/path"))
        .collect();
    for entry in acp
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if text(entry, "/type").as_deref() == Some("diff")
            && let Some(path) = text(entry, "/path")
            && !paths.contains(&path)
        {
            paths.push(path);
        }
    }
    paths
}

fn content_of(acp: &Value) -> impl Iterator<Item = &Value> {
    acp.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn locations_of(acp: &Value) -> Vec<FactLocation> {
    acp.get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| {
            Some(FactLocation {
                path: text(location, "/path")?,
                line: location.get("line").and_then(Value::as_u64),
            })
        })
        .collect()
}

/// The fields of a call's input that its subject reads, and no other.
fn subject_input(input: &Value) -> Value {
    let mut kept = serde_json::Map::new();
    match input.get("command") {
        Some(Value::String(command)) => {
            let first = command.lines().next().unwrap_or_default();
            kept.insert("command".into(), Value::String(first.to_string()));
        }
        Some(Value::Array(parts)) => {
            let parts: Vec<Value> = parts
                .iter()
                .filter(|part| part.is_string())
                .cloned()
                .collect();
            kept.insert("command".into(), Value::Array(parts));
        }
        _ => {}
    }
    for key in SUBJECT_KEYS {
        match input.get(*key) {
            Some(value @ Value::String(_)) => {
                kept.insert((*key).into(), value.clone());
            }
            Some(value) if *key == "line" && value.is_u64() => {
                kept.insert((*key).into(), value.clone());
            }
            _ => {}
        }
    }
    Value::Object(kept)
}

/// The output text of a call event that its preview reads: its non-empty
/// text contents, else its `rawOutput`. None where it has neither.
pub fn output_text(acp: &Value) -> Option<String> {
    let text: Vec<String> = content_of(acp)
        .filter(|entry| text(entry, "/type").as_deref() == Some("content"))
        .filter_map(|entry| text(entry, "/content/text"))
        .filter(|text| !text.is_empty())
        .collect();
    if !text.is_empty() {
        return Some(text.join("\n"));
    }
    match acp.get("rawOutput") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Object(object))
            if object.contains_key("stdout") || object.contains_key("stderr") =>
        {
            Some(
                ["stdout", "stderr"]
                    .into_iter()
                    .filter_map(|key| object.get(key)?.as_str())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
        Some(raw) => Some(raw.to_string()),
    }
}

/// Whether a call event gives its call a preview: output text whose first
/// lines are not empty.
fn has_output(acp: &Value) -> bool {
    output_text(acp).is_some_and(|whole| {
        let mut lines = whole.lines();
        match (lines.next(), lines.next()) {
            (None, _) => false,
            (Some(first), None) => !first.is_empty(),
            _ => true,
        }
    })
}

fn text(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{facts_of, lane_of};

    /// A call's facts are its id, status, kind and paths; the facts of a
    /// permission name its options' kinds; a subagent's own events are in
    /// its lane, and the permission it asks is in the session's.
    #[test]
    fn the_facts_hold_what_a_read_counts_by() {
        let call = facts_of(
            "post_tool_use",
            &json!({"acp": {"toolCallId": "c-1", "status": "completed", "kind": "edit",
                            "locations": [{"path": "a.rs"}],
                            "content": [{"type": "diff", "path": "b.rs"}]}}),
        );
        assert_eq!(call.call.as_deref(), Some("c-1"));
        assert_eq!(call.status.as_deref(), Some("completed"));
        assert_eq!(call.tool_kind.as_deref(), Some("edit"));
        assert_eq!(call.paths, ["a.rs", "b.rs"]);

        let abandoned = facts_of(
            "tool_call_abandoned",
            &json!({"tool_call_id": "c-2", "reason": "relaunch"}),
        );
        assert_eq!(abandoned.status.as_deref(), Some("interrupted"));

        let child = json!({"subagent": {"id": "child-1"}});
        assert_eq!(lane_of("pre_tool_use", &child).as_deref(), Some("child-1"));
        assert_eq!(lane_of("permission_request", &child), None);

        let request = facts_of(
            "permission_request",
            &json!({"tool_name": "mcp__ariadne__complete_step",
                    "options": [{"optionId": "yes", "kind": "allow_once"},
                                {"optionId": "no", "kind": "reject_once"}]}),
        );
        assert!(request.ariadne);
        let allow = facts_of(
            "permission.replied",
            &json!({"option_id": "yes", "decided_by": "auto"}),
        );
        let reject = facts_of(
            "permission.replied",
            &json!({"option_id": "no", "decided_by": "auto"}),
        );
        let person = facts_of(
            "permission.replied",
            &json!({"option_id": "yes", "decided_by": "console"}),
        );
        assert!(allow.daemon_allowed(&request));
        assert!(!reject.daemon_allowed(&request));
        assert!(!person.daemon_allowed(&request));
    }

    /// A call's facts hold its title, locations and the subject fields of
    /// its input, and whether it carries output and diffs, but never the
    /// output or the rest of the input.
    #[test]
    fn a_calls_facts_hold_its_small_fields_and_no_output() {
        let facts = facts_of(
            "tool_call_update",
            &json!({"acp": {"toolCallId": "c-1", "title": "Run", "kind": "execute",
                            "locations": [{"path": "a.rs", "line": 4}],
                            "rawInput": {"command": "make\nrm -rf", "content": "BIG-INPUT"},
                            "content": [{"type": "content",
                                         "content": {"type": "text", "text": "LOG-TEXT"}}]}}),
        );
        assert_eq!(facts.title.as_deref(), Some("Run"));
        assert_eq!(facts.locations[0].line, Some(4));
        assert_eq!(facts.input, Some(json!({"command": "make"})));
        assert!(facts.output);
        assert!(!facts.diffs);
        let text = serde_json::to_string(&facts).unwrap();
        assert!(!text.contains("LOG-TEXT") && !text.contains("BIG-INPUT"));
    }

    /// An event flags output only where its preview is not empty: an empty
    /// raw output and an empty stdout and stderr flag none.
    #[test]
    fn only_an_output_with_a_preview_is_flagged() {
        let flagged = |raw: serde_json::Value| {
            facts_of(
                "tool_call_update",
                &json!({"acp": {"toolCallId": "c", "rawOutput": raw}}),
            )
            .output
        };
        assert!(flagged(json!("useful output")));
        assert!(!flagged(json!("")));
        assert!(!flagged(json!("\n")));
        assert!(!flagged(json!({"stdout": "", "stderr": ""})));
        assert!(flagged(json!({"stdout": "", "stderr": "boom"})));
        assert!(flagged(json!({"exit": 0})));
    }

    /// The stored `tool_call_update` and `tool_call_abandoned` of the ticket
    /// contract, `{session_id, tool_call_id, acp}` and
    /// `{session_id, tool_call_id, reason, acp}`, give the facts of their call
    /// whether or not `acp` repeats the id.
    #[test]
    fn the_stored_call_updates_give_the_facts_of_their_call() {
        let update = facts_of(
            "tool_call_update",
            &json!({"session_id": "s", "tool_call_id": "c-1",
                    "acp": {"status": "in_progress", "kind": "edit",
                            "locations": [{"path": "a.rs"}]}}),
        );
        assert_eq!(update.call.as_deref(), Some("c-1"));
        assert_eq!(update.status.as_deref(), Some("in_progress"));
        assert_eq!(update.tool_kind.as_deref(), Some("edit"));
        assert_eq!(update.paths, ["a.rs"]);

        let cancelled = facts_of(
            "tool_call_abandoned",
            &json!({"session_id": "s", "tool_call_id": "c-2", "reason": "cancelled",
                    "acp": {"toolCallId": "c-2", "status": "in_progress", "kind": "execute"}}),
        );
        assert_eq!(cancelled.call.as_deref(), Some("c-2"));
        assert_eq!(
            cancelled.status.as_deref(),
            Some("cancelled"),
            "the reason wins"
        );
        assert_eq!(cancelled.tool_kind.as_deref(), Some("execute"));
        assert_eq!(
            lane_of("tool_call_abandoned", &json!({"tool_call_id": "c-2"})),
            None
        );
    }
}
