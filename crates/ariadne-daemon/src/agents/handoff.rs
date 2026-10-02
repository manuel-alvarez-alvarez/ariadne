//! Render a handoff text from a session's stored events.
//!
//! The text is the history a new agent reads before it goes on with a
//! session its predecessor cannot carry across: the events of the old
//! session, folded into the same blocks the console reads
//! ([`ariadne_console::transcript`]), and written out as one fenced entry
//! per block.

use serde_json::Value;

use ariadne_console::transcript::{PlanEntry, Tool, TranscriptItem, fold, tool_is_terminal};
use ariadne_store::AgentEvent;

use crate::http::convert::event_dto;

/// Opens and closes every entry's fence. Reserved: stripped from an entry's
/// own text first, so nothing a tool printed or a diff carried can forge one
/// of these and close the block early.
const FENCE_OPEN: char = '⟪';
const FENCE_CLOSE: char = '⟫';

/// A tool's output or a diff is folded to its last lines, with a count of
/// the hidden ones, the same as the console (008 rule 25).
const BODY_FOLD: usize = 10;

/// A prompt the daemon sent keeps its first lines, with a count of the rest
/// (008 rule 24).
const DAEMON_PROMPT_FOLD: usize = 6;

/// The opening line of every handoff: what the text is, and what the agent
/// that reads it owes.
const INTRO: &str =
    "This is the history of the session you continue. Go on from where it ended.\n\n";

/// Render `events` as the handoff text a new agent reads in place of the
/// conversation it cannot resume, kept under `budget` characters.
pub(crate) fn handoff_text(events: &[AgentEvent], budget: usize) -> String {
    let dtos: Vec<_> = events.iter().cloned().map(event_dto).collect();
    let entries: Vec<String> = fold(&dtos).iter().filter_map(render_entry).collect();
    apply_budget(entries, budget)
}

/// One transcript item as a fenced entry, or `None` for a kind the handoff
/// leaves out: a thought, and the notes for `stop`, `session_start` and
/// `session_end`.
fn render_entry(item: &TranscriptItem) -> Option<String> {
    let body = match item {
        TranscriptItem::UserPrompt { text, source, .. } => {
            user_prompt_body(text, source.as_deref())
        }
        TranscriptItem::AgentText { text, .. } => format!("agent\n{text}"),
        TranscriptItem::Thought { .. } => return None,
        TranscriptItem::Plan { entries, .. } => plan_body(entries),
        TranscriptItem::ToolCall { tool, .. } => tool_body(tool),
        TranscriptItem::PermissionQuestion { tool, answer, .. } => {
            permission_body(tool, answer.as_deref())
        }
        TranscriptItem::Error { text, .. } => format!("error\n{text}"),
        TranscriptItem::SystemNote { .. } | TranscriptItem::Raw { .. } => return None,
    };
    Some(fenced(body))
}

/// A `user_prompt_submit`: whole under the `user` marker from the console,
/// folded to its first lines with a count of the rest under the `daemon`
/// marker from the daemon.
fn user_prompt_body(text: &str, source: Option<&str>) -> String {
    if source == Some("daemon") {
        let (shown, hidden) = head_fold(text, DAEMON_PROMPT_FOLD);
        let mut body = format!("daemon\n{shown}");
        if hidden > 0 {
            let noun = if hidden == 1 { "line" } else { "lines" };
            body.push_str(&format!("\n… {hidden} more {noun}"));
        }
        body
    } else {
        format!("user\n{text}")
    }
}

/// A plan, as the console's checklist: a head that counts the completed
/// entries, then one glyph and line per entry.
fn plan_body(entries: &[PlanEntry]) -> String {
    let completed = entries
        .iter()
        .filter(|entry| entry.status == "completed")
        .count();
    let mut body = format!("plan\nplan {completed}/{}", entries.len());
    for entry in entries {
        let mark = match entry.status.as_str() {
            "completed" => "☑",
            "in_progress" => "◐",
            _ => "☐",
        };
        body.push_str(&format!("\n{mark} {}", entry.content));
    }
    body
}

/// A tool call: the head line the console draws for it, then its output and
/// its diff, each folded to their last lines with a count of what is
/// hidden. A call with no end — its last known status is not `completed` or
/// `failed` — renders its head alone: what it gave back is not known yet.
fn tool_body(tool: &Tool) -> String {
    let mut body = format!("tool\n{}", subject(tool));
    if tool_is_terminal(tool.status.as_deref()) {
        if let Some(output) = &tool.output {
            body.push('\n');
            body.push_str(&tail_fold(output, BODY_FOLD));
        }
        if let Some(diff) = &tool.diff {
            body.push('\n');
            body.push_str(&tail_fold(diff, BODY_FOLD));
        }
    }
    body
}

