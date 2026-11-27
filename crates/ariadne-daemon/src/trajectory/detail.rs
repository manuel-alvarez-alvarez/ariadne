//! The full detail of one tool call, and the bytes of one content block
//! (040): what a client reads on demand, past the preview of a step.
//!
//! The rules that read loose ACP tool content are the ones the two consoles
//! keep (`Tool` in `ariadne-console`'s transcript, and `toolOf`,
//! `toolOutput` and `toolDiff` in the web console's).

use base64::Engine as _;
use serde_json::{Map, Value};

use ariadne_api::events::AgentEventDto;
use ariadne_api::trajectory::{
    CommandDetailDto, DiffDetailDto, DiffFileDto, FetchDetailDto, FileDetailDto, GenericDetailDto,
    RawToolDto, SearchDetailDto, ToolCallDetailDto, ToolCallStatus, ToolDetailDto,
};
use ariadne_core::event_facts::EventFacts;
use ariadne_store::EventHead;

use super::steps::{blocks_of, is_terminal, merge_call, new_call, status_of, string_at};

/// The kinds of a tool call's events.
const CALL_KINDS: &[&str] = &[
    "pre_tool_use",
    "tool_call_update",
    "post_tool_use",
    "tool_call_abandoned",
];

/// The kinds that end a turn, and with it every call still open in it
/// (rule 12).
const TURN_ENDS: &[&str] = &[
    "user_prompt_submit",
    "stop",
    "session.error",
    "session_start",
    "session_end",
];

/// The kinds whose heads [`call_events`] walks.
pub(crate) fn detail_kinds() -> Vec<&'static str> {
    CALL_KINDS.iter().chain(TURN_ENDS).copied().collect()
}

/// The most output of a command one detail carries: the end of it, where
/// the result and the errors are. A megabyte is the most the ACP runtime
/// stores of one content block.
const OUTPUT_BYTES: usize = 1024 * 1024;

/// The events of one call, by their heads.
#[derive(Debug, PartialEq)]
pub(crate) struct CallEvents {
    /// The rows to load: the opener and the newest updates, oldest first,
    /// at most `budget`.
    pub(crate) load: Vec<String>,
    /// How many events changed the call after its opener: the step's
    /// `detail_rev`.
    pub(crate) updates: u64,
    /// The state the end of its turn left an open call in.
    pub(crate) ended: Option<ToolCallStatus>,
}

/// The events of the call the step `item_id` opened, from the heads of the
/// session from `item_id` on (of [`detail_kinds`]): the events of its lane
/// and its tool call id, up to the event that closes it, the next opener of
/// that id, or the end of its turn — as the fold joins them (rule 21). None
/// where `item_id` opens no call.
pub(crate) fn call_events(heads: &[EventHead], item_id: &str, budget: usize) -> Option<CallEvents> {
    let (opener, rest) = heads.split_first()?;
    if opener.id != item_id || !CALL_KINDS.contains(&opener.kind.as_str()) {
        return None;
    }
    let facts =
        |head: &EventHead| serde_json::from_str::<EventFacts>(&head.facts).unwrap_or_default();
    let first = facts(opener);
    let mut status = first.status.as_deref().and_then(status_of);
    let mut ids = vec![opener.id.clone()];
    let mut ended = None;
    for head in rest {
        if status.is_some_and(is_terminal) {
            break;
        }
        if TURN_ENDS.contains(&head.kind.as_str()) {
            let head_facts = facts(head);
            ended = Some(
                match (head.kind.as_str(), head_facts.stop_reason.as_deref()) {
                    ("stop", Some("cancelled")) => ToolCallStatus::Cancelled,
                    _ => ToolCallStatus::Interrupted,
                },
            );
            break;
        }
        let head_facts = facts(head);
        if head.lane != opener.lane || head_facts.call != first.call {
            continue;
        }
        if head.kind == "pre_tool_use" {
            break;
        }
        if let Some(next) = head_facts.status.as_deref().and_then(status_of) {
            status = Some(next);
        }
        ids.push(head.id.clone());
    }
    let updates = ids.len() as u64 - 1;
    let budget = budget.max(1);
    let load = if ids.len() > budget {
        let mut kept = vec![ids[0].clone()];
        kept.extend(ids[ids.len() - (budget - 1)..].iter().cloned());
        kept
    } else {
        ids
    };
    Some(CallEvents {
        load,
        updates,
        ended,
    })
}

