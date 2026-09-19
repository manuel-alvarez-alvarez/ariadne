//! The blocks of the transcript as the lines they are drawn on: a prompt, the
//! agent's text, a thought, a plan, a tool call with its output and diff.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::markdown;
use crate::theme::{
    self, ADDED, AGENT, DAEMON, DIM, FAIL, FILE, HUNK, PLAN, PROMPT, REMOVED, TOOL, USER,
};
use crate::transcript::{PlanEntry, Tool, TranscriptItem};

use super::picker::permission;

/// A thought and a tool's output are context, not the answer: they are folded
/// to this many lines with a count of what was left out. The output keeps its
/// last lines, which is where a command says how it went.
const FOLD: usize = 4;
/// A diff is read from the top, so it keeps its first lines: this many, with
/// a count of what was left out.
const DIFF_FOLD: usize = 24;
/// A prompt the daemon sent — a briefing can run to a hundred lines — keeps
/// its first lines: this many, with a count of what was left out.
const DAEMON_FOLD: usize = 6;
/// The indent of a tool's output and diff: two columns under the head's
/// mark, and the two of the output marker.
const BODY_INDENT: &str = "    ";

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
        TranscriptItem::UserPrompt { text, .. } => typed_prompt(text, width, false),
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
        TranscriptItem::Plan { entries, .. } => plan(entries, width),
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
        TranscriptItem::Error { text, .. } => marked(
            wrap_whole(text, width.saturating_sub(theme::ERROR_MARKER.width())),
            theme::ERROR_MARKER,
            FAIL,
            FAIL,
        ),
        // An event the pane has no block for says what it is and what it
        // was about, never its bare kind.
        TranscriptItem::Raw { kind, summary, .. } => {
            let text = match summary.is_empty() {
                true => kind.clone(),
                false => format!("{kind}{}{summary}", theme::SEPARATOR),
            };
            prefixed(&text, "  ", DIM, DIM, width, None)
        }
    }
}

/// A typed prompt that the daemon has not taken yet: the prompt, with a dim
/// tag that says it waits.
pub(super) fn queued(item: &TranscriptItem, width: usize) -> Vec<Line<'static>> {
    match item {
        TranscriptItem::UserPrompt { text, source, .. } if source.as_deref() != Some("daemon") => {
            typed_prompt(text, width.max(8), true)
        }
        _ => block(item, width, None),
    }
}

/// A prompt typed into the console: the input box's glyph on its first row
/// and a bar in the user's colour down the left edge of every row, so it
/// stands out from the text around it on a dark and a light theme alike.
fn typed_prompt(text: &str, width: usize, queued: bool) -> Vec<Line<'static>> {
    let marker = theme::USER_MARKER.width();
    let bar = || Span::styled(theme::USER_BAR, USER);
    let room = width.saturating_sub(theme::USER_BAR.width() + marker);
    let mut lines: Vec<Line<'static>> = wrap(text, room)
        .into_iter()
        .enumerate()
        .map(|(at, row)| {
            let lead = match at {
                0 => Span::styled(theme::USER_MARKER, USER),
                _ => Span::raw(" ".repeat(marker)),
            };
            Line::from(vec![bar(), lead, Span::styled(row, PROMPT)])
        })
        .collect();
    if queued {
        let tag = format!("  {}", theme::QUEUED);
        match lines.last_mut() {
            Some(last) if last.width() + tag.width() <= width => {
                last.spans.push(Span::styled(tag, DIM));
            }
            _ => lines.push(Line::from(vec![
                bar(),
                Span::raw(" ".repeat(marker)),
                Span::styled(theme::QUEUED, DIM),
            ])),
        }
    }
    lines
}

/// A prompt the daemon itself sent — a briefing, a nudge, a message — under
/// its own marker and label, so it reads apart from what was typed, and
/// folded to its first lines with a count of the rest.
fn daemon_prompt(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut rows = wrap(text, width.saturating_sub(2));
    let hidden = rows.len().saturating_sub(DAEMON_FOLD);
    rows.truncate(DAEMON_FOLD);
    let mut lines = vec![Line::from(Span::styled(theme::DAEMON_MARKER, DAEMON))];
    for line in rows {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(line, DAEMON),
        ]));
    }
    if hidden > 0 {
        lines.push(Line::from(Span::styled(
            format!("  … {hidden} more lines"),
            DIM,
        )));
    }
    lines
}

