//! The blocks of the transcript as the lines they are drawn on: a prompt, the
//! agent's text, a thought, a plan, a tool call with its output and diff.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::markdown;
use crate::theme::{self, ADDED, AGENT, DAEMON, DIM, FAIL, FILE, HUNK, PLAN, REMOVED, TOOL, USER};
use crate::transcript::{Tool, TranscriptItem};

use super::picker::permission;

/// A thought and a tool's output are context, not the answer: they are folded
/// to this many lines with a count of what was left out. The output keeps its
/// last lines, which is where a command says how it went.
const FOLD: usize = 4;
/// A diff is read from the top, so it keeps its first lines: this many, with
/// a count of what was left out.
const DIFF_FOLD: usize = 24;

/// One transcript item as the lines the console draws it on.
///
/// `picked` is the highlighted option of a permission question being answered
/// right now — `None` everywhere else, including once it is in the scrollback.
pub(super) fn block(
    item: &TranscriptItem,
    width: usize,
    picked: Option<usize>,
) -> Vec<Line<'static>> {
    let width = width.max(8);
    match item {
        TranscriptItem::UserPrompt { text, source, .. } if source.as_deref() == Some("daemon") => {
            daemon_prompt(text, width)
        }
        TranscriptItem::UserPrompt { text, .. } => {
            prefixed(text, theme::USER_MARKER, USER, USER, width, None)
        }
        TranscriptItem::AgentText { text, .. } => {
            let mut lines = vec![Line::from(Span::styled(theme::AGENT_MARKER, AGENT))];
            for line in markdown::render(text, width.saturating_sub(2)) {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(line.spans);
                lines.push(Line::from(spans));
            }
            // The bullet marks where the agent started speaking; its first
            // line of text rides on it rather than under it.
            join_first(lines)
        }
        TranscriptItem::Thought { text, .. } => {
            prefixed(text, theme::THOUGHT_MARKER, DIM, DIM, width, Some(FOLD))
        }
        TranscriptItem::Plan { entries, .. } => {
            let mut lines = vec![Line::from(Span::styled("plan", PLAN))];
            for entry in entries {
                let done = entry.status == "completed";
                lines.push(Line::from(vec![
                    Span::styled(
                        if done {
                            theme::PLAN_DONE
                        } else {
                            theme::PLAN_TODO
                        },
                        PLAN,
                    ),
                    Span::styled(entry.content.clone(), if done { DIM } else { Style::new() }),
                ]));
            }
            lines
        }
        TranscriptItem::ToolCall { meta, tool } => call(tool, &meta.created_at, width),
        TranscriptItem::PermissionQuestion {
            question,
            tool,
            options,
            answer,
            ..
        } => permission(
            question,
            tool,
            options,
            answer.as_deref(),
            picked,
            width,
            DIFF_FOLD,
        ),
        TranscriptItem::SystemNote { text, .. } => prefixed(text, "  ", DIM, DIM, width, None),
        TranscriptItem::Error { text, .. } => {
            prefixed(text, theme::ERROR_MARKER, FAIL, FAIL, width, None)
        }
        TranscriptItem::Raw { kind, .. } => {
            vec![Line::from(Span::styled(kind.clone(), DIM))]
        }
    }
}

/// A prompt the daemon itself sent — a briefing, a nudge, a message — under
/// its own marker and label, so it reads apart from what was typed.
fn daemon_prompt(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(theme::DAEMON_MARKER, DAEMON))];
    for line in wrap(text, width.saturating_sub(2)) {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(line, DAEMON),
        ]));
    }
    lines
}

/// Pull the first line of text up onto the marker line above it.
fn join_first(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if lines.len() > 1 {
        let second = lines.remove(1);
        let marker = lines[0].spans.clone();
        let mut spans = marker;
        spans.extend(second.spans.into_iter().skip(1));
        lines[0] = Line::from(spans);
    }
    lines
}