/// Whether the event `item_id` opens a step as the fold opens them: `heads`
/// are the heads of the session from the turn of `item_id` on, of
/// [`detail_kinds`]. An update of a call that is open joins its step and
/// opens none (rule 21), so its id names no item.
pub(crate) fn opens_a_step(mut fold: super::Fold, heads: &[EventHead], item_id: &str) -> bool {
    for head in heads {
        fold.apply_head(head);
        if head.id == item_id {
            break;
        }
    }
    fold.index().iter().any(|turn| {
        turn.lanes
            .values()
            .any(|lane| lane.openers.iter().any(|opener| opener.step == item_id))
    })
}

/// The detail of a call from its loaded events, oldest first, the opener
/// first.
pub(crate) fn detail_of(
    session_id: &str,
    events: &[AgentEventDto],
    call: &CallEvents,
    raw: bool,
) -> Option<ToolCallDetailDto> {
    let opener = events.first()?;
    let acp_of = |event: &AgentEventDto| {
        let payload = &event.payload;
        payload.get("acp").unwrap_or(payload).clone()
    };
    let tool_call_id = string_at(&acp_of(opener), "/toolCallId")
        .or_else(|| string_at(&opener.payload, "/tool_call_id"))
        .unwrap_or_default();
    let mut step = new_call(opener, "", "", &tool_call_id);
    // Each stored event holds what the call says so far; a later field
    // replaces an earlier one, as the ACP call update does.
    let mut merged = Map::new();
    let mut content_event = opener.id.clone();
    for event in events {
        merge_call(&mut step, event);
        if let Value::Object(fields) = acp_of(event) {
            for (key, value) in fields {
                if value.is_null() {
                    continue;
                }
                if key == "content" {
                    content_event = event.id.clone();
                }
                merged.insert(key, value);
            }
        }
        if let Some(input) = event
            .payload
            .get("tool_input")
            .filter(|input| !input.is_null())
        {
            merged.insert("rawInput".into(), input.clone());
        }
    }
    // The end of the turn closes an open call, and that change counts in
    // its `detail_rev` as the fold counts it (rule 12).
    let mut detail_rev = call.updates;
    if !is_terminal(step.status)
        && let Some(ended) = call.ended
    {
        step.status = ended;
        detail_rev += 1;
    }
    let acp = Value::Object(merged);
    let input = acp.get("rawInput").cloned().unwrap_or(Value::Null);
    let detail = match step.kind.as_deref() {
        Some("execute") => command_detail(&acp, &input, &step.title),
        Some("edit") => ToolDetailDto::Diff(DiffDetailDto {
            files: diff_files(&acp),
        }),
        Some("read") => file_detail(&acp, &input),
        Some("search") => ToolDetailDto::Search(SearchDetailDto {
            query: field(&input, &["pattern", "query", "regex"]).or_else(|| {
                string_at(&acp, "/title")
                    .as_deref()
                    .or(Some(step.title.as_str()))
                    .and_then(title_query)
            }),
            text: output_of(&acp),
        }),
        Some("fetch") => ToolDetailDto::Fetch(FetchDetailDto {
            url: field(&input, &["url"]),
            text: output_of(&acp),
        }),
        _ => ToolDetailDto::Generic(GenericDetailDto {
            content: call_blocks(&acp, session_id, &content_event),
        }),
    };
    let present = |value: Option<&Value>| value.filter(|value| !value.is_null()).cloned();
    Some(ToolCallDetailDto {
        id: opener.id.clone(),
        tool_call_id,
        title: step.title,
        kind: step.kind,
        status: step.status,
        detail_rev,
        detail,
        raw: raw.then(|| RawToolDto {
            input: present(acp.get("rawInput")),
            output: present(acp.get("rawOutput")),
        }),
    })
}

