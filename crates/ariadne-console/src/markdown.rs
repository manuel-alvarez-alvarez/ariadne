//! Agent markdown as styled terminal lines.
//!
//! This renderer is a pure function of text and width. It measures terminal
//! columns rather than bytes and cuts only between grapheme clusters.
//!
//! A fenced block in a language that syntect has a grammar for is coloured by
//! the scopes of that grammar, mapped onto the pane's ANSI palette. No theme
//! is loaded, so the terminal's own theme decides every hue.

use std::sync::OnceLock;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::{
    CODE, CODE_COMMENT, CODE_CONTINUATION, CODE_KEYWORD, CODE_NUMBER, CODE_PLAIN, CODE_STRING,
    CODE_TYPE, HEADING, LIST_BULLETS, MARK, QUOTE_BAR, RULE, TABLE_HEADER, TASK_DONE, TASK_TODO,
};

/// Render `text` as markdown, wrapped to `width` columns.
pub fn render(text: &str, width: usize) -> Vec<Line<'static>> {
    Writer::new(width.max(1)).run(text)
}

#[derive(Default)]
struct Writer {
    width: usize,
    lines: Vec<Line<'static>>,
    pending: Vec<Span<'static>>,
    style: Style,
    quote: String,
    lists: Vec<Option<u64>>,
    items: Vec<Item>,
    code: bool,
    highlight: Option<Highlight>,
    table: Option<Table>,
    links: Vec<Link>,
}

struct Item {
    first: String,
    rest: String,
}

struct Link {
    destination: String,
    text: String,
}

struct Table {
    alignments: Vec<Alignment>,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: Option<String>,
    header_open: bool,
}

impl Writer {
    fn new(width: usize) -> Self {
        Self {
            width,
            ..Self::default()
        }
    }