/// A block of plain text behind a two-column marker, wrapped and optionally
/// folded to a few lines.
fn prefixed(
    text: &str,
    marker: &str,
    marker_style: Style,
    style: Style,
    width: usize,
    fold: Option<usize>,
) -> Vec<Line<'static>> {
    let mut wrapped = wrap(text, width.saturating_sub(marker.width()));
    let hidden = fold.map_or(0, |fold| wrapped.len().saturating_sub(fold));
    if hidden > 0 {
        wrapped.truncate(fold.unwrap_or(wrapped.len()));
        wrapped.push(format!("… {hidden} more lines"));
    }
    wrapped
        .into_iter()
        .enumerate()
        .map(|(at, line)| {
            let lead = if at == 0 {
                Span::styled(marker.to_string(), marker_style)
            } else {
                Span::raw(" ".repeat(marker.width()))
            };
            Line::from(vec![lead, Span::styled(line, style)])
        })
        .collect()
}

/// A tool call, the way a coding agent's console reads one: what it did, on
/// what, and how it went. The head line, then the output folded to its last
/// lines and the file change as a diff.
fn call(tool: &Tool, started_at: &str, width: usize) -> Vec<Line<'static>> {
    let (glyph, style) = match tool.status.as_deref() {
        Some("completed") => (theme::CALL_DONE, AGENT),
        Some("failed") => (theme::CALL_FAILED, FAIL),
        Some("in_progress") => (theme::CALL_RUNNING, TOOL),
        _ => (theme::CALL_PENDING, TOOL),
    };
    let elapsed = tool
        .ended_at
        .as_deref()
        .and_then(|ended_at| elapsed(started_at, ended_at));
    let mut lines = vec![head(tool, Span::styled(glyph, style), elapsed, width)];
    if let Some(output) = &tool.output {
        lines.extend(folded(output, width, DIM));
    }
    if let Some(diff) = &tool.diff {
        lines.extend(folded_diff(diff, width, DIFF_FOLD));
    }
    lines
}

/// The head line of a call: a status mark, the kind's glyph, the subject —
/// what the call is about, from its input — and how long it took, once it
/// has ended.
pub(super) fn head(
    tool: &Tool,
    mark: Span<'static>,
    elapsed: Option<String>,
    width: usize,
) -> Line<'static> {
    let lead = mark.content.width() + 1;
    let tail = elapsed.as_ref().map_or(0, |elapsed| elapsed.width() + 2);
    let room = width.saturating_sub(lead + 2 + tail).max(1);
    let mut spans = vec![
        mark,
        Span::raw(" "),
        Span::styled(format!("{} ", kind_glyph(tool.kind.as_deref())), TOOL),
        Span::raw(clip(&subject(tool), room)),
    ];
    if let Some(elapsed) = elapsed {
        spans.push(Span::styled(format!("  {elapsed}"), DIM));
    }
    Line::from(spans)
}

/// One glyph per ACP kind, so the eye tells a command from a read from an
/// edit before reading a word.
fn kind_glyph(kind: Option<&str>) -> &'static str {
    theme::KINDS
        .iter()
        .find(|(name, _)| Some(*name) == kind)
        .map_or(theme::KIND_OTHER, |(_, glyph)| glyph)
}

/// What the call is about, from the field of its input that names it: the
/// command, the path and line, the pattern and where it is looked for, the
/// URL. The title where the kind names nothing, and never the raw JSON.
/// Locations the head does not already name follow it.
fn subject(tool: &Tool) -> String {
    let input = &tool.input;
    let named =
        match tool.kind.as_deref() {
            Some("execute") => {
                command(input).map(|command| command.lines().next().unwrap_or_default().to_string())
            }
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

/// The command an `execute` call runs: one string, or the argument list
/// some agents send it as.
pub(super) fn command(input: &serde_json::Value) -> Option<String> {
    if let serde_json::Value::String(text) = input {
        return Some(text.clone());
    }
    match input.get("command")? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(words) => Some(
            words
                .iter()
                .filter_map(serde_json::Value::as_str)
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
        .and_then(serde_json::Value::as_u64)
        .or_else(|| first.filter(|location| location.path == path)?.line);
    Some(match line {
        Some(line) => format!("{path}:{line}"),
        None => path,
    })
}

/// The first of `keys` the input carries as a string.
fn field(input: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| input.get(key)?.as_str().map(str::to_string))
}

/// Cut a line to `room` columns, saying so. The cut falls between grapheme
/// clusters, each as wide as it draws, so an emoji of several characters is
/// kept or dropped whole.
fn clip(text: &str, room: usize) -> String {
    if text.width() <= room {
        return text.to_string();
    }
    let mut cut = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let width = grapheme.width();
        if used + width > room.saturating_sub(1) {
            break;
        }
        used += width;
        cut.push_str(grapheme);
    }
    cut.push('…');
    cut
}

/// How long a call took, in the unit that fits it.
fn elapsed(started_at: &str, ended_at: &str) -> Option<String> {
    let started = chrono::DateTime::parse_from_rfc3339(started_at).ok()?;
    let ended = chrono::DateTime::parse_from_rfc3339(ended_at).ok()?;
    let millis = (ended - started).num_milliseconds().max(0);
    Some(if millis < 1_000 {
        format!("{millis}ms")
    } else if millis < 60_000 {
        format!("{:.1}s", millis as f64 / 1_000.0)
    } else {
        format!("{}m {:02}s", millis / 60_000, millis % 60_000 / 1_000)
    })
}

/// A tool's output: indented, and folded to the last few lines, which is
/// where a command says how it went. Trailing blank lines are not output.
fn folded(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let all = wrap(text.trim_end(), width.saturating_sub(4));
    let hidden = all.len().saturating_sub(FOLD);
    let mut lines = Vec::new();
    if hidden > 0 {
        lines.push(Line::from(Span::styled(
            format!("    … {hidden} more lines"),
            DIM,
        )));
    }
    for line in all.into_iter().skip(hidden) {
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(line, style),
        ]));
    }
    lines
}

