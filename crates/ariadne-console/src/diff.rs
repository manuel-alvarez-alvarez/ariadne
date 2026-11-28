//! A tool call's file change as the lines it is drawn on: the file it
//! changes as a header, then each line coloured for what it is, folded past
//! a page with a count of what was left out.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::{ADDED, DIM, FILE, HUNK, REMOVED};
use crate::wrap::{self, clip, wrap};

/// A tool's output: indented, and folded to the last few lines, which is
/// where a command says how it went, unless `fold` is `None`. Trailing blank
/// lines are not output.
pub(crate) fn folded(
    text: &str,
    width: usize,
    style: Style,
    fold: Option<usize>,
) -> Vec<Line<'static>> {
    let all = wrap(text.trim_end(), width.saturating_sub(4));
    let hidden = fold.map_or(0, |fold| all.len().saturating_sub(fold));
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
pub(crate) fn folded_top(lines: Vec<Line<'static>>, fold: usize) -> Vec<Line<'static>> {
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
/// `fold` lines. Inside a hunk, a removed line pairs with the added line
/// that replaces it — the i-th of each, where a run of one is as long as the
/// run of the other — and the words that differ between the two are bold.
pub(crate) fn folded_diff(diff: &str, width: usize, fold: usize) -> Vec<Line<'static>> {
    let room = width.saturating_sub(4);
    let mut lines = Vec::new();
    let (mut old, mut new) = (None, None);
    let mut in_hunk = false;
    let mut rows: Vec<(char, String)> = Vec::new();
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
                lines.push(diff_row(clip(&header, room), FILE));
            }
        }
        let kind = match line.chars().next() {
            Some('+') => '+',
            Some('-') => '-',
            Some('@') => '@',
            _ => ' ',
        };
        rows.push((kind, line.to_string()));
    }
    lines.extend(diff_body(&rows, room));
    folded_top(lines, fold)
}

/// The lines of a diff's body: a run of removed lines paired, i-th to i-th,
/// with the run of added lines directly after it where the two runs are the
/// same length; every other line — an unequal pair, an unpaired run, a hunk
/// or context line — plain, as it always drew.
fn diff_body(rows: &[(char, String)], room: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        if rows[i].0 != '-' {
            let (kind, text) = &rows[i];
            out.push(plain_row(*kind, text, room));
            i += 1;
            continue;
        }
        let removed_start = i;
        while i < rows.len() && rows[i].0 == '-' {
            i += 1;
        }
        let added_start = i;
        while i < rows.len() && rows[i].0 == '+' {
            i += 1;
        }
        let removed = &rows[removed_start..added_start];
        let added = &rows[added_start..i];
        if added.is_empty() || removed.len() != added.len() {
            for (kind, text) in removed.iter().chain(added.iter()) {
                out.push(plain_row(*kind, text, room));
            }
            continue;
        }
        // A unified diff lists a run's removed lines whole, then its added
        // lines whole — never interleaved — so the pair is drawn in the
        // words it marks, never in the order it is read.
        let paired: Vec<(Line<'static>, Line<'static>)> = removed
            .iter()
            .zip(added.iter())
            .map(|((_, removed), (_, added))| paired_diff_lines(removed, added, room))
            .collect();
        out.extend(paired.iter().map(|(removed, _)| removed.clone()));
        out.extend(paired.into_iter().map(|(_, added)| added));
    }
    out
}

/// One line, coloured for its kind and clipped to `room`, with no bold: how
/// every diff line drew before a pair of it was considered for word marks,
/// and how one still draws where the pairing does not apply.
fn plain_row(kind: char, text: &str, room: usize) -> Line<'static> {
    let style = match kind {
        '+' => ADDED,
        '-' => REMOVED,
        '@' => HUNK,
        _ => DIM,
    };
    diff_row(clip(text, room), style)
}