    fn run(mut self, text: &str) -> Vec<Line<'static>> {
        let options =
            Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
        for event in Parser::new_ext(text, options) {
            self.event(event);
        }
        self.flush();
        if let Some(table) = self.table.take() {
            self.draw_table(table);
        }
        while self.lines.last().is_some_and(Line::is_blank) {
            self.lines.pop();
        }
        self.lines
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if self.in_table_cell(&text) {
                    return;
                }
                if let Some(link) = self.links.last_mut() {
                    link.text.push_str(&text);
                }
                if self.code {
                    for line in text.lines() {
                        self.verbatim(line);
                    }
                } else {
                    self.push(&text, self.style);
                }
            }
            Event::Code(code) => {
                // A code span keeps its backticks in a table cell as in text.
                if self.in_table_cell(&format!("`{code}`")) {
                    return;
                }
                if let Some(link) = self.links.last_mut() {
                    link.text.push_str(&code);
                }
                self.push(&format!("`{code}`"), CODE);
            }
            Event::SoftBreak | Event::HardBreak if self.in_table_cell(" ") => {}
            Event::SoftBreak => self.push(" ", self.style),
            Event::HardBreak => self.flush(),
            Event::Rule => self.rule(),
            Event::TaskListMarker(done) => {
                self.push(if done { TASK_DONE } else { TASK_TODO }, MARK)
            }
            Event::FootnoteReference(name) => self.push(&format!("[^{name}]"), MARK),
            Event::InlineMath(math) | Event::DisplayMath(math) => self.push(&math, CODE),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Table(alignments) => {
                self.blank();
                self.table = Some(Table {
                    alignments,
                    header: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                    cell: None,
                    header_open: false,
                });
            }
            Tag::TableHead => {
                self.table
                    .as_mut()
                    .expect("a table head has a table")
                    .header_open = true
            }
            Tag::TableRow => self
                .table
                .as_mut()
                .expect("a table row has a table")
                .row
                .clear(),
            Tag::TableCell => {
                self.table.as_mut().expect("a table cell has a table").cell = Some(String::new())
            }
            Tag::Heading { level, .. } => {
                self.blank();
                self.style = HEADING;
                self.push(&format!("{} ", "#".repeat(heading_depth(level))), MARK);
            }
            Tag::Paragraph => self.blank(),
            Tag::BlockQuote(_) => {
                self.blank();
                self.quote.push_str(QUOTE_BAR);
            }
            Tag::CodeBlock(kind) => {
                self.blank();
                if let CodeBlockKind::Fenced(language) = kind
                    && !language.is_empty()
                {
                    self.draw_wrapped(&language, MARK);
                    self.highlight = Highlight::new(&language);
                }
                self.code = true;
            }
            Tag::List(first) => {
                if self.lists.is_empty() {
                    self.blank();
                } else {
                    self.flush();
                }
                self.lists.push(first);
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}. ");
                        *number += 1;
                        marker
                    }
                    _ => LIST_BULLETS[(self.lists.len().saturating_sub(1)).min(2)].to_string(),
                };
                let base = format!(
                    "{}{}",
                    self.quote,
                    "  ".repeat(self.lists.len().saturating_sub(1))
                );
                self.items.push(Item {
                    first: format!("{base}{marker}"),
                    rest: format!("{base}{}", " ".repeat(marker.width())),
                });
            }
            Tag::Emphasis => self.style = self.style.add_modifier(Modifier::ITALIC),
            Tag::Strong => self.style = self.style.add_modifier(Modifier::BOLD),
            Tag::Strikethrough => self.style = self.style.add_modifier(Modifier::CROSSED_OUT),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => self.links.push(Link {
                destination: dest_url.to_string(),
                text: String::new(),
            }),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::TableCell => {
                let table = self.table.as_mut().expect("a table cell has a table");
                table.row.push(table.cell.take().unwrap_or_default());
            }
            TagEnd::TableHead => {
                let table = self.table.as_mut().expect("a table head has a table");
                table.header = std::mem::take(&mut table.row);
                table.header_open = false;
            }
            TagEnd::TableRow => {
                let table = self.table.as_mut().expect("a table row has a table");
                if !table.header_open {
                    table.rows.push(std::mem::take(&mut table.row));
                }
            }
            TagEnd::Table => {
                let table = self.table.take().expect("a table end has a table");
                self.draw_table(table);
            }
            TagEnd::Heading(_) | TagEnd::Paragraph => {
                self.style = Style::new();
                self.flush();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.remove_quote();
            }
            TagEnd::CodeBlock => {
                self.code = false;
                self.highlight = None;
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush();
                self.items.pop();
            }
            TagEnd::Emphasis => self.style = self.style.remove_modifier(Modifier::ITALIC),
            TagEnd::Strong => self.style = self.style.remove_modifier(Modifier::BOLD),
            TagEnd::Strikethrough => self.style = self.style.remove_modifier(Modifier::CROSSED_OUT),
            TagEnd::Link | TagEnd::Image => {
                if let Some(link) = self.links.pop()
                    && link.text != link.destination
                {
                    self.push(&format!(" ({})", link.destination), MARK);
                }
            }
            _ => {}
        }
    }

    fn in_table_cell(&mut self, text: &str) -> bool {
        let Some(table) = &mut self.table else {
            return false;
        };
        let Some(cell) = &mut table.cell else {
            return false;
        };
        cell.push_str(text);
        true
    }

    fn push(&mut self, text: &str, style: Style) {
        if !text.is_empty() {
            self.pending.push(Span::styled(text.to_string(), style));
        }
    }

    fn rule(&mut self) {
        self.flush();
        self.lines
            .push(Line::from(Span::styled(RULE.repeat(self.width), MARK)));
    }

    fn verbatim(&mut self, text: &str) {
        let base = self.block_prefix();
        let first = format!("{base}  ");
        let continued = format!("{base}{CODE_CONTINUATION}");
        let runs = match self
            .highlight
            .as_mut()
            .map(|highlight| highlight.line(text))
        {
            Some(Some(runs)) => runs,
            Some(None) => {
                // A grammar that fails on a line leaves the rest of the
                // block plain, rather than coloured from a broken state.
                self.highlight = None;
                vec![(0, CODE)]
            }
            None => vec![(0, CODE)],
        };
        let mut rest = text;
        let mut first_line = true;
        loop {
            let prefix = if first_line { &first } else { &continued };
            let start = text.len() - rest.len();
            let (part, next) = cut(rest, self.width.saturating_sub(prefix.width()).max(1));
            let mut spans = vec![Span::styled(prefix.clone(), MARK)];
            spans.extend(styled(text, start..start + part.len(), &runs));
            self.lines.push(Line::from(spans));
            let Some(next) = next else {
                break;
            };
            rest = next;
            first_line = false;
        }
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let (first, rest) = self.prefixes();
        let mut spans = Vec::new();
        let mut used = 0;
        let mut first_line = true;
        for span in std::mem::take(&mut self.pending) {
            for word in words(&span.content) {
                if word.trim().is_empty() {
                    let prefix = if first_line { &first } else { &rest };
                    if used > 0 && word.width() <= self.room(prefix, used) {
                        used += word.width();
                        spans.push(Span::styled(word, span.style));
                    }
                    continue;
                }
                let mut word = word.as_str();
                loop {
                    let prefix = if first_line { &first } else { &rest };
                    let room = self.room(prefix, used);
                    if used > 0 && word.width() > room {
                        trim_space(&mut spans);
                        self.emit(&mut spans, prefix);
                        used = 0;
                        first_line = false;
                        continue;
                    }
                    if word.width() <= room {
                        used += word.width();
                        spans.push(Span::styled(word.to_string(), span.style));
                        break;
                    }
                    let (part, next) = cut(word, room);
                    spans.push(Span::styled(part, span.style));
                    self.emit(&mut spans, prefix);
                    used = 0;
                    first_line = false;
                    word = next.expect("a full line leaves text");
                }
            }
        }
        trim_space(&mut spans);
        if !spans.is_empty() {
            self.emit(&mut spans, if first_line { &first } else { &rest });
        }
    }

    fn blank(&mut self) {
        self.flush();
        if !self.lines.is_empty() && !self.lines.last().is_some_and(Line::is_blank) {
            self.lines.push(Line::default());
        }
    }

    fn remove_quote(&mut self) {
        let mut end = self.quote.len();
        for _ in QUOTE_BAR.graphemes(true) {
            let start = self.quote[..end]
                .grapheme_indices(true)
                .next_back()
                .map(|(at, _)| at)
                .unwrap_or(0);
            end = start;
        }
        self.quote.truncate(end);
    }

    fn prefixes(&self) -> (String, String) {
        self.items
            .last()
            .map(|item| (item.first.clone(), item.rest.clone()))
            .unwrap_or_else(|| (self.quote.clone(), self.quote.clone()))
    }

    fn block_prefix(&self) -> String {
        self.items
            .last()
            .map(|item| item.rest.clone())
            .unwrap_or_else(|| self.quote.clone())
    }

    fn room(&self, prefix: &str, used: usize) -> usize {
        self.width
            .saturating_sub(prefix.width())
            .saturating_sub(used)
            .max(1)
    }

    fn emit(&mut self, spans: &mut Vec<Span<'static>>, prefix: &str) {
        let mut line = Vec::new();
        if !prefix.is_empty() {
            line.push(Span::styled(prefix.to_string(), MARK));
        }
        line.append(spans);
        self.lines.push(Line::from(line));
    }

    fn line(&mut self, prefix: &str, text: &str, style: Style) {
        let mut spans = Vec::new();
        if !prefix.is_empty() {
            spans.push(Span::styled(prefix.to_string(), MARK));
        }
        spans.push(Span::styled(text.to_string(), style));
        self.lines.push(Line::from(spans));
    }

    fn draw_wrapped(&mut self, text: &str, style: Style) {
        let prefix = self.block_prefix();
        for part in wrap_exact(text, self.width.saturating_sub(prefix.width()).max(1)) {
            self.line(&prefix, &part, style);
        }
    }

    fn draw_table(&mut self, table: Table) {
        let columns = table
            .alignments
            .len()
            .max(table.header.len())
            .max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
        if columns == 0 {
            return;
        }
        let prefix = self.block_prefix();
        let width = self.width.saturating_sub(prefix.width()).max(1);
        let separator = " | ";
        let natural: Vec<usize> = (0..columns)
            .map(|column| {
                std::iter::once(table.header.get(column))
                    .chain(table.rows.iter().map(|row| row.get(column)))
                    .flatten()
                    .map(|cell| cell.width())
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let natural_width =
            natural.iter().sum::<usize>() + separator.width() * columns.saturating_sub(1);
        let widths = if natural_width <= width {
            natural
        } else if width
            < columns
                .saturating_mul(8)
                .saturating_add(separator.width() * columns.saturating_sub(1))
        {
            self.draw_pairs(&prefix, width, &table, columns);
            return;
        } else {
            fit_columns(
                &natural,
                width.saturating_sub(separator.width() * columns.saturating_sub(1)),
            )
        };
        let drawn = widths.iter().sum::<usize>() + separator.width() * columns.saturating_sub(1);
        self.draw_row(&prefix, &table.header, &widths, &table.alignments, true);
        self.line(&prefix, &RULE.repeat(drawn), MARK);
        for row in &table.rows {
            self.draw_row(&prefix, row, &widths, &table.alignments, false);
        }
    }

    fn draw_row(
        &mut self,
        prefix: &str,
        row: &[String],
        widths: &[usize],
        aligns: &[Alignment],
        header: bool,
    ) {
        let cells: Vec<_> = widths
            .iter()
            .enumerate()
            .map(|(i, width)| wrap_words(row.get(i).map(String::as_str).unwrap_or(""), *width))
            .collect();
        let height = cells.iter().map(Vec::len).max().unwrap_or(1);
        for at in 0..height {
            let mut line = String::new();
            for i in 0..widths.len() {
                if i > 0 {
                    line.push_str(" | ");
                }
                let cell = cells[i].get(at).map(String::as_str).unwrap_or("");
                line.push_str(&pad(
                    cell,
                    widths[i],
                    aligns.get(i).copied().unwrap_or(Alignment::None),
                ));
            }
            self.line(
                prefix,
                &line,
                if header { TABLE_HEADER } else { Style::new() },
            );
        }
    }

    fn draw_pairs(&mut self, prefix: &str, width: usize, table: &Table, columns: usize) {
        for row in &table.rows {
            for i in 0..columns {
                let header = table.header.get(i).map(String::as_str).unwrap_or("");
                let value = row.get(i).map(String::as_str).unwrap_or("");
                for part in wrap_words(&format!("{header}: {value}"), width) {
                    self.line(prefix, &part, Style::new());
                }
            }
        }
    }
}

/// The grammars fenced code is coloured by, and the style of each scope.
struct Grammars {
    syntaxes: SyntaxSet,
    styles: Vec<(Scope, Style)>,
}

#[cfg(test)]
pub(crate) static GRAMMARS_BUILT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Built once for the process: the daemon hosts many consoles, and building
/// the set for each would cost every one of them the same work.
fn grammars() -> &'static Grammars {
    static GRAMMARS: OnceLock<Grammars> = OnceLock::new();
    GRAMMARS.get_or_init(|| {
        #[cfg(test)]
        GRAMMARS_BUILT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let styles = [
            ("comment", CODE_COMMENT),
            ("string", CODE_STRING),
            ("constant.numeric", CODE_NUMBER),
            ("keyword", CODE_KEYWORD),
            ("storage", CODE_KEYWORD),
            ("entity", CODE_TYPE),
            ("support.type", CODE_TYPE),
            ("support.class", CODE_TYPE),
        ];
        Grammars {
            syntaxes: SyntaxSet::load_defaults_nonewlines(),
            styles: styles
                .into_iter()
                .map(|(name, style)| (Scope::new(name).expect("a scope name parses"), style))
                .collect(),
        }
    })
}