/// A plan as a checklist under a head that counts the entries completed: a
/// mark per status, and a long entry wrapped under its own text.
fn plan(entries: &[PlanEntry], width: usize) -> Vec<Line<'static>> {
    let done = entries
        .iter()
        .filter(|entry| entry.status == "completed")
        .count();
    let mut lines = vec![Line::from(Span::styled(
        format!("plan {done}/{}", entries.len()),
        PLAN,
    ))];
    for entry in entries {
        let (mark, mark_style, style) = match entry.status.as_str() {
            "completed" => (theme::PLAN_DONE, DIM, DIM),
            "in_progress" => (theme::PLAN_DOING, PLAN, Style::new()),
            _ => (theme::PLAN_TODO, Style::new(), Style::new()),
        };
        let indent = mark.width();
        for (at, row) in wrap(&entry.content, width.saturating_sub(indent))
            .into_iter()
            .enumerate()
        {
            let lead = match at {
                0 => Span::styled(mark, mark_style),
                _ => Span::raw(" ".repeat(indent)),
            };
            lines.push(Line::from(vec![lead, Span::styled(row, style)]));
        }
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
    marked(wrapped, marker, marker_style, style)
}

/// Rows of text behind a marker on the first and its width of blank on the
/// rest.
fn marked(
    rows: Vec<String>,
    marker: &str,
    marker_style: Style,
    style: Style,
) -> Vec<Line<'static>> {
    rows.into_iter()
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
/// lines and the file change as a diff, both hung from the head by the
/// output marker.
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
    let mut body = Vec::new();
    if let Some(output) = &tool.output {
        body.extend(folded(output, width, DIM));
    }
    if let Some(diff) = &tool.diff {
        body.extend(folded_diff(diff, width, DIFF_FOLD));
    }
    let mut lines = vec![head(tool, Span::styled(glyph, style), elapsed, width)];
    lines.extend(hung(body));
    lines
}

/// A call's body with its first row started by the output marker in place
/// of the indent, so the eye ties the output to the head above it.
fn hung(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if let Some(first) = lines.first_mut()
        && let Some(lead) = first.spans.first()
        && let Some(rest) = lead.content.strip_prefix(BODY_INDENT)
    {
        let rest = Span::styled(rest.to_string(), lead.style);
        first
            .spans
            .splice(0..1, [Span::styled(theme::OUTPUT_MARKER, DIM), rest]);
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
/// A word wider than a row is cut between grapheme clusters across rows, so
/// no character of it is lost past the edge. The space after a word takes no
/// room at the end of a row, where it is not drawn.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for source in text.split('\n') {
        let source = detab(source);
        let mut line = String::new();
        let mut used = 0usize;
        for word in source.split_inclusive(char::is_whitespace) {
            let body = word.trim_end();
            if body.width() <= width && used > 0 && used + body.width() > width {
                out.push(std::mem::take(&mut line).trim_end().to_string());
                used = 0;
            }
            if body.width() > width {
                for grapheme in body.graphemes(true) {
                    let columns = grapheme.width();
                    if used > 0 && used + columns > width {
                        out.push(std::mem::take(&mut line).trim_end().to_string());
                        used = 0;
                    }
                    used += columns;
                    line.push_str(grapheme);
                }
                let space = &word[body.len()..];
                if used + space.width() <= width {
                    used += space.width();
                    line.push_str(space);
                }
                continue;
            }
            used += word.width();
            line.push_str(word);
        }
        out.push(line.trim_end().to_string());
    }
    out
}