/// Text read from the top — a command, a diff — indented and folded to its
/// first `fold` lines, with a count of what was left out.
pub(super) fn folded_top(lines: Vec<Line<'static>>, fold: usize) -> Vec<Line<'static>> {
    let hidden = lines.len().saturating_sub(fold);
    if hidden == 0 {
        return lines;
    }
    let mut kept: Vec<_> = lines.into_iter().take(fold).collect();
    kept.push(Line::from(Span::styled(
        format!("    … {hidden} more lines"),
        DIM,
    )));
    kept
}

/// A unified diff: the file it changes as a header — the old name to the new
/// where they differ — then each line coloured for what it is, folded past
/// `fold` lines.
pub(super) fn folded_diff(diff: &str, width: usize, fold: usize) -> Vec<Line<'static>> {
    let indent =
        |text: String, style: Style| Line::from(vec![Span::raw("    "), Span::styled(text, style)]);
    let room = width.saturating_sub(4);
    let mut lines = Vec::new();
    let (mut old, mut new) = (None, None);
    let mut in_hunk = false;
    for line in diff.trim_end().lines() {
        if !in_hunk {
            if let Some(path) = line.strip_prefix("--- ") {
                old = file_name(path);
                continue;
            }
            if let Some(path) = line.strip_prefix("+++ ") {
                new = file_name(path);
                continue;
            }
            if !line.starts_with("@@") {
                // What git writes above a hunk — `diff --git`, `index`, a
                // mode — names nothing the header does not.
                continue;
            }
            in_hunk = true;
            let header = match (&old, &new) {
                (Some(old), Some(new)) if old != new => Some(format!("{old} → {new}")),
                (_, Some(name)) | (Some(name), None) => Some(name.clone()),
                (None, None) => None,
            };
            if let Some(header) = header {
                lines.push(indent(clip(&header, room), FILE));
            }
        }
        let style = match line.chars().next() {
            Some('+') => ADDED,
            Some('-') => REMOVED,
            Some('@') => HUNK,
            _ => DIM,
        };
        lines.push(indent(clip(line, room), style));
    }
    folded_top(lines, fold)
}

/// The path a diff's file header names, bare of git's `a/` and `b/`, and
/// nothing for the `/dev/null` of a file that did not exist.
fn file_name(header: &str) -> Option<String> {
    let path = header.split('\t').next().unwrap_or(header).trim();
    if path == "/dev/null" || path.is_empty() {
        return None;
    }
    Some(
        path.strip_prefix("a/")
            .or_else(|| path.strip_prefix("b/"))
            .unwrap_or(path)
            .to_string(),
    )
}

/// Hard-wrap text to `width` columns, keeping the line breaks it already has.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for source in text.split('\n') {
        let source = detab(source);
        let mut line = String::new();
        let mut used = 0usize;
        for word in source.split_inclusive(char::is_whitespace) {
            let length = word.width();
            if used > 0 && used + length > width {
                out.push(std::mem::take(&mut line).trim_end().to_string());
                used = 0;
            }
            used += length;
            line.push_str(word);
        }
        out.push(line.trim_end().to_string());
    }
    out
}