/// A permission request, as one line: the head of the call it asks about,
/// and the option the console chose.
fn permission_body(tool: &Tool, answer: Option<&str>) -> String {
    format!(
        "permission\n{} — {}",
        subject(tool),
        answer.unwrap_or("not yet answered")
    )
}

/// What a tool call is about, from its input: the command for `execute`, the
/// path and line for `read`, `edit`, `delete` and `move`, the pattern and
/// where it is looked for for `search`, the URL for `fetch`, and the call's
/// title where the kind names nothing. A location the result does not
/// already name follows it. This is the console's own head line (008 rule
/// 25), without the glyph and the elapsed time a terminal draws it with.
fn subject(tool: &Tool) -> String {
    let input = &tool.input;
    let named =
        match tool.kind.as_deref() {
            Some("execute") => tool_command(input)
                .map(|command| command.lines().next().unwrap_or_default().to_string()),
            Some("read" | "edit" | "delete" | "move") => place(tool),
            Some("search") => field(input, &["pattern", "query", "regex"]).map(|pattern| {
                match field(input, &["path", "file_path", "directory", "cwd"])
                    .or_else(|| tool.locations.first().map(|location| location.path.clone()))
                {
                    Some(path) => format!("{pattern} in {path}"),
                    None => pattern,
                }
            }),
            Some("fetch") => field(input, &["url"]),
            _ => None,
        };
    let mut head = named.unwrap_or_else(|| tool.name.clone());
    let elsewhere: Vec<String> = tool
        .locations
        .iter()
        .filter(|location| !head.contains(&location.path))
        .map(|location| match location.line {
            Some(line) => format!("{}:{line}", location.path),
            None => location.path.clone(),
        })
        .collect();
    if !elsewhere.is_empty() {
        head.push_str(&format!(" ({})", elsewhere.join(", ")));
    }
    head
}

/// The command an `execute` call runs: one string, or the argument list some
/// agents send it as.
fn tool_command(input: &Value) -> Option<String> {
    if let Value::String(text) = input {
        return Some(text.clone());
    }
    match input.get("command")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(words) => Some(
            words
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

/// The file a call is on, and the line where one is named: `path:line`.
fn place(tool: &Tool) -> Option<String> {
    let first = tool.locations.first();
    let path = field(&tool.input, &["path", "file_path", "filePath", "file"])
        .or_else(|| first.map(|location| location.path.clone()))?;
    let line = tool
        .input
        .get("line")
        .and_then(Value::as_u64)
        .or_else(|| first.filter(|location| location.path == path)?.line);
    Some(match line {
        Some(line) => format!("{path}:{line}"),
        None => path,
    })
}

/// The first of `keys` the input carries as a string.
fn field(input: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| input.get(key)?.as_str().map(str::to_string))
}

/// `text`'s first `limit` lines, and the count of the lines after them.
fn head_fold(text: &str, limit: usize) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= limit {
        return (text.to_string(), 0);
    }
    (lines[..limit].join("\n"), lines.len() - limit)
}

/// `text`'s last `limit` lines, with a count of the hidden ones ahead of
/// them. Trailing blank lines are dropped first, since they say nothing.
fn tail_fold(text: &str, limit: usize) -> String {
    let trimmed = text.trim_end_matches('\n');
    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() <= limit {
        return trimmed.to_string();
    }
    let hidden = lines.len() - limit;
    let noun = if hidden == 1 { "line" } else { "lines" };
    format!(
        "… {hidden} more {noun}\n{}",
        lines[lines.len() - limit..].join("\n")
    )
}

/// Wrap `body` in the one fence every entry uses, its own fence characters
/// stripped first so the body cannot forge one and close the block early.
fn fenced(body: String) -> String {
    let safe = body.replace(FENCE_OPEN, "<").replace(FENCE_CLOSE, ">");
    format!("{FENCE_OPEN}\n{safe}\n{FENCE_CLOSE}\n\n")
}

