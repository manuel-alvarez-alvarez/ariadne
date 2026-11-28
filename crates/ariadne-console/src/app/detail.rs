//! The detail of an open tool call step, drawn in place under its row, by
//! the kind of the call: a command with its output, an edit as a coloured
//! diff, a read as the file in its language, a search and a fetch with what
//! they found. `r` draws the raw input and the raw output instead.
//!
//! An output keeps its last lines,
//! and a diff, a file and raw JSON keep their first ones, each with a count
//! of what was left out. A second Enter on the step draws it whole.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use ariadne_api::trajectory::{ContentBlockDto, ToolCallDetailDto, ToolDetailDto};

use crate::diff::{folded, folded_diff, folded_top};
use crate::markdown;
use crate::theme::{DIM, FAIL, FILE, OUTPUT, TOOL};
use crate::wrap::{self, wrap};

/// How many of its last lines a command's output keeps while folded.
const OUTPUT_FOLD: usize = 12;
/// How many of their first lines a diff, a file and raw JSON keep while
/// folded.
const TOP_FOLD: usize = 24;
/// The indent of a detail line, as the fold helpers draw it.
const INDENT: &str = "    ";

/// The lines of a call's detail at `width`, each behind a four-column
/// indent. `raw` draws the raw input and output, and `whole` every line.
pub(super) fn lines(
    detail: &ToolCallDetailDto,
    raw: bool,
    whole: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let top = if whole { usize::MAX } else { TOP_FOLD };
    let room = width.saturating_sub(INDENT.len()).max(1);
    if raw {
        return match &detail.raw {
            Some(raw) => {
                let mut lines = json("raw input", raw.input.as_ref(), room, top);
                lines.extend(json("raw output", raw.output.as_ref(), room, top));
                lines
            }
            None => vec![said("reading the raw input and output…", DIM)],
        };
    }
    match &detail.detail {
        ToolDetailDto::Command(command) => {
            let mut lines: Vec<Line<'static>> = wrap(&format!("$ {}", command.command), room)
                .into_iter()
                .map(|part| said(&part, TOOL))
                .collect();
            if let Some(cwd) = &command.cwd {
                lines.push(said(&wrap::clip(&format!("in {cwd}"), room), DIM));
            }
            if command.truncated {
                lines.push(said("… the start of the output was cut", DIM));
            }
            if let Some(output) = command
                .output
                .as_deref()
                .filter(|out| !out.trim().is_empty())
            {
                let fold = (!whole).then_some(OUTPUT_FOLD);
                lines.extend(folded(output, width, OUTPUT, fold));
            }
            match command.exit_code {
                Some(0) => lines.push(said("exit 0", DIM)),
                Some(code) => lines.push(said(&format!("exit {code}"), FAIL)),
                None => {}
            }
            lines
        }
        ToolDetailDto::Diff(diff) => {
            let mut lines = Vec::new();
            for file in &diff.files {
                match file.unified.trim().is_empty() {
                    true => lines.push(said(&wrap::clip(&file.path, room), FILE)),
                    false => lines.extend(folded_diff(&file.unified, width, top)),
                }
            }
            if lines.is_empty() {
                lines.push(said("no change", DIM));
            }
            lines
        }
        ToolDetailDto::File(file) => {
            let mut lines = Vec::new();
            if let Some(path) = &file.path {
                let place = match file.line {
                    Some(line) => format!("{path}:{line}"),
                    None => path.clone(),
                };
                lines.push(said(&wrap::clip(&place, room), FILE));
            }
            if let Some(text) = &file.text {
                let code = markdown::code(text, file.language.as_deref(), room)
                    .into_iter()
                    .map(indent)
                    .collect();
                lines.extend(folded_top(code, top));
            }
            lines
        }
        ToolDetailDto::Search(search) => {
            found(search.query.as_deref(), search.text.as_deref(), room, top)
        }
        ToolDetailDto::Fetch(fetch) => {
            found(fetch.url.as_deref(), fetch.text.as_deref(), room, top)
        }
        ToolDetailDto::Generic(generic) => {
            let mut lines = Vec::new();
            for block in &generic.content {
                match media(block) {
                    Some(line) => lines.push(said(&wrap::clip(&line, room), DIM)),
                    None => {
                        let text = match block {
                            ContentBlockDto::Text { text } => text.as_str(),
                            ContentBlockDto::Resource {
                                text: Some(text), ..
                            } => text.as_str(),
                            _ => "",
                        };
                        lines.extend(
                            markdown::plain(text.trim_end(), room, OUTPUT)
                                .into_iter()
                                .map(indent),
                        );
                    }
                }
            }
            folded_top(lines, top)
        }
    }
}

/// A content block that is not text, as one line: its type and its name.
/// `None` for a text block, and for a resource that carries its text.
pub(super) fn media(block: &ContentBlockDto) -> Option<String> {
    let (kind, name) = match block {
        ContentBlockDto::Text { .. } | ContentBlockDto::Resource { text: Some(_), .. } => {
            return None;
        }
        ContentBlockDto::Image { mime_type, uri, .. } => (
            "image",
            uri.as_deref().map(last_segment).or(mime_type.as_deref()),
        ),
        ContentBlockDto::Audio { mime_type, .. } => ("audio", mime_type.as_deref()),
        ContentBlockDto::Resource { uri, .. } => ("resource", Some(last_segment(uri))),
        ContentBlockDto::ResourceLink { name, title, .. } => {
            ("resource link", Some(title.as_deref().unwrap_or(name)))
        }
    };
    Some(format!(
        "[{kind}] {}",
        name.filter(|name| !name.is_empty()).unwrap_or("unnamed")
    ))
}

/// The last segment of a path or a URI: the name of what it points at.
fn last_segment(uri: &str) -> &str {
    let trimmed = uri.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// A search or a fetch: what it looked for, then what it found.
fn found(subject: Option<&str>, text: Option<&str>, room: usize, top: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(subject) = subject {
        lines.push(said(&wrap::clip(subject, room), TOOL));
    }
    if let Some(text) = text {
        let body = markdown::plain(text.trim_end(), room, OUTPUT)
            .into_iter()
            .map(indent)
            .collect();
        lines.extend(folded_top(body, top));
    }
    lines
}

/// Raw JSON under its label, pretty, or `none` where the agent sent none.
fn json(
    label: &str,
    value: Option<&serde_json::Value>,
    room: usize,
    top: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![said(label, FILE)];
    match value {
        Some(value) => {
            let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
            let body = pretty
                .lines()
                .flat_map(|line| wrap(line, room))
                .map(|part| said(&part, OUTPUT))
                .collect();
            lines.extend(folded_top(body, top));
        }
        None => lines.push(said("none", DIM)),
    }
    lines
}

/// One line of text behind the detail indent.
fn said(text: &str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::raw(INDENT),
        Span::styled(text.to_string(), style),
    ])
}

fn indent(line: Line<'static>) -> Line<'static> {
    let mut spans = vec![Span::raw(INDENT)];
    spans.extend(line.spans);
    Line::from(spans).style(line.style)
}