fn command_detail(acp: &Value, input: &Value, title: &str) -> ToolDetailDto {
    let command = match input.get("command") {
        Some(Value::String(command)) => Some(command.clone()),
        Some(Value::Array(parts)) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    };
    let raw = acp.get("rawOutput").unwrap_or(&Value::Null);
    let exit_code = ["/exit_code", "/exitCode", "/metadata/exit"]
        .iter()
        .find_map(|pointer| raw.pointer(pointer).and_then(Value::as_i64))
        .or_else(|| {
            acp.pointer("/_meta/terminal_exit/exit_code")
                .and_then(Value::as_i64)
        });
    let (output, truncated) = match output_of(acp) {
        Some(text) => {
            let (text, truncated) = tail(text, OUTPUT_BYTES);
            (Some(text), truncated)
        }
        None => (None, false),
    };
    ToolDetailDto::Command(CommandDetailDto {
        command: command.unwrap_or_else(|| title.to_string()),
        cwd: field(input, &["cwd", "workdir", "working_directory"]),
        output,
        exit_code,
        truncated,
    })
}

/// The last `limit` bytes of `text`, from a character boundary, and whether
/// any was cut.
fn tail(text: String, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text, false);
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    (text[start..].to_string(), true)
}

/// Each file a call changes: its `diff` entries, each as the old and the
/// new text and as a unified diff. A patch the agent sent is the diff, with
/// the file header taken from the entry's path where it has none.
fn diff_files(acp: &Value) -> Vec<DiffFileDto> {
    entries(acp)
        .filter(|entry| string_at(entry, "/type").as_deref() == Some("diff"))
        .filter_map(|entry| {
            let path = string_at(entry, "/path");
            let old_text = string_at(entry, "/oldText");
            let new_text = string_at(entry, "/newText");
            let unified = match string_at(entry, "/patch/text") {
                Some(patch) => {
                    let headed = patch
                        .lines()
                        .take_while(|line| !line.starts_with("@@"))
                        .any(|line| line.starts_with("--- ") || line.starts_with("+++ "));
                    match &path {
                        Some(path) if !headed => format!("--- {path}\n+++ {path}\n{patch}"),
                        _ => patch,
                    }
                }
                None => {
                    let path = path.as_deref()?;
                    unified(
                        path,
                        old_text.as_deref().unwrap_or_default(),
                        new_text.as_deref().unwrap_or_default(),
                    )
                }
            };
            let path = path.or_else(|| patch_path(&unified)).unwrap_or_default();
            Some(DiffFileDto {
                language: language_of(&path),
                path,
                old_text,
                new_text,
                unified,
            })
        })
        .collect()
}

/// The hunks between two texts of one file, with three lines of context,
/// under the file header a patch carries: as the terminal console draws it.
fn unified(path: &str, old: &str, new: &str) -> String {
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .missing_newline_hint(false)
        .header(path, path)
        .to_string()
}

/// The path a patch's `+++` header names.
fn patch_path(patch: &str) -> Option<String> {
    patch.lines().find_map(|line| {
        let path = line.strip_prefix("+++ ")?;
        let path = path.strip_prefix("b/").unwrap_or(path);
        Some(path.trim().to_string())
    })
}

fn file_detail(acp: &Value, input: &Value) -> ToolDetailDto {
    let locations = super::steps::locations_of(acp);
    let first = locations.first();
    let path = field(input, &["path", "file_path", "filePath", "file"])
        .or_else(|| first.map(|location| location.path.clone()));
    let output = output_of(acp);
    let stripped = output.as_deref().and_then(numbered_text);
    let line = stripped
        .as_ref()
        .map(|(_, line)| *line)
        .or_else(|| first.and_then(|location| location.line))
        .or_else(|| {
            ["line", "offset"]
                .iter()
                .find_map(|key| input.get(*key)?.as_u64())
        });
    ToolDetailDto::File(FileDetailDto {
        language: path.as_deref().and_then(language_of),
        path,
        line,
        text: stripped.map(|(text, _)| text).or(output),
    })
}