/// Keep the newest of `entries` under `budget` characters, counted after the
/// intro line, and drop the oldest. An entry is kept or dropped whole, never
/// cut, and a dropped run is replaced by one line that counts it. A budget
/// that fits every entry writes no such line.
///
/// The result never runs over `budget`, and never holds a partial intro or a
/// partial note: every candidate tried here is the intro, a whole note where
/// one is owed, and whole entries. Where no candidate fits — not even the
/// intro alone, or the intro with the note that every entry was dropped —
/// the handoff is empty rather than a line cut part-way through.
fn apply_budget(entries: Vec<String>, budget: usize) -> String {
    let intro_len = INTRO.chars().count();
    if entries.is_empty() {
        return if intro_len <= budget {
            INTRO.to_string()
        } else {
            String::new()
        };
    }
    let lengths: Vec<usize> = entries.iter().map(|entry| entry.chars().count()).collect();
    let total: usize = lengths.iter().sum();
    if intro_len + total <= budget {
        return format!("{INTRO}{}", entries.concat());
    }
    let count = entries.len();
    for kept in (0..count).rev() {
        let dropped = count - kept;
        let note = omitted_note(dropped);
        let tail_len: usize = lengths[count - kept..].iter().sum();
        if intro_len + note.chars().count() + tail_len <= budget {
            return format!("{INTRO}{note}{}", entries[count - kept..].concat());
        }
    }
    String::new()
}