/// The parse of one fenced block, carried from line to line: a string or a
/// comment that spans lines keeps its colour on the next.
struct Highlight {
    state: ParseState,
    stack: ScopeStack,
}

impl Highlight {
    /// None where the label names no grammar, so the block draws as before.
    fn new(language: &str) -> Option<Self> {
        let syntax = grammars().syntaxes.find_syntax_by_token(language)?;
        Some(Self {
            state: ParseState::new(syntax),
            stack: ScopeStack::new(),
        })
    }

    /// Where each style of `line` starts, by byte, or None where the grammar
    /// fails on it.
    fn line(&mut self, line: &str) -> Option<Vec<(usize, Style)>> {
        let ops = self.state.parse_line(line, &grammars().syntaxes).ok()?;
        let mut runs = Vec::new();
        mark(&mut runs, 0, self.style());
        for (at, op) in ops {
            self.stack.apply(&op).ok()?;
            mark(&mut runs, at, self.style());
        }
        Some(runs)
    }

    /// The style of the innermost scope the palette has a colour for.
    fn style(&self) -> Style {
        let styles = &grammars().styles;
        self.stack
            .as_slice()
            .iter()
            .rev()
            .find_map(|scope| {
                styles
                    .iter()
                    .find(|(prefix, _)| prefix.is_prefix_of(*scope))
                    .map(|(_, style)| *style)
            })
            .unwrap_or(CODE_PLAIN)
    }
}

