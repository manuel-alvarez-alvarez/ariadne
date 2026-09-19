//! Agent markdown as styled terminal lines.
//!
//! This renderer is a pure function of text and width. It measures terminal
//! columns rather than bytes and cuts only between grapheme clusters.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::{
    CODE, CODE_CONTINUATION, HEADING, LIST_BULLETS, MARK, QUOTE_BAR, RULE, TASK_DONE, TASK_TODO,
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
                if self.in_table_cell(&code) {
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
            TagEnd::CodeBlock => self.code = false,
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
        let mut rest = text;
        let mut first_line = true;
        loop {
            let prefix = if first_line { &first } else { &continued };
            let (part, next) = cut(rest, self.width.saturating_sub(prefix.width()).max(1));
            self.line(prefix, &part, CODE);
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
            distribute(
                width.saturating_sub(separator.width() * columns.saturating_sub(1)),
                columns,
            )
        };
        self.draw_row(&prefix, &table.header, &widths, &table.alignments, true);
        self.line(&prefix, &RULE.repeat(natural_width.min(width)), MARK);
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
            .map(|(i, width)| wrap_exact(row.get(i).map(String::as_str).unwrap_or(""), *width))
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
                if header {
                    HEADING.add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                },
            );
        }
    }

    fn draw_pairs(&mut self, prefix: &str, width: usize, table: &Table, columns: usize) {
        for row in &table.rows {
            for i in 0..columns {
                let header = table.header.get(i).map(String::as_str).unwrap_or("");
                let value = row.get(i).map(String::as_str).unwrap_or("");
                for part in wrap_exact(&format!("{header}: {value}"), width) {
                    self.line(prefix, &part, Style::new());
                }
            }
        }
    }
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
    use super::*;

    fn text(lines: &[Line<'static>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
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
        assert_eq!(
            styles(&lines, "name"),
            Some(HEADING.add_modifier(Modifier::BOLD))
        );
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
    #[test]
    fn plain_text_with_no_markdown_in_it_survives_unchanged() {
        assert_eq!(text(&render("just a sentence.", 40)), ["just a sentence."]);
    }
}