/// The text of what a call gave back, read by its type alone: the text of
/// its content, else its `rawOutput` where that is text, a stdout and
/// stderr pair or an object with an output text, else the output of its
/// terminal. A structure with none of these is raw data, which only
/// `raw=true` gives; a detail never writes it out as JSON.
fn output_of(acp: &Value) -> Option<String> {
    let texts: Vec<String> = entries(acp)
        .filter(|entry| string_at(entry, "/type").as_deref() == Some("content"))
        .filter_map(|entry| string_at(entry, "/content/text"))
        .filter(|text| !text.is_empty())
        .collect();
    if !texts.is_empty() {
        return Some(texts.join("\n"));
    }
    let raw = acp.get("rawOutput").unwrap_or(&Value::Null);
    if let Some(text) = raw.as_str() {
        return Some(text.to_string());
    }
    if raw.is_object() {
        let streams: Vec<&str> = ["stdout", "stderr"]
            .iter()
            .filter_map(|key| raw.get(*key)?.as_str())
            .filter(|text| !text.is_empty())
            .collect();
        if !streams.is_empty() {
            return Some(streams.join("\n"));
        }
        if let Some(text) = ["output", "aggregated_output", "formatted_output", "text"]
            .iter()
            .find_map(|key| raw.get(*key)?.as_str())
        {
            return Some(text.to_string());
        }
    }
    [
        "/_meta/terminal_output/data",
        "/_meta/terminal_output_delta/data",
    ]
    .iter()
    .find_map(|pointer| acp.pointer(pointer)?.as_str())
    .map(str::to_string)
}

/// The query a search's title names, where its input names none: Codex
/// names a search it ran in a shell only so, as `Search for 'query'` or
/// `Search for 'query' in path`.
fn title_query(title: &str) -> Option<String> {
    let rest = title.strip_prefix("Search for '")?;
    let query = match rest.strip_suffix('\'') {
        Some(query) if !query.contains("' in ") => query,
        _ => &rest[..rest.rfind("' in ")?],
    };
    (!query.is_empty()).then(|| query.to_string())
}

/// The text a read gave back, without the agent's line numbers, and the
/// number of its first line, where every line of it carries one:
///
/// - Claude Code's lines sit in a fence of three backticks or more, each
///   after its number and a tab, and a notice such as `[File truncated: …]`
///   can follow them.
/// - OpenCode's lines follow a `Read file <path>, lines <from>-<to>` header,
///   each after its number, a colon and a space, and a notice such as
///   `[Output truncated. …]` can follow them. Its first line is the line it
///   read from, also where that is before the offset asked for.
///
/// A notice is no line of the file, and the text leaves it out.
fn numbered_text(output: &str) -> Option<(String, u64)> {
    let mut lines: Vec<&str> = output.split('\n').collect();
    let fence = |line: &str| line.len() >= 3 && line.trim_end().chars().all(|c| c == '`');
    // The wrapper names the format, and the format names the separator: a
    // text in no wrapper is the file's own, numbers and all.
    let separator = if lines.first().is_some_and(|line| fence(line)) {
        lines.remove(0);
        if lines.last().is_some_and(|line| fence(line)) {
            lines.pop();
        }
        "\t"
    } else if lines
        .first()
        .is_some_and(|line| line.starts_with("Read file ") && line.contains(", lines "))
    {
        lines.remove(0);
        ": "
    } else {
        return None;
    };
    let notice = |line: &str| {
        let line = line.trim();
        line.is_empty()
            || (line.starts_with('[') && line.ends_with(']'))
            || (line.starts_with('(') && line.ends_with(')'))
    };
    while lines.last().is_some_and(|line| notice(line)) {
        lines.pop();
    }
    let split = |line: &str| -> Option<(u64, String)> {
        let trimmed = line.trim_start();
        let digits = trimmed.len()
            - trimmed
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .len();
        if digits == 0 {
            return None;
        }
        let number = trimmed[..digits].parse::<u64>().ok()?;
        let rest = &trimmed[digits..];
        let text = rest
            .strip_prefix(separator)
            .or_else(|| (separator == ": " && rest == ":").then_some(""))?;
        Some((number, text.to_string()))
    };
    let parts: Vec<(u64, String)> = lines
        .iter()
        .map(|line| split(line))
        .collect::<Option<_>>()?;
    let first = parts.first()?.0;
    let text: Vec<String> = parts.into_iter().map(|(_, text)| text).collect();
    Some((text.join("\n"), first))
}