/// A removed line and the added line replacing it, each in its own colour
/// with the words that differ between them bold, where the two share at
/// least half their words. Plain, as `plain_row` draws them, where they do
/// not: a rewritten line must not become a sea of bold.
///
/// "Share" is counted over word tokens alone: `similar`'s own `ratio()`
/// counts the whitespace between words as matching too, so two same-length,
/// mostly-unrelated lines with the same spacing pattern would pass a gate
/// built on it.
fn paired_diff_lines(removed: &str, added: &str, room: usize) -> (Line<'static>, Line<'static>) {
    let old_body = removed.get(1..).unwrap_or_default();
    let new_body = added.get(1..).unwrap_or_default();
    let word_diff = similar::TextDiff::from_words(old_body, new_body);
    let mut old_words: Vec<(bool, String)> = Vec::new();
    let mut new_words: Vec<(bool, String)> = Vec::new();
    let (mut old_count, mut new_count, mut shared) = (0usize, 0usize, 0usize);
    for change in word_diff.iter_all_changes() {
        let word = change.as_str().unwrap_or_default();
        let is_word = !word.trim().is_empty();
        match change.tag() {
            similar::ChangeTag::Equal => {
                push_word(&mut old_words, false, word);
                push_word(&mut new_words, false, word);
                if is_word {
                    old_count += 1;
                    new_count += 1;
                    shared += 1;
                }
            }
            similar::ChangeTag::Delete => {
                push_word(&mut old_words, true, word);
                old_count += usize::from(is_word);
            }
            similar::ChangeTag::Insert => {
                push_word(&mut new_words, true, word);
                new_count += usize::from(is_word);
            }
        }
    }
    // `shared` words counted once, matched on each side: the shared fraction
    // is `2 * shared / (old_count + new_count)`, compared to one half without
    // the division.
    if old_count + new_count == 0 || 4 * shared < old_count + new_count {
        return (plain_row('-', removed, room), plain_row('+', added, room));
    }
    (
        diff_spans('-', REMOVED, old_words, room),
        diff_spans('+', ADDED, new_words, room),
    )
}

/// Append `word` to the run of the same emphasis at the end of `words`, or
/// start a new run — so that a stretch of unchanged or of changed words
/// becomes one span, not one per word.
fn push_word(words: &mut Vec<(bool, String)>, changed: bool, word: &str) {
    match words.last_mut() {
        Some((last_changed, run)) if *last_changed == changed => run.push_str(word),
        _ => words.push((changed, word.to_string())),
    }
}

/// A diff line as spans: the leading `+`/`-` in `style`, then each run of
/// `words` in `style`, bold where the run is a word that changed, clipped to
/// `room` columns total.
fn diff_spans(sign: char, style: Style, words: Vec<(bool, String)>, room: usize) -> Line<'static> {
    let mut parts: Vec<(String, Style)> = vec![(sign.to_string(), style)];
    parts.extend(words.into_iter().map(|(changed, text)| {
        let style = if changed {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        };
        (text, style)
    }));
    clipped_spans(parts, room)
}

/// One line behind the diff's four-column indent, in one style.
fn diff_row(text: String, style: Style) -> Line<'static> {
    Line::from(vec![Span::raw("    "), Span::styled(text, style)])
}

/// `parts` behind the diff's four-column indent, cut to `room` columns kept
/// across all of them together — as [`clip`] cuts one string — so a styled
/// run never overruns the room a plain line would have.
fn clipped_spans(parts: Vec<(String, Style)>, room: usize) -> Line<'static> {
    let mut spans = vec![Span::raw("    ")];
    let (text, ellipsis_style) = wrap::clip_spans(&parts, room);
    let mut offset = 0;
    for (part, style) in &parts {
        let end = offset + part.len();
        if let Some(slice) = text.get(offset.min(text.len())..end.min(text.len()))
            && !slice.is_empty()
        {
            spans.push(Span::styled(slice.to_string(), *style));
        }
        offset = end;
    }
    if let Some(style) = ellipsis_style {
        spans.push(Span::styled("…", style));
    }
    Line::from(spans)
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