/// A line with its tabs as the columns they take, to the next stop of eight.
/// A cell holds one character, so a tab drawn as itself is nothing at all,
/// and the text on either side of it runs together.
fn detab(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::new();
    let mut column = 0;
    for grapheme in line.graphemes(true) {
        if grapheme == "\t" {
            let stop = 8 - column % 8;
            out.extend(std::iter::repeat_n(' ', stop));
            column += stop;
        } else {
            out.push_str(grapheme);
            column += grapheme.width();
        }
    }
    out
}

#[cfg(test)]
mod tests {

    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::{TerminalOptions, Viewport};
    use serde_json::json;

    use crate::tui::testing::*;
    use crate::tui::viewport::VIEWPORT;
    use crate::tui::*;

    use super::*;

    /// One tool call as the daemon records it (021): the ACP call under
    /// `acp`, with its kind and its raw input.
    fn called(id: &str, kind: &str, status: &str, input: serde_json::Value) -> AgentEventDto {
        event(
            "post_tool_use",
            id,
            json!({
                "tool_name": kind,
                "tool_input": input,
                "acp": {"toolCallId": id, "title": kind, "kind": kind, "status": status,
                        "rawInput": input}
            }),
        )
    }

    /// The style of the first cell of the first line that, past its indent,
    /// starts with `text`.
    fn style_of(buffer: &Buffer, text: &str) -> Style {
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if row.trim_start().starts_with(text) {
                let x = u16::try_from(row.len() - row.trim_start().len()).unwrap();
                return buffer[(x, y)].style();
            }
        }
        panic!("{text:?} is not on the screen:\n{}", rows(buffer));
    }

    #[tokio::test]
    async fn each_kind_of_call_draws_its_glyph_and_what_it_is_about() {
        let (shown, _, _) = console(
            Stub::new(vec![
                called(
                    "run",
                    "execute",
                    "completed",
                    json!({"command": "cargo nextest run"}),
                ),
                called(
                    "read",
                    "read",
                    "completed",
                    json!({"file_path": "src/main.rs", "line": 12}),
                ),
                called(
                    "grep",
                    "search",
                    "completed",
                    json!({"pattern": "fn main", "path": "src"}),
                ),
                called(
                    "get",
                    "fetch",
                    "failed",
                    json!({"url": "https://example.com/spec"}),
                ),
            ])
            .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        assert!(shown.contains("✓ $ cargo nextest run"), "{shown}");
        assert!(shown.contains("✓ ≡ src/main.rs:12"), "{shown}");
        assert!(shown.contains("✓ ⌕ fn main in src"), "{shown}");
        assert!(shown.contains("✗ ↓ https://example.com/spec"), "{shown}");
        assert!(
            !shown.contains('{'),
            "no raw JSON where a field names the subject: {shown}"
        );
    }

    #[tokio::test]
    async fn a_completed_call_draws_its_duration() {
        let (shown, _, _) = console(
            Stub::new(vec![
                event_at(
                    "pre_tool_use",
                    "test",
                    json!({"acp": {"toolCallId": "test", "kind": "execute", "status": "pending",
                                   "rawInput": {"command": "cargo test"}}}),
                    "2026-09-12T10:00:00.000Z",
                ),
                event_at(
                    "post_tool_use",
                    "test",
                    json!({"acp": {"toolCallId": "test", "kind": "execute", "status": "completed",
                                   "rawInput": {"command": "cargo test"}, "rawOutput": "ok"}}),
                    "2026-09-12T10:00:01.500Z",
                ),
            ])
            .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        assert!(shown.contains("✓ $ cargo test  1.5s"), "{shown}");
    }

    #[test]
    fn a_diff_draws_its_file_header_its_lines_coloured_and_a_fold_count() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let body: String = (1..=30).map(|n| format!("+line {n}\n")).collect();
        let patch = format!("--- /dev/null\n+++ b/src/new.rs\n@@ -0,0 +1,30 @@\n{body}");
        console.apply(&event(
            "post_tool_use",
            "write",
            json!({"acp": {"toolCallId": "write", "kind": "edit", "status": "completed",
                           "rawInput": {"file_path": "src/new.rs"},
                           "content": [{"type": "diff", "patch": {"text": patch}}]}}),
        ));
        console.apply(&event("agent_message", "done", json!({"text": "Written."})));

        console.commit(&mut terminal).unwrap();
        let shown = screen(&terminal);
        let buffer = terminal.backend().buffer();

        assert!(shown.contains("✓ ✎ src/new.rs"), "{shown}");
        assert!(
            row_of(&shown, "    src/new.rs") < row_of(&shown, "@@ -0,0 +1,30 @@"),
            "the file header is above the hunk: {shown}"
        );
        assert_eq!(style_of(buffer, "src/new.rs").add_modifier, Modifier::BOLD);
        assert_eq!(style_of(buffer, "@@ -0,0").fg, Some(Color::Cyan));
        assert_eq!(style_of(buffer, "+line 1").fg, Some(Color::Green));
        // The header, the hunk line and thirty added lines.
        assert!(
            shown.contains(&format!("… {} more lines", 32 - DIFF_FOLD)),
            "the tail past the limit is counted: {shown}"
        );
        assert!(!shown.contains("+line 30"), "{shown}");
    }

    /// A tool's output with tabs in it — a numbered file read, for one — is
    /// drawn with the columns the tabs take, not with the text on either
    /// side of them run together.
    #[test]
    fn a_tab_in_a_tool_output_takes_the_columns_to_the_next_tab_stop() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&event(
            "post_tool_use",
            "Read notes.txt",
            json!({"tool_name": "Read",
                   "acp": {"toolCallId": "read", "kind": "read", "status": "completed",
                           "rawInput": {"file_path": "notes.txt"},
                           "rawOutput": "1\tline 1\n2\tline 2\n"}}),
        ));

        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);

        assert!(
            shown.contains("    1       line 1\n    2       line 2"),
            "{shown}"
        );
    }

    /// Four words of three CJK characters, each six columns wide, on a
    /// terminal sixteen columns wide: two words fill a line of the block,
    /// where a count of characters would fit three.
    #[test]
    fn a_line_of_wide_characters_wraps_at_the_display_width() {
        let mut console = Console::new(header());
        let mut terminal = Terminal::with_options(
            TestBackend::new(16, 40),
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT),
            },
        )
        .unwrap();
        console.apply(&event(
            "user_prompt_submit",
            "wide",
            json!({"text": "日本語 日本語 日本語 日本語", "source": "console"}),
        ));

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert!(
            shown.contains("> 日本語 日本語\n  日本語 日本語"),
            "{shown}"
        );
    }

    /// A woman scientist — three characters joined into one two-column
    /// emoji — heads a command too long for a pane sixteen columns wide.
    /// The head has twelve columns for it: eleven of text and the mark. A
    /// cut that counted characters would take two columns too many for the
    /// emoji and stop two characters short.
    #[test]
    fn a_head_is_cut_between_whole_emoji_sequences() {
        let mut console = Console::new(header());
        let mut terminal = Terminal::with_options(
            TestBackend::new(16, 40),
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT),
            },
        )
        .unwrap();
        console.apply(&called(
            "run",
            "execute",
            "completed",
            json!({"command": "\u{1f469}\u{200d}\u{1f52c} lab notes now"}),
        ));

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = screen(&terminal);
        assert!(
            shown.contains("✓ $ \u{1f469}\u{200d}\u{1f52c} lab note…"),
            "{shown}"
        );
    }

    #[tokio::test]
    async fn a_daemon_sourced_prompt_draws_under_its_own_marker() {
        let (shown, _, _) = console(
            Stub::new(vec![
                event(
                    "user_prompt_submit",
                    "Land the change.",
                    json!({"text": "Land the change.", "source": "daemon"}),
                ),
                event(
                    "user_prompt_submit",
                    "Go ahead.",
                    json!({"text": "Go ahead.", "source": "console"}),
                ),
            ])
            .deltas(vec![ended()]),
            Vec::new(),
        )
        .await;

        assert!(
            shown.contains("» daemon\n  Land the change."),
            "the briefing is labelled and drawn under its own marker: {shown}"
        );
        assert!(
            !shown.contains("> Land the change."),
            "and not as typed input: {shown}"
        );
        assert!(shown.contains("> Go ahead."), "{shown}");
    }
}