/// The content blocks of a call's `content` entries, each `blob` URL naming
/// the event that holds it and the entry's place in it.
fn call_blocks(
    acp: &Value,
    session_id: &str,
    event_id: &str,
) -> Vec<ariadne_api::trajectory::ContentBlockDto> {
    let blocks: Vec<Value> = entries(acp)
        .map(|entry| match string_at(entry, "/type").as_deref() {
            Some("content") => entry.get("content").cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        })
        .collect();
    blocks_of(&blocks, session_id, event_id)
}

fn entries(acp: &Value) -> impl Iterator<Item = &Value> {
    acp.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// The first of `keys` the input holds as a string.
fn field(input: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| string_at(input, &format!("/{key}")))
}

/// The language of a file by its extension, by the names the web console's
/// highlighter knows. None where the extension names no language.
pub(crate) fn language_of(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?;
    let lower = name.to_ascii_lowercase();
    let by_name = match lower.as_str() {
        "dockerfile" => Some("dockerfile"),
        "makefile" | "gnumakefile" => Some("makefile"),
        "cmakelists.txt" => Some("cmake"),
        _ => None,
    };
    if let Some(language) = by_name {
        return Some(language.into());
    }
    let (_, extension) = lower.rsplit_once('.')?;
    let language = match extension {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "py" | "pyi" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "c++",
        "cs" => "c#",
        "swift" => "swift",
        "rb" => "ruby",
        "php" => "php",
        "lua" => "lua",
        "sh" | "bash" | "zsh" => "shell",
        "ps1" => "powershell",
        "sql" => "sql",
        "json" | "jsonc" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "xml" => "xml",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "less" => "less",
        "md" | "markdown" => "markdown",
        "vue" => "vue",
        "svelte" => "svelte",
        "proto" => "protobuf",
        "diff" | "patch" => "diff",
        "hs" => "haskell",
        "ex" | "exs" => "elixir",
        "erl" => "erlang",
        "clj" => "clojure",
        "dart" => "dart",
        "r" => "r",
        "pl" => "perl",
        "nix" => "nix",
        _ => return None,
    };
    Some(language.into())
}