/// Start a run of `style` at `at`, over one that started there too, and
/// merged into the run before where it has the same style.
fn mark(runs: &mut Vec<(usize, Style)>, at: usize, style: Style) {
    if runs.last().is_some_and(|&(last, _)| last == at) {
        runs.pop();
    }
    if runs.last().is_none_or(|&(_, last)| last != style) {
        runs.push((at, style));
    }
}

/// The spans of `text` within `range`, each in the style of its run. An empty
/// range is one empty span in the style it falls in.
fn styled(
    text: &str,
    range: std::ops::Range<usize>,
    runs: &[(usize, Style)],
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, &(at, style)) in runs.iter().enumerate() {
        let end = runs.get(i + 1).map_or(text.len(), |&(next, _)| next);
        let (from, to) = (at.max(range.start), end.min(range.end));
        if from < to {
            spans.push(Span::styled(text[from..to].to_string(), style));
        }
    }
    if spans.is_empty() {
        let style = runs
            .iter()
            .rev()
            .find(|&&(at, _)| at <= range.start)
            .map_or(CODE, |&(_, style)| style);
        spans.push(Span::styled(String::new(), style));
    }
    spans
}

fn cut(text: &str, width: usize) -> (String, Option<&str>) {
    let mut used = 0;
    let mut end = 0;
    for (at, grapheme) in text.grapheme_indices(true) {
        if used + grapheme.width() > width && end > 0 {
            return (text[..end].to_string(), Some(&text[end..]));
        }
        if used + grapheme.width() > width {
            let end = at + grapheme.len();
            return (text[..end].to_string(), Some(&text[end..]));
        }
        used += grapheme.width();
        end = at + grapheme.len();
    }
    (text.to_string(), None)
}