/// Wrap text to `width` columns and keep every character of it, spaces
/// included: a word goes to the next row with the space after it where the
/// two do not fit, and a word wider than a row starts where the row is and
/// is cut between grapheme clusters. The rows of one line, joined, are that
/// line.
fn wrap_whole(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for source in text.split('\n') {
        let source = detab(source);
        let mut line = String::new();
        let mut used = 0usize;
        for word in source.split_inclusive(char::is_whitespace) {
            if word.width() <= width && used > 0 && used + word.width() > width {
                out.push(std::mem::take(&mut line));
                used = 0;
            }
            for grapheme in word.graphemes(true) {
                let columns = grapheme.width();
                if used > 0 && used + columns > width {
                    out.push(std::mem::take(&mut line));
                    used = 0;
                }
                used += columns;
                line.push_str(grapheme);
            }
        }
        out.push(line);
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

    /// The style of the first cell of the first line that, past its indent
    /// and an output marker, starts with `text`.
    fn style_of(buffer: &Buffer, text: &str) -> Style {
        let marker = theme::OUTPUT_MARKER.trim_start();
        for y in 0..buffer.area.height {
            let cells: Vec<&str> = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            let mut x = cells.iter().take_while(|cell| **cell == " ").count();
            let row = cells[x..].concat();
            let row = match row.strip_prefix(marker) {
                Some(rest) => {
                    x += marker.width();
                    rest.to_string()
                }
                None => row,
            };
            if row.starts_with(text) {
                return buffer[(u16::try_from(x).unwrap(), y)].style();
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
        assert!(shown.contains("✗ ⇣ https://example.com/spec"), "{shown}");
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
            row_of(&shown, "  ⎿ src/new.rs") < row_of(&shown, "@@ -0,0 +1,30 @@"),
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
            shown.contains("  ⎿ 1       line 1\n    2       line 2"),
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
            shown.contains("▌❯ 日本語 日本語\n▌  日本語 日本語"),
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
            !shown.contains("❯ Land the change."),
            "and not as typed input: {shown}"
        );
        assert!(shown.contains("❯ Go ahead."), "{shown}");
    }

    /// A terminal `width` columns wide, the pane inline in it.
    fn wide(width: u16) -> Terminal<TestBackend> {
        Terminal::with_options(
            TestBackend::new(width, 40),
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT),
            },
        )
        .unwrap()
    }

    /// The pane with these events applied and drawn, as the screen reads.
    fn drawn(events: &[AgentEventDto]) -> (String, Terminal<TestBackend>) {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        for event in events {
            console.apply(event);
        }
        terminal.draw(|frame| console.render(frame)).unwrap();
        (screen(&terminal), terminal)
    }

    #[test]
    fn a_typed_prompt_of_three_rows_draws_the_bar_on_each_row_and_the_marker_on_the_first() {
        let (shown, terminal) = drawn(&[event(
            "user_prompt_submit",
            "rows",
            json!({"text": "one\ntwo\nthree", "source": "console"}),
        )]);
        let buffer = terminal.backend().buffer();

        assert!(shown.contains("▌❯ one\n▌  two\n▌  three"), "{shown}");
        let bars: Vec<u16> = (0..buffer.area.height)
            .filter(|y| buffer[(0, *y)].symbol() == "▌")
            .collect();
        assert_eq!(bars.len(), 3, "{shown}");
        for y in bars {
            assert_eq!(
                buffer[(0, y)].fg,
                Color::Cyan,
                "the bar is the user's colour"
            );
        }
    }

    #[test]
    fn a_daemon_prompt_of_forty_lines_draws_six_and_a_count_of_the_rest() {
        let briefing: Vec<String> = (1..=40).map(|n| format!("line {n}")).collect();
        let (shown, _) = drawn(&[event(
            "user_prompt_submit",
            "briefing",
            json!({"text": briefing.join("\n"), "source": "daemon"}),
        )]);

        assert!(
            shown.contains("» daemon\n  line 1\n") && shown.contains("  line 6\n  … 34 more lines"),
            "{shown}"
        );
        assert!(!shown.contains("line 7"), "{shown}");
    }

    #[test]
    fn the_output_and_the_diff_of_a_call_hang_from_its_head_at_one_column() {
        let (shown, _) = drawn(&[event(
            "post_tool_use",
            "edit",
            json!({"acp": {"toolCallId": "edit", "kind": "edit", "status": "completed",
                           "rawInput": {"file_path": "a.txt"},
                           "rawOutput": "first\nsecond",
                           "content": [{"type": "diff",
                                        "patch": {"text": "--- a.txt\n+++ a.txt\n@@ -1 +1 @@\n-old\n+new\n"}}]}}),
        )]);

        assert!(
            shown.contains(
                "✓ ✎ a.txt\n  ⎿ first\n    second\n    a.txt\n    @@ -1 +1 @@\n    -old\n    +new"
            ),
            "{shown}"
        );
    }

    #[test]
    fn an_error_wider_than_the_pane_wraps_with_no_character_lost() {
        let text: String = "error: "
            .chars()
            .chain(('a'..='z').cycle().take(293))
            .collect();
        let mut console = Console::new(header());
        let mut terminal = wide(80);

        console.failed(&text);
        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);

        let rows: Vec<&str> = shown
            .lines()
            .skip_while(|row| !row.starts_with("✗ "))
            .take_while(|row| !row.is_empty())
            .collect();
        assert!(rows.len() > 1, "{shown}");
        assert!(rows.iter().all(|row| row.width() <= 80), "{shown}");
        let joined: String = rows
            .iter()
            .map(|row| row.strip_prefix("✗ ").or(row.strip_prefix("  ")).unwrap())
            .collect();
        assert_eq!(joined, text, "{shown}");
    }

    /// Words of uneven length, so rows break after a word and before one.
    /// The rows are read from the cells in the error's colour: a space
    /// that ends a row is one of them, and the blank after the text is not.
    #[test]
    fn a_multi_word_error_wider_than_the_pane_keeps_every_character() {
        let words: Vec<String> = (0..60).map(|n| format!("w{}", n * 37)).collect();
        let text = format!("error: {}", words.join(" "));
        assert!(text.width() >= 300);
        let mut console = Console::new(header());
        let mut terminal = wide(80);

        console.failed(&text);
        terminal.draw(|frame| console.render(frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let first = (0..buffer.area.height)
            .find(|y| buffer[(0, *y)].symbol() == "✗")
            .unwrap();
        let mut joined = String::new();
        for y in first..buffer.area.height {
            let row: String = (2..buffer.area.width)
                .take_while(|x| buffer[(*x, y)].fg == Color::Red)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if row.is_empty() {
                break;
            }
            joined.push_str(&row);
        }
        assert_eq!(joined, text, "{}", rows(buffer));
    }

    #[test]
    fn a_stop_at_the_end_of_a_turn_adds_no_line_and_a_cancelled_turn_says_so() {
        let said = event("agent_message", "done", json!({"text": "done"}));
        let stop = |reason: &str| event("stop", reason, json!({"stop_reason": reason}));

        // The transcript: what is above the status line.
        let transcript = |events: &[AgentEventDto]| {
            let (shown, _) = drawn(events);
            shown[..shown.find("author").unwrap()].to_string()
        };
        let before = transcript(std::slice::from_ref(&said));
        let after = transcript(&[said.clone(), stop("end_turn")]);
        let cancelled = transcript(&[said, stop("cancelled")]);

        assert_eq!(after, before, "an ended turn adds nothing");
        assert!(cancelled.contains("  turn cancelled"), "{cancelled}");
        assert!(!cancelled.contains("stopped"), "{cancelled}");
    }

    #[test]
    fn an_unknown_event_draws_its_kind_and_its_summary() {
        let (shown, terminal) = drawn(&[event("foo.bar", "baz", json!({}))]);

        assert!(shown.contains("foo.bar · baz"), "{shown}");
        assert!(
            style_of(terminal.backend().buffer(), "foo.bar")
                .add_modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn a_plan_draws_a_mark_per_status_and_counts_the_completed_in_its_head() {
        let (shown, terminal) = drawn(&[event(
            "plan",
            "plan",
            json!({"entries": [
                {"content": "Read the code", "status": "completed"},
                {"content": "Write the test", "status": "in_progress"},
                {"content": "Run the suite", "status": "pending"}
            ]}),
        )]);
        let buffer = terminal.backend().buffer();

        assert!(
            shown.contains("plan 1/3\n  ☑ Read the code\n  ◐ Write the test\n  ☐ Run the suite"),
            "{shown}"
        );
        assert!(style_of(buffer, "☑").add_modifier.contains(Modifier::DIM));
        assert_eq!(style_of(buffer, "◐").fg, Some(Color::Blue));
    }

    #[test]
    fn a_long_plan_entry_wraps_under_its_own_text() {
        let mut console = Console::new(header());
        let mut terminal = wide(20);
        console.apply(&event(
            "plan",
            "plan",
            json!({"entries": [{"content": "Write the test that proves it", "status": "pending"}]}),
        ));

        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);

        assert!(
            shown.contains("  ☐ Write the test\n    that proves it"),
            "{shown}"
        );
    }
}