/// The bytes of one content block of a stored event, and its MIME type: the
/// block at `index` of its `content`, of a call's `content` entries, or of
/// a compaction's `summary`, as the `blob` URLs of the fold name them. None
/// where no image, audio or resource block with bytes is there.
pub(crate) fn blob_of(event: &AgentEventDto, index: usize) -> Option<(String, Vec<u8>)> {
    let payload = &event.payload;
    let block = if CALL_KINDS.contains(&event.kind.as_str()) {
        let acp = payload.get("acp").unwrap_or(payload);
        entries(acp).nth(index)?.get("content")?
    } else if event.kind == "compaction_update" {
        payload.get("summary")?.as_array()?.get(index)?
    } else {
        payload.get("content")?.as_array()?.get(index)?
    };
    let (data, mime) = match string_at(block, "/type").as_deref()? {
        "image" | "audio" => (string_at(block, "/data")?, string_at(block, "/mimeType")),
        "resource" => (
            string_at(block, "/resource/blob")?,
            string_at(block, "/resource/mimeType"),
        ),
        _ => return None,
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .ok()?;
    Some((
        mime.unwrap_or_else(|| "application/octet-stream".into()),
        bytes,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use ariadne_api::events::AgentEventDto;
    use ariadne_api::trajectory::{
        DiffFileDto, FetchDetailDto, FileDetailDto, SearchDetailDto, ToolCallStatus, ToolDetailDto,
    };
    use ariadne_store::EventHead;

    use super::{CallEvents, call_events, detail_of, opens_a_step};
    use crate::trajectory::Fold;

    fn event(id: &str, kind: &str, payload: Value) -> AgentEventDto {
        AgentEventDto {
            id: id.into(),
            session_id: Some("session".into()),
            task_id: None,
            kind: kind.into(),
            payload,
            summary: String::new(),
            created_at: "2026-11-28T00:00:00Z".into(),
        }
    }

    fn head(id: &str, kind: &str, facts: Value) -> EventHead {
        EventHead {
            id: id.into(),
            kind: kind.into(),
            lane: None,
            facts: facts.to_string(),
            created_at: "2026-11-28T00:00:00Z".into(),
        }
    }

    /// The detail of one finished call of `kind`, with `input` and `acp`.
    fn detail(kind: &str, input: Value, acp: Value) -> ToolDetailDto {
        let mut acp = acp;
        acp["toolCallId"] = "call-1".into();
        acp["kind"] = kind.into();
        acp["status"] = "completed".into();
        acp["rawInput"] = input;
        let events = [event("0001", "pre_tool_use", json!({"acp": acp}))];
        let call = CallEvents {
            load: vec!["0001".into()],
            updates: 0,
            ended: None,
        };
        detail_of("session", &events, &call, false).unwrap().detail
    }

    /// An edit gives each file its old and new text, a unified diff and its
    /// language; a patch keeps its own lines under the entry's path
    /// (rule 50).
    #[test]
    fn an_edit_gives_each_file_its_texts_its_diff_and_its_language() {
        let ToolDetailDto::Diff(diff) = detail(
            "edit",
            json!({}),
            json!({"content": [
                {"type": "diff", "path": "src/lib.rs", "oldText": "a\nb\n", "newText": "a\nc\n"},
                {"type": "diff", "path": "notes.md",
                 "patch": {"format": "git_patch", "text": "@@ -1 +1 @@\n-x\n+y\n"}},
            ]}),
        ) else {
            panic!("an edit is a diff");
        };

        assert_eq!(
            diff.files,
            [
                DiffFileDto {
                    path: "src/lib.rs".into(),
                    old_text: Some("a\nb\n".into()),
                    new_text: Some("a\nc\n".into()),
                    unified: "--- src/lib.rs\n+++ src/lib.rs\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n".into(),
                    language: Some("rust".into()),
                },
                DiffFileDto {
                    path: "notes.md".into(),
                    old_text: None,
                    new_text: None,
                    unified: "--- notes.md\n+++ notes.md\n@@ -1 +1 @@\n-x\n+y\n".into(),
                    language: Some("markdown".into()),
                },
            ]
        );
    }

    /// A read gives its path, the number of its first line, its language and
    /// its text without the agent's line numbers (rule 50).
    #[test]
    fn a_read_gives_its_path_first_line_language_and_text() {
        let output = "```\n    41\tfn main() {\n    42\t}\n```";

        let detail = detail(
            "read",
            json!({"file_path": "src/main.rs"}),
            json!({"content": [{"type": "content", "content": {"type": "text", "text": output}}]}),
        );

        assert_eq!(
            detail,
            ToolDetailDto::File(FileDetailDto {
                path: Some("src/main.rs".into()),
                line: Some(41),
                language: Some("rust".into()),
                text: Some("fn main() {\n}".into()),
            })
        );
    }

    /// A search gives its query and its result, and a fetch its URL and its
    /// result (rule 50).
    #[test]
    fn a_search_and_a_fetch_give_their_subject_and_their_result() {
        let result = |text: &str| json!({"content": [{"type": "content", "content": {"type": "text", "text": text}}]});

        let search = detail("search", json!({"pattern": "TODO"}), result("a.rs:1"));
        let fetch = detail(
            "fetch",
            json!({"url": "https://example.org"}),
            json!({"rawOutput": "<html>"}),
        );

        assert_eq!(
            search,
            ToolDetailDto::Search(SearchDetailDto {
                query: Some("TODO".into()),
                text: Some("a.rs:1".into()),
            })
        );
        assert_eq!(
            fetch,
            ToolDetailDto::Fetch(FetchDetailDto {
                url: Some("https://example.org".into()),
                text: Some("<html>".into()),
            })
        );
    }

    /// A call's events are those of its lane and id up to the end of its
    /// turn, which leaves an open call interrupted; another call's events
    /// and the next opener of its id are not its own (rule 49).
    #[test]
    fn a_call_ends_at_the_end_of_its_turn_and_skips_other_calls() {
        let heads = [
            head(
                "0001",
                "pre_tool_use",
                json!({"call": "c-1", "status": "pending"}),
            ),
            head(
                "0002",
                "tool_call_update",
                json!({"call": "c-2", "status": "in_progress"}),
            ),
            head(
                "0003",
                "tool_call_update",
                json!({"call": "c-1", "status": "in_progress"}),
            ),
            head("0004", "user_prompt_submit", json!({})),
            head(
                "0005",
                "tool_call_update",
                json!({"call": "c-1", "status": "completed"}),
            ),
        ];

        let call = call_events(&heads, "0001", 500).unwrap();

        assert_eq!(
            call,
            CallEvents {
                load: vec!["0001".into(), "0003".into()],
                updates: 1,
                ended: Some(ToolCallStatus::Interrupted),
            }
        );
        assert_eq!(
            call_events(&heads, "0004", 500),
            None,
            "a prompt is no call"
        );
    }

    /// A call of more updates than the budget loads its opener and its
    /// newest updates, and counts every update (rule 49).
    #[test]
    fn a_long_call_loads_its_opener_and_its_newest_updates() {
        let mut heads = vec![head("0000", "pre_tool_use", json!({"call": "c-1"}))];
        for n in 1..=9 {
            heads.push(head(
                &format!("000{n}"),
                "tool_call_update",
                json!({"call": "c-1"}),
            ));
        }

        let call = call_events(&heads, "0000", 3).unwrap();

        assert_eq!(call.load, ["0000", "0008", "0009"]);
        assert_eq!(call.updates, 9);
        assert_eq!(call.ended, None);
    }

    /// Codex reports a search it ran in a shell with its query in the title
    /// alone, and its output as a raw text (rule 50).
    #[test]
    fn a_codex_shell_search_takes_its_query_from_its_title() {
        let search = |title: &str| {
            detail(
                "search",
                Value::Null,
                json!({"title": title, "rawOutput": "src/a.rs:3: TODO"}),
            )
        };

        assert_eq!(
            search("Search for 'TODO' in src"),
            ToolDetailDto::Search(SearchDetailDto {
                query: Some("TODO".into()),
                text: Some("src/a.rs:3: TODO".into()),
            })
        );
        let ToolDetailDto::Search(bare) = search("Search for 'it's here'") else {
            panic!("a search");
        };
        assert_eq!(bare.query.as_deref(), Some("it's here"));
        let ToolDetailDto::Search(untitled) = search("Search") else {
            panic!("a search");
        };
        assert_eq!(untitled.query, None);
    }

    /// OpenCode numbers each line after a header that names the range it
    /// read, which can start before the offset asked for: the text loses
    /// the header, the numbers and the notice, and the line is the first it
    /// read (rule 50).
    #[test]
    fn an_opencode_read_gives_the_range_it_read() {
        let output = "Read file src/a.rs, lines 8-9 (offset 20 is past the end; the file has 9 lines)\n8: fn a() {\n9: }\n[Output truncated. Continue reading with offset: 10]";

        let detail = detail(
            "read",
            json!({"path": "src/a.rs", "offset": 20}),
            json!({"content": [{"type": "content", "content": {"type": "text", "text": output}}]}),
        );

        assert_eq!(
            detail,
            ToolDetailDto::File(FileDetailDto {
                path: Some("src/a.rs".into()),
                line: Some(8),
                language: Some("rust".into()),
                text: Some("fn a() {\n}".into()),
            })
        );
    }

    /// Claude Code can put a truncation notice after its numbered lines in
    /// the fence: the lines and the first number stay (rule 50).
    #[test]
    fn a_claude_read_keeps_its_lines_before_a_truncation_notice() {
        let output = "````\n     5\tlet a = 1;\n     6\tlet b = 2;\n[File truncated: showing 2 of 90 lines]\n````";

        let ToolDetailDto::File(file) = detail(
            "read",
            json!({"file_path": "src/a.rs"}),
            json!({"content": [{"type": "content", "content": {"type": "text", "text": output}}]}),
        ) else {
            panic!("a read is a file");
        };

        assert_eq!(file.line, Some(5));
        assert_eq!(file.text.as_deref(), Some("let a = 1;\nlet b = 2;"));
    }

    /// A raw output that is no text, no stdout and stderr and no output text
    /// is raw data: a typed detail does not write it out (rule 51).
    #[test]
    fn an_opaque_raw_output_stays_out_of_a_typed_detail() {
        let opaque = json!({"rawOutput": {"status": 200, "body": {"token": "secret"}}});

        let command = detail("execute", json!({"command": "curl x"}), opaque.clone());
        let fetch = detail("fetch", json!({"url": "https://x"}), opaque);

        let ToolDetailDto::Command(command) = command else {
            panic!("a command");
        };
        assert_eq!(command.output, None);
        assert_eq!(
            fetch,
            ToolDetailDto::Fetch(FetchDetailDto {
                url: Some("https://x".into()),
                text: None,
            })
        );
    }

    /// An opener opens a step, and so does an update of a call that is not
    /// open; an update or the end of an open call joins its step and opens
    /// none (rules 21, 54).
    #[test]
    fn only_an_event_that_opens_a_step_is_an_item() {
        let heads = [
            head("0001", "user_prompt_submit", json!({})),
            head(
                "0002",
                "pre_tool_use",
                json!({"call": "c-1", "status": "pending"}),
            ),
            head(
                "0003",
                "tool_call_update",
                json!({"call": "c-1", "status": "in_progress"}),
            ),
            head(
                "0004",
                "post_tool_use",
                json!({"call": "c-1", "status": "completed"}),
            ),
            head(
                "0005",
                "tool_call_update",
                json!({"call": "c-2", "status": "in_progress"}),
            ),
        ];
        let opens = |id: &str| {
            let fold = Fold::new(
                "session",
                ariadne_api::trajectory::SessionStateDto {
                    status: ariadne_core::SessionStatus::Running,
                    attention_reason: None,
                    running_turn_id: None,
                    title: None,
                    mode: None,
                    config_options: Vec::new(),
                    context: None,
                    usage: Default::default(),
                    cost: None,
                    commands: Vec::new(),
                },
            );
            opens_a_step(fold, &heads, id)
        };

        assert!(opens("0002"), "the opener");
        assert!(!opens("0003"), "a progress row");
        assert!(!opens("0004"), "the completion");
        assert!(opens("0005"), "an update with no open call");
    }

    /// A read whose text sits in no wrapper of a known agent is the file's
    /// own text: a line that starts with a number keeps it, and the line is
    /// the call's location line (rule 50).
    #[test]
    fn an_unwrapped_read_keeps_its_text_as_it_is() {
        let ToolDetailDto::File(file) = detail(
            "read",
            json!({"file_path": "notes.txt"}),
            json!({"locations": [{"path": "notes.txt", "line": 1}],
                   "content": [{"type": "content",
                                "content": {"type": "text", "text": "12: keep this"}}]}),
        ) else {
            panic!("a read is a file");
        };

        assert_eq!(file.text.as_deref(), Some("12: keep this"));
        assert_eq!(file.line, Some(1));
    }
}