/// The one line that stands in for a run of entries the budget dropped.
fn omitted_note(dropped: usize) -> String {
    let noun = if dropped == 1 { "entry" } else { "entries" };
    format!("… {dropped} earlier {noun} left out\n\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(id: &str, kind: &str, payload: Value) -> AgentEvent {
        AgentEvent {
            id: id.into(),
            session_id: Some("session".into()),
            task_id: None,
            kind: kind.into(),
            payload: payload.to_string(),
            created_at: "2026-10-03T12:00:00Z".into(),
        }
    }

    #[test]
    fn the_text_opens_with_the_history_line() {
        let out = handoff_text(&[], 10_000);
        assert_eq!(out, INTRO);
    }

    #[test]
    fn a_console_prompt_renders_whole_and_a_daemon_prompt_folds_to_six_lines_with_a_count() {
        let console_text = "do the task\nwith these notes";
        let daemon_lines: Vec<String> = (1..=9).map(|n| format!("daemon line {n}")).collect();
        let daemon_text = daemon_lines.join("\n");
        let events = [
            event(
                "a",
                "user_prompt_submit",
                json!({"text": console_text, "source": "console"}),
            ),
            event(
                "b",
                "user_prompt_submit",
                json!({"text": daemon_text, "source": "daemon"}),
            ),
        ];

        let out = handoff_text(&events, 10_000);

        assert!(out.contains(console_text), "{out}");
        assert!(out.contains("daemon line 1"), "{out}");
        assert!(out.contains("daemon line 6"), "{out}");
        assert!(!out.contains("daemon line 7"), "{out}");
        assert!(out.contains("… 3 more lines"), "{out}");
    }

    #[test]
    fn an_agent_message_renders_whole() {
        let text = "the plan is ready, moving on to the next step";
        let events = [event("a", "agent_message", json!({"text": text}))];

        let out = handoff_text(&events, 10_000);

        assert!(out.contains(text), "{out}");
    }

    #[test]
    fn a_thought_is_left_out() {
        let marker = "a thought nobody outside the turn should read";
        let events = [
            event("a", "agent_thought", json!({"text": marker})),
            event("b", "agent_message", json!({"text": "the visible answer"})),
        ];

        let out = handoff_text(&events, 10_000);

        assert!(!out.contains(marker), "{out}");
        assert!(out.contains("the visible answer"), "{out}");
    }

    #[test]
    fn a_tool_call_and_its_end_render_as_one_entry_with_the_head_and_the_folded_output() {
        let output_lines: Vec<String> = (1..=14).map(|n| format!("line {n}")).collect();
        let events = [
            event(
                "pre",
                "pre_tool_use",
                json!({"tool_name": "Bash", "acp": {"toolCallId": "call-1", "kind": "execute",
                       "status": "pending", "rawInput": {"command": "do the thing"}}}),
            ),
            event(
                "post",
                "post_tool_use",
                json!({"tool_name": "Bash", "acp": {"toolCallId": "call-1", "kind": "execute",
                       "status": "completed", "rawInput": {"command": "do the thing"},
                       "rawOutput": {"stdout": output_lines.join("\n")}}}),
            ),
        ];

        let out = handoff_text(&events, 10_000);

        assert_eq!(out.matches(FENCE_OPEN).count(), 1, "{out}");
        assert!(out.contains("do the thing"), "{out}");
        assert!(out.contains("line 14"), "{out}");
        assert!(!out.contains("line 1\n"), "{out}");
        assert!(out.contains("… 4 more lines"), "{out}");
    }

    #[test]
    fn a_diff_is_folded_the_same_way_as_output() {
        let hunk: Vec<String> = (1..=14).map(|n| format!("+line{n}")).collect();
        let patch = format!("@@ -0,0 +1,14 @@\n{}\n", hunk.join("\n"));
        let events = [event(
            "post",
            "post_tool_use",
            json!({"tool_name": "Edit",
                   "acp": {"toolCallId": "call-1", "kind": "edit", "status": "completed",
                           "content": [{"type": "diff", "path": "a.txt",
                                        "patch": {"format": "git_patch", "text": patch}}]}}),
        )];

        let out = handoff_text(&events, 10_000);

        assert!(out.contains("+line14"), "{out}");
        assert!(!out.contains("+line1\n"), "{out}");
        assert!(out.contains("… 7 more lines"), "{out}");
    }

    #[test]
    fn a_call_with_no_end_renders_its_head_alone() {
        let events = [event(
            "pre",
            "pre_tool_use",
            json!({"tool_name": "Bash", "acp": {"toolCallId": "call-1", "kind": "execute",
                   "status": "pending", "rawInput": {"command": "still running"},
                   "rawOutput": "should not appear"}}),
        )];

        let out = handoff_text(&events, 10_000);

        assert!(out.contains("still running"), "{out}");
        assert!(!out.contains("should not appear"), "{out}");
    }

    #[test]
    fn a_plan_renders_as_a_checklist() {
        let events = [event(
            "plan",
            "plan",
            json!({"entries": [
                {"content": "inspect the code", "status": "completed"},
                {"content": "run the tests", "status": "in_progress"},
                {"content": "commit the change", "status": "pending"}
            ]}),
        )];

        let out = handoff_text(&events, 10_000);

        assert!(out.contains("plan 1/3"), "{out}");
        assert!(out.contains("☑ inspect the code"), "{out}");
        assert!(out.contains("◐ run the tests"), "{out}");
        assert!(out.contains("☐ commit the change"), "{out}");
    }

    #[test]
    fn a_permission_request_renders_as_one_line_with_the_option_chosen() {
        let events = [
            event(
                "ask",
                "permission_request",
                json!({"tool_name": "Write", "tool_input": {"path": "src/main.rs"},
                       "kind": "edit",
                       "options": [{"optionId": "yes", "name": "Allow"}]}),
            ),
            event("answer", "permission.replied", json!({"option_id": "yes"})),
        ];

        let out = handoff_text(&events, 10_000);

        assert!(out.contains("src/main.rs"), "{out}");
        assert!(out.contains("— Allow"), "{out}");
    }

    #[test]
    fn a_session_error_renders_as_one_line_with_its_message() {
        let events = [event(
            "err",
            "session.error",
            json!({"error": {"message": "the agent process crashed"}}),
        )];

        let out = handoff_text(&events, 10_000);

        assert!(out.contains("the agent process crashed"), "{out}");
    }

    #[test]
    fn stop_session_start_and_session_end_are_left_out() {
        let events = [
            event("start", "session_start", json!({})),
            event("stop", "stop", json!({"stop_reason": "cancelled"})),
            event("end", "session_end", json!({})),
            event(
                "msg",
                "agent_message",
                json!({"text": "the only visible line"}),
            ),
        ];

        let out = handoff_text(&events, 10_000);

        assert_eq!(out.matches(FENCE_OPEN).count(), 1, "{out}");
        assert!(out.contains("the only visible line"), "{out}");
    }

    #[test]
    fn a_budget_of_zero_characters_produces_no_text_over_it() {
        let events = [event("a", "agent_message", json!({"text": "anything"}))];

        assert_eq!(handoff_text(&events, 0), "");
        assert_eq!(handoff_text(&[], 0), "");
    }

    #[test]
    fn a_budget_too_small_for_the_intro_returns_no_handoff() {
        let short = INTRO.chars().count() - 1;

        assert_eq!(handoff_text(&[], short), "");
    }

    #[test]
    fn a_budget_that_exactly_fits_the_intro_alone_returns_it_whole() {
        let exact = INTRO.chars().count();

        assert_eq!(handoff_text(&[], exact), INTRO);
    }

    #[test]
    fn a_low_budget_that_cannot_hold_a_whole_drop_count_line_returns_no_handoff() {
        let events = [event("a", "agent_message", json!({"text": "entry text"}))];
        let intro_len = INTRO.chars().count();

        // Enough for the intro alone, and one more character — not enough
        // for the shortest whole note a dropped entry needs. No partial
        // note is ever shown in its place.
        assert_eq!(handoff_text(&events, intro_len), "");
        assert_eq!(handoff_text(&events, intro_len + 1), "");
    }

    #[test]
    fn a_budget_that_exactly_fits_the_intro_and_a_whole_drop_note_returns_them() {
        let events: Vec<AgentEvent> = ["first", "second"]
            .into_iter()
            .enumerate()
            .map(|(i, word)| event(&i.to_string(), "agent_message", json!({"text": word})))
            .collect();
        let note = omitted_note(2);
        let budget = INTRO.chars().count() + note.chars().count();

        assert_eq!(handoff_text(&events, budget), format!("{INTRO}{note}"));
    }

    #[test]
    fn a_budget_that_fits_every_entry_writes_no_count_line() {
        let events = [event("a", "agent_message", json!({"text": "fits easily"}))];

        let full = handoff_text(&events, usize::MAX);

        assert!(!full.contains("left out"), "{full}");
        assert_eq!(handoff_text(&events, full.chars().count()), full);
    }

    #[test]
    fn the_newest_entries_survive_a_small_budget_and_one_line_counts_what_was_left_out() {
        let events: Vec<AgentEvent> = ["first", "second", "third", "fourth"]
            .into_iter()
            .enumerate()
            .map(|(i, word)| event(&i.to_string(), "agent_message", json!({"text": word})))
            .collect();

        let full = handoff_text(&events, usize::MAX);
        // The piece of `full` after the last entry's opening fence holds that
        // whole entry: everything up to the end of the text, since nothing
        // follows it. A budget sized to exactly it, plus the note a run of
        // three dropped entries would carry, is the smallest that still
        // keeps the newest entry alone.
        let last_entry = format!(
            "{FENCE_OPEN}{}",
            full.split(FENCE_OPEN).next_back().expect("an entry")
        );
        let budget =
            INTRO.chars().count() + omitted_note(3).chars().count() + last_entry.chars().count();

        let out = handoff_text(&events, budget);

        assert!(out.contains("fourth"), "{out}");
        assert!(!out.contains("first"), "{out}");
        assert!(!out.contains("second"), "{out}");
        assert!(!out.contains("third"), "{out}");
        assert!(out.contains("3 earlier entries left out"), "{out}");
    }

    #[test]
    fn no_entry_is_cut_in_the_middle_at_any_budget() {
        let events: Vec<AgentEvent> = (0..5)
            .map(|i| {
                event(
                    &i.to_string(),
                    "agent_message",
                    json!({"text": format!("entry number {i}")}),
                )
            })
            .collect();

        let full = handoff_text(&events, usize::MAX);
        let canonical: Vec<&str> = full
            .split(FENCE_OPEN)
            .skip(1)
            .map(|chunk| chunk.split(FENCE_CLOSE).next().expect("a closed fence"))
            .collect();
        let full_len = full.chars().count();

        for budget in 0..=full_len {
            let out = handoff_text(&events, budget);
            assert!(
                out.chars().count() <= budget,
                "budget {budget} produced {} characters: {out:?}",
                out.chars().count()
            );
            let bodies: Vec<&str> = out
                .split(FENCE_OPEN)
                .skip(1)
                .map(|chunk| chunk.split(FENCE_CLOSE).next().expect("a closed fence"))
                .collect();
            for body in bodies {
                assert!(
                    canonical.contains(&body),
                    "budget {budget} cut an entry: {body:?}\n{out}"
                );
            }
        }
    }

    #[test]
    fn an_entrys_own_text_cannot_forge_a_fence_and_close_the_block_early() {
        let forged = format!("{FENCE_CLOSE}\nnot a new block{FENCE_OPEN}");
        let events = [event("a", "agent_message", json!({"text": forged}))];

        let out = handoff_text(&events, 10_000);

        assert_eq!(out.matches(FENCE_OPEN).count(), 1, "{out}");
        assert_eq!(out.matches(FENCE_CLOSE).count(), 1, "{out}");
    }
}