fn wrap_exact(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let (part, next) = cut(rest, width.max(1));
        out.push(part);
        let Some(next) = next else {
            break;
        };
        rest = next;
    }
    out
}

/// Text wrapped at its spaces, each line at most `width` wide. A word wider
/// than a line is cut, as [`wrap_exact`] would, rather than let out.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let wanted = if line.is_empty() {
            word.width()
        } else {
            line.width() + 1 + word.width()
        };
        if wanted <= width {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
            continue;
        }
        if !line.is_empty() {
            out.push(std::mem::take(&mut line));
        }
        let mut parts = wrap_exact(word, width);
        line = parts.pop().unwrap_or_default();
        out.extend(parts);
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
    out
}

/// Column widths for a table wider than `room`: a column that fits its fair
/// share keeps its natural width, and the columns that do not share what the
/// others left, so a narrow `#` column does not take a third of the row.
fn fit_columns(natural: &[usize], room: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..natural.len()).collect();
    order.sort_by_key(|&column| natural[column]);
    let mut widths = natural.to_vec();
    let mut left = room;
    for (at, &column) in order.iter().enumerate() {
        let rest = &order[at..];
        if natural[column] <= left / rest.len() {
            left -= natural[column];
            continue;
        }
        for (&column, share) in rest.iter().zip(distribute(left, rest.len())) {
            widths[column] = share.max(1);
        }
        break;
    }
    widths
}

fn distribute(width: usize, columns: usize) -> Vec<usize> {
    let base = width / columns;
    let extra = width % columns;
    (0..columns)
        .map(|i| base + usize::from(i < extra))
        .collect()
}

fn pad(text: &str, width: usize, alignment: Alignment) -> String {
    let space = width.saturating_sub(text.width());
    match alignment {
        Alignment::Right => format!("{}{}", " ".repeat(space), text),
        Alignment::Center => format!(
            "{}{}{}",
            " ".repeat(space / 2),
            text,
            " ".repeat(space - space / 2)
        ),
        Alignment::None | Alignment::Left => format!("{}{}", text, " ".repeat(space)),
    }
}

fn trim_space(spans: &mut Vec<Span<'static>>) {
    while spans
        .last()
        .is_some_and(|span| span.content.trim().is_empty())
    {
        spans.pop();
    }
}

fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut space = false;
    for character in text.chars() {
        let now_space = character.is_whitespace();
        if !word.is_empty() && now_space != space {
            words.push(std::mem::take(&mut word));
        }
        space = now_space;
        word.push(character);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn heading_depth(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

trait Blank {
    fn is_blank(&self) -> bool;
}
impl Blank for Line<'_> {
    fn is_blank(&self) -> bool {
        self.spans.iter().all(|span| span.content.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;

    fn text(lines: &[Line<'static>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_code_span_in_a_table_cell_keeps_its_backticks_as_in_text() {
        let lines = text(&render(
            "Run `cargo test`.\n\n| Key | Does |\n|---|---|\n| `enter` | send |\n",
            60,
        ));

        assert!(
            lines.iter().any(|line| line.contains("`cargo test`")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("`enter`")),
            "{lines:?}"
        );
    }
    fn styles(lines: &[Line<'static>], wanted: &str) -> Option<Style> {
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.content.contains(wanted))
            .map(|span| span.style)
    }
    fn widest(lines: &[Line<'static>]) -> usize {
        lines
            .iter()
            .map(|line| line.to_string().width())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn a_heading_a_code_block_and_a_list_each_keep_their_own_style() {
        let lines = render(
            "# Report\n\nRan **all** the `cargo` tests:\n\n```sh\ncargo nextest run\n```\n\n- one\n- two",
            60,
        );
        let rendered = text(&lines);
        assert!(rendered.contains(&"sh".to_string()));
        assert!(rendered.contains(&"  cargo nextest run".to_string()));
        assert!(!rendered.iter().any(|line| line.contains("```")));
        assert_eq!(styles(&lines, "Report"), Some(HEADING));
        assert_eq!(styles(&lines, "`cargo`"), Some(CODE));
    }

    #[test]
    fn a_table_aligns_wide_cells_under_its_headers() {
        let lines = render(
            "| name | place | count |\n| --- | --- | --- |\n| 日本 | Madrid | 2 |",
            40,
        );
        assert_eq!(text(&lines)[0], "name | place  | count");
        assert_eq!(text(&lines)[1], RULE.repeat(21));
        assert_eq!(text(&lines)[2], "日本 | Madrid | 2    ");
        // One line under the header: the rule, not an underline as well.
        assert_eq!(styles(&lines, "name"), Some(TABLE_HEADER));
    }

    #[test]
    fn a_right_aligned_table_column_is_flush_right() {
        let lines = render("| item | total |\n| --- | ---: |\n| tea | 12 |", 20);
        assert_eq!(text(&lines)[2], "tea  |    12");
    }

    #[test]
    fn a_wide_table_wraps_each_cell_without_losing_text() {
        let source = "| first | second | third |\n| --- | --- | --- |\n| aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb | cccccccccccccccccccccccccccccccccccccccccccccc |";
        let lines = render(source, 80);
        let rendered = text(&lines).join("");
        assert!(widest(&lines) <= 80);
        assert_eq!(rendered.matches('a').count(), 46);
        assert_eq!(rendered.matches('b').count(), 46);
        assert_eq!(rendered.matches('c').count(), 47);
    }

    #[test]
    fn a_wide_table_keeps_short_columns_whole_and_wraps_long_cells_at_spaces() {
        let task = "Repositories graph with a sidebar entry and pickers";
        let source =
            format!("| # | Task | Depends on |\n| --- | --- | --- |\n| A | {task} | B, E |");
        let lines = render(&source, 40);
        let rendered = text(&lines);
        assert!(widest(&lines) <= 40, "{rendered:?}");
        assert!(rendered[0].starts_with("# | Task"), "{rendered:?}");
        assert!(rendered[0].ends_with("| Depends on"), "{rendered:?}");
        // The rule is as wide as the rows it sits between.
        assert_eq!(rendered[1].width(), rendered[0].width());
        let words: Vec<&str> = rendered[2..]
            .iter()
            .flat_map(|row| row.split(" | ").nth(1).unwrap_or("").split_whitespace())
            .collect();
        assert_eq!(words, task.split_whitespace().collect::<Vec<_>>());
    }

    #[test]
    fn a_word_wider_than_its_cell_is_cut_not_let_out() {
        assert_eq!(wrap_words("abcdefgh ij", 3), ["abc", "def", "gh", "ij"]);
        assert_eq!(wrap_words("", 3), [""]);
    }

    #[test]
    fn a_narrow_table_draws_header_value_pairs() {
        let lines = render(
            "| first | second | third |\n| --- | --- | --- |\n| one | two | three |",
            20,
        );
        assert_eq!(text(&lines), ["first: one", "second: two", "third: three"]);
    }

    #[test]
    fn a_long_code_line_wraps_with_continuation_marks_without_loss() {
        let source = "x".repeat(200);
        let rendered = text(&render(&format!("```rust\n{source}\n```"), 80));
        let body: String = rendered[1..]
            .iter()
            .map(|line| {
                line.trim_start_matches("  ")
                    .trim_start_matches(CODE_CONTINUATION)
            })
            .collect();
        assert_eq!(rendered.len(), 4);
        assert_eq!(rendered[1..].len(), 3);
        assert!(rendered[2].starts_with(CODE_CONTINUATION));
        assert!(rendered[3].starts_with(CODE_CONTINUATION));
        assert_eq!(body, source);
    }

    #[test]
    fn links_keep_their_destination_once() {
        assert_eq!(
            text(&render(
                "[docs](https://example.com) <https://example.com>",
                80
            )),
            ["docs (https://example.com) https://example.com"]
        );
    }

    #[test]
    fn nested_lists_use_a_glyph_and_indent_for_each_depth() {
        assert_eq!(
            text(&render(
                "- one\n  - two\n    - three\n      1. four\n- [x] done",
                80
            )),
            [
                "• one",
                "  ◦ two",
                "    ▪ three",
                "      1. four",
                "• ☑ done"
            ]
        );
    }

    #[test]
    fn every_wrapped_quote_row_keeps_its_bar() {
        assert_eq!(
            text(&render("> alpha beta gamma delta", 12)),
            ["│ alpha beta", "│ gamma", "│ delta"]
        );
    }

    #[test]
    fn nested_quote_rows_keep_one_bar_for_each_level() {
        assert_eq!(
            text(&render("> > alpha beta gamma", 16)),
            ["│ │ alpha beta", "│ │ gamma"]
        );
    }

    #[test]
    fn every_prefix_of_streamed_markdown_renders_without_a_panic() {
        let source = "| a | b |\n| --- | --- |\n| one | two |\n\n```rust\nlet x = 1;\n\n[docs](https://example.com)\n\n- one\n  - two\n\n> quote";
        for width in [1, 20, 80] {
            for end in 0..=source.len() {
                if source.is_char_boundary(end) {
                    let _ = render(&source[..end], width);
                }
            }
        }
    }

    #[test]
    fn markdown_never_returns_a_line_wider_than_its_width() {
        let source = "| one | two | three |\n| --- | ---: | --- |\n| 日本語日本語日本語 | abcdefghijklmnopqrstuvwxyz | xyz |\n\n```rust\nabcdefghijk\n```\n\n> alpha beta gamma\n\n- a longwordwithoutspaces";
        for width in [20, 80, 120] {
            let lines = render(source, width);
            assert!(widest(&lines) <= width, "{width}: {:?}", text(&lines));
        }
    }

    #[test]
    fn a_paragraph_wraps_at_the_width_it_is_drawn_at() {
        assert_eq!(
            text(&render("alpha beta gamma delta epsilon", 12)),
            ["alpha beta", "gamma delta", "epsilon"]
        );
    }
    #[test]
    fn a_paragraph_of_wide_characters_wraps_at_the_display_width() {
        assert_eq!(
            text(&render("日本語 日本語 日本語", 13)),
            ["日本語 日本語", "日本語"]
        );
    }
    #[test]
    fn a_code_line_is_cut_between_whole_emoji_sequences() {
        assert_eq!(
            text(&render("```\nab\u{1f469}\u{200d}\u{1f52c}cd\n```", 4)),
            ["  ab", "↪\u{1f469}\u{200d}\u{1f52c}c", "↪d"]
        );
    }
    #[test]
    fn a_word_longer_than_the_line_wraps_at_grapheme_boundaries() {
        assert_eq!(
            text(&render("short supercalifragilistic end", 6)),
            ["short", "superc", "alifra", "gilist", "ic end"]
        );
    }
    #[test]
    fn an_ordered_list_numbers_its_items_and_indents_what_wraps() {
        assert_eq!(
            text(&render("1. first item here\n2. second", 12)),
            ["1. first", "   item here", "2. second"]
        );
    }
    fn code_styles(lines: &[Line<'static>]) -> Vec<Style> {
        lines
            .iter()
            .flat_map(|line| line.spans.iter().skip(1))
            .filter(|span| !span.content.trim().is_empty())
            .map(|span| span.style)
            .collect()
    }

    #[test]
    fn a_rust_fence_colours_a_comment_a_string_a_keyword_and_a_number_apart() {
        let lines = render(
            "```rust\n// the answer\nlet name = \"forty\";\nlet n = 42;\n```",
            80,
        );

        assert_eq!(styles(&lines, "the answer"), Some(CODE_COMMENT));
        assert_eq!(styles(&lines, "forty"), Some(CODE_STRING));
        assert_eq!(styles(&lines, "let"), Some(CODE_KEYWORD));
        assert_eq!(styles(&lines, "42"), Some(CODE_NUMBER));
        assert_eq!(
            text(&lines),
            [
                "rust",
                "  // the answer",
                "  let name = \"forty\";",
                "  let n = 42;"
            ]
        );
    }

    #[test]
    fn a_json_a_yaml_and_a_sh_fence_are_coloured() {
        let json = render("```json\n{\"count\": 3}\n```", 80);
        assert_eq!(styles(&json, "3"), Some(CODE_NUMBER), "{json:?}");
        assert!(code_styles(&json).contains(&CODE_STRING), "{json:?}");

        let yaml = render("```yaml\nname: \"console\" # the crate\n```", 80);
        assert_eq!(styles(&yaml, "the crate"), Some(CODE_COMMENT), "{yaml:?}");
        assert!(code_styles(&yaml).contains(&CODE_STRING), "{yaml:?}");

        let sh = render("```sh\necho \"hi\" # greet\n```", 80);
        assert_eq!(styles(&sh, "greet"), Some(CODE_COMMENT), "{sh:?}");
        assert!(code_styles(&sh).contains(&CODE_STRING), "{sh:?}");
    }

    #[test]
    fn a_fence_with_no_label_or_an_unknown_one_draws_in_the_one_code_colour() {
        for (source, label) in [
            ("```\nlet n = 42; // x\n\nn\n```", None),
            (
                "```nosuchlanguage\nlet n = 42; // x\n\nn\n```",
                Some("nosuchlanguage"),
            ),
            // syntect bundles no TOML grammar.
            ("```toml\nlet n = 42; // x\n\nn\n```", Some("toml")),
        ] {
            let lines = render(source, 80);
            let mut wanted = Vec::new();
            if let Some(label) = label {
                wanted.push(Line::from(Span::styled(label.to_string(), MARK)));
            }
            for code in ["let n = 42; // x", "", "n"] {
                wanted.push(Line::from(vec![
                    Span::styled("  ", MARK),
                    Span::styled(code, CODE),
                ]));
            }
            assert_eq!(lines, wanted, "{source}");
        }
    }

    #[test]
    fn the_same_fence_drawn_twice_has_the_same_styles() {
        let source = "```rust\n/* a\n   b */ fn main() { let s = \"x\"; }\n```";
        assert_eq!(render(source, 20), render(source, 20));
    }

    #[test]
    fn fenced_code_uses_the_ansi_palette_alone() {
        let source = [
            "```rust\n#[derive(Debug)]\npub struct A<T: Copy>(u8, &'static str);\nfn f() -> i32 { 0x1f }\n```",
            "```json\n{\"a\": [1, true, null]}\n```",
            "```sh\nfor f in *.rs; do echo \"$f\"; done\n```",
            "```python\n@dataclass\nclass A:\n    x: int = 1  # one\n```",
        ]
        .join("\n\n");
        for span in render(&source, 80).iter().flat_map(|line| &line.spans) {
            for colour in [span.style.fg, span.style.bg].into_iter().flatten() {
                assert!(
                    !matches!(colour, Color::Rgb(..) | Color::Indexed(_)),
                    "{span:?}"
                );
            }
        }
    }

    #[test]
    fn coloured_code_wraps_by_display_width_with_its_indent_and_continuation() {
        let source = format!("let s = \"{}日本\"; // done", "x".repeat(20));
        let lines = render(&format!("```rust\n{source}\n```"), 16);
        let rendered = text(&lines);

        assert!(widest(&lines) <= 16, "{rendered:?}");
        assert!(rendered[1].starts_with("  let"), "{rendered:?}");
        assert!(
            rendered[2..]
                .iter()
                .all(|line| line.starts_with(CODE_CONTINUATION)),
            "{rendered:?}"
        );
        let body: String = rendered[1..]
            .iter()
            .map(|line| {
                line.trim_start_matches("  ")
                    .trim_start_matches(CODE_CONTINUATION)
            })
            .collect();
        assert_eq!(body, source);
        // A string cut across two rows keeps its colour on both.
        assert!(
            lines[1..3]
                .iter()
                .all(|line| line.spans.iter().any(|span| span.style == CODE_STRING)),
            "{lines:?}"
        );
        assert_eq!(styles(&lines, "done"), Some(CODE_COMMENT));
    }

    #[test]
    fn every_prefix_of_a_coloured_fence_renders_without_a_panic() {
        let source =
            "```rust\n/* open\nfn main() { let s = \"日本\u{1f469}\u{200d}\u{1f52c}\"; }\n```";
        for width in [1, 4, 80] {
            for end in 0..=source.len() {
                if source.is_char_boundary(end) {
                    let _ = render(&source[..end], width);
                }
            }
        }
    }

    #[test]
    fn plain_text_with_no_markdown_in_it_survives_unchanged() {
        assert_eq!(text(&render("just a sentence.", 40)), ["just a sentence."]);
    }
}
