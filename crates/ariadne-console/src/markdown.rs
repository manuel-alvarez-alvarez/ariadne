//! Agent markdown as styled terminal lines.
//!
//! An agent writes markdown, so the console reads it as markdown: headings,
//! bold, code spans, fenced code and lists, wrapped to the width it is drawn
//! at. [`pulldown-cmark`](https://docs.rs/pulldown-cmark) parses it and
//! nothing else — it carries no terminal types of its own, which is what lets
//! every event below be styled in the console's own palette rather than in a
//! second crate's, and keeps one ratatui in the tree.
//!
//! Everything here is a pure function of the text and the width, so the whole
//! module is testable without a terminal.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A heading: the one thing on a long answer a reader's eye jumps between.
const HEADING: Style = Style::new()
    .fg(Color::Cyan)
    .add_modifier(Modifier::BOLD.union(Modifier::UNDERLINED));
/// A code span, and the body of a fenced block.
const CODE: Style = Style::new().fg(Color::Yellow);
/// The fence itself, a list bullet, a quote bar: the marks around the text.
const MARK: Style = Style::new().add_modifier(Modifier::DIM);

/// Render `text` as markdown, wrapped to `width` columns.
///
/// A width of zero is a viewport with no room in it; one column is the
/// narrowest wrap that still terminates.
pub fn render(text: &str, width: usize) -> Vec<Line<'static>> {
    Writer::new(width.max(1)).run(text)
}

/// The inline styles open at one point in the parse, innermost last.
#[derive(Default)]
struct Writer {
    width: usize,
    lines: Vec<Line<'static>>,
    /// The paragraph being built, as styled words waiting to be wrapped.
    pending: Vec<Span<'static>>,
    style: Style,
    /// The prefix every line of the current block carries: list indent, the
    /// bar of a block quote.
    indent: String,
    /// One entry per open list: `None` for a bullet list, the next number for
    /// an ordered one.
    lists: Vec<Option<u64>>,
    /// Inside a fenced or indented code block, where text is kept verbatim.
    code: bool,
}

impl Writer {
    fn new(width: usize) -> Self {
        Self {
            width,
            ..Self::default()
        }
    }

    fn run(mut self, text: &str) -> Vec<Line<'static>> {
        let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
        for event in Parser::new_ext(text, options) {
            self.event(event);
        }
        self.flush();
        // A trailing blank line is a block separator with nothing after it.
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
                if self.code {
                    for line in text.lines() {
                        self.verbatim(line);
                    }
                } else {
                    self.push(&text, self.style);
                }
            }
            Event::Code(code) => self.push(&format!("`{code}`"), CODE),
            Event::SoftBreak => self.push(" ", self.style),
            Event::HardBreak => self.wrap_pending(),
            Event::Rule => {
                self.flush();
                self.lines
                    .push(Line::from(Span::styled("─".repeat(self.width), MARK)));
            }
            Event::TaskListMarker(done) => {
                self.push(if done { "[x] " } else { "[ ] " }, MARK);
            }
            Event::FootnoteReference(name) => self.push(&format!("[^{name}]"), MARK),
            Event::InlineMath(math) | Event::DisplayMath(math) => self.push(&math, CODE),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.blank();
                self.style = HEADING;
                // The hashes stay: they are how a reader tells an `h3` from an
                // `h1` when both are one bold line.
                self.push(&format!("{} ", "#".repeat(heading_depth(level))), MARK);
                self.style = HEADING;
            }
            Tag::Paragraph => self.blank(),
            Tag::BlockQuote(_) => {
                self.blank();
                self.indent.push_str("│ ");
            }
            Tag::CodeBlock(kind) => {
                self.blank();
                let language = match &kind {
                    CodeBlockKind::Fenced(language) => language.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.fence(&language);
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
                    _ => "• ".to_string(),
                };
                self.push(&marker, MARK);
                // Continuation lines of one item line up under its text.
                self.indent.push_str(&" ".repeat(marker.width()));
            }
            Tag::Emphasis => self.style = self.style.add_modifier(Modifier::ITALIC),
            Tag::Strong => self.style = self.style.add_modifier(Modifier::BOLD),
            Tag::Strikethrough => self.style = self.style.add_modifier(Modifier::CROSSED_OUT),
            Tag::Link { .. } | Tag::Image { .. } => self.style = self.style.fg(Color::Blue),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) | TagEnd::Paragraph => {
                self.style = Style::new();
                self.flush();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.dedent(2);
            }
            TagEnd::CodeBlock => {
                self.code = false;
                self.fence("");
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush();
                let marker = self.indent.len() - self.indent.trim_end_matches(' ').len();
                self.dedent(marker);
            }
            TagEnd::Emphasis => self.style = self.style.remove_modifier(Modifier::ITALIC),
            TagEnd::Strong => self.style = self.style.remove_modifier(Modifier::BOLD),
            TagEnd::Strikethrough => self.style = self.style.remove_modifier(Modifier::CROSSED_OUT),
            TagEnd::Link | TagEnd::Image => self.style = Style::new(),
            _ => {}
        }
    }

    /// Add inline text to the paragraph being built.
    fn push(&mut self, text: &str, style: Style) {
        if !text.is_empty() {
            self.pending.push(Span::styled(text.to_string(), style));
        }
    }

    /// One line of a code block: kept as written, cut rather than wrapped, so
    /// its indentation still lines up. The cut falls between grapheme
    /// clusters, each as wide as it draws, so an emoji of several characters
    /// is kept or dropped whole.
    fn verbatim(&mut self, text: &str) {
        let room = self.width.saturating_sub(self.indent.width()).max(1);
        let mut used = 0;
        let text: String = text
            .graphemes(true)
            .take_while(|grapheme| {
                used += grapheme.width();
                used <= room
            })
            .collect();
        self.lines.push(Line::from(vec![
            Span::styled(self.indent.clone(), MARK),
            Span::styled(text, CODE),
        ]));
    }

    fn fence(&mut self, language: &str) {
        self.lines.push(Line::from(vec![
            Span::styled(self.indent.clone(), MARK),
            Span::styled(format!("```{language}"), MARK),
        ]));
    }

    /// End the paragraph being built, wrapping it into lines.
    fn flush(&mut self) {
        if !self.pending.is_empty() {
            self.wrap_pending();
        }
    }

    /// A blank line between blocks, never two of them and never a leading one.
    fn blank(&mut self) {
        self.flush();
        if !self.lines.is_empty() && !self.lines.last().is_some_and(Line::is_blank) {
            self.lines.push(Line::default());
        }
    }

    fn dedent(&mut self, columns: usize) {
        let keep = self.indent.len().saturating_sub(columns);
        self.indent.truncate(keep);
    }

    /// Greedy word wrap over styled spans: words keep the style of the span
    /// they came from, and a word longer than the line stands on its own.
    fn wrap_pending(&mut self) {
        let room = self.width.saturating_sub(self.indent.width()).max(1);
        // The first line of a list item carries its marker, which the pending
        // spans already hold; the rest carry the indent alone.
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut used = 0usize;
        let mut first = true;
        let mut flush_line = |spans: &mut Vec<Span<'static>>, first: &mut bool| {
            let mut line = Vec::new();
            if !*first && !self.indent.is_empty() {
                line.push(Span::styled(self.indent.clone(), MARK));
            }
            line.append(spans);
            self.lines.push(Line::from(line));
            *first = false;
        };
        for span in std::mem::take(&mut self.pending) {
            for word in words(&span.content) {
                let length = word.width();
                if word.trim().is_empty() {
                    // Leading space on a fresh line is the wrap's, not the
                    // text's.
                    if used > 0 {
                        used += length;
                        spans.push(Span::styled(word, span.style));
                    }
                    continue;
                }
                if used > 0 && used + length > room {
                    // Do not carry the space the wrap happened at.
                    while spans.last().is_some_and(|s| s.content.trim().is_empty()) {
                        spans.pop();
                    }
                    flush_line(&mut spans, &mut first);
                    used = 0;
                }
                used += length;
                spans.push(Span::styled(word, span.style));
            }
        }
        while spans.last().is_some_and(|s| s.content.trim().is_empty()) {
            spans.pop();
        }
        if !spans.is_empty() || first {
            flush_line(&mut spans, &mut first);
        }
    }
}

/// The text split into words and the runs of space between them, so a wrap
/// can drop the space it broke at and keep the rest.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut space = false;
    for character in text.chars() {
        let is_space = character.is_whitespace();
        if !current.is_empty() && is_space != space {
            out.push(std::mem::take(&mut current));
        }
        space = is_space;
        current.push(character);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
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
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.contains(wanted))
            .map(|span| span.style)
    }

    #[test]
    fn a_heading_a_code_block_and_a_list_each_keep_their_own_style() {
        let lines = render(
            "# Report\n\nRan **all** the `cargo` tests:\n\n```sh\ncargo nextest run\n```\n\n\
             - one\n- two\n",
            60,
        );

        let rendered = text(&lines);
        assert!(rendered.contains(&"# Report".to_string()), "{rendered:?}");
        assert!(
            rendered.contains(&"cargo nextest run".to_string()),
            "{rendered:?}"
        );
        assert!(rendered.contains(&"```sh".to_string()), "{rendered:?}");
        assert!(rendered.contains(&"• one".to_string()), "{rendered:?}");
        assert!(rendered.contains(&"• two".to_string()), "{rendered:?}");
        assert_eq!(styles(&lines, "Report"), Some(HEADING));
        assert_eq!(
            styles(&lines, "all").map(|style| style.add_modifier),
            Some(Modifier::BOLD)
        );
        assert_eq!(styles(&lines, "`cargo`"), Some(CODE));
        assert_eq!(styles(&lines, "cargo nextest run"), Some(CODE));
    }

    #[test]
    fn a_paragraph_wraps_at_the_width_it_is_drawn_at() {
        let lines = render("alpha beta gamma delta epsilon", 12);

        assert_eq!(text(&lines), ["alpha beta", "gamma delta", "epsilon"]);
    }

    /// Three words of three CJK characters, six columns each: two fill a
    /// line thirteen columns wide, where a count of characters would fit all
    /// three.
    #[test]
    fn a_paragraph_of_wide_characters_wraps_at_the_display_width() {
        let lines = render("日本語 日本語 日本語", 13);

        assert_eq!(text(&lines), ["日本語 日本語", "日本語"]);
    }

    /// A woman scientist: three characters joined into one two-column
    /// emoji. A cut that counted characters would drop her or split her.
    #[test]
    fn a_code_line_is_cut_between_whole_emoji_sequences() {
        let lines = render("```\nab\u{1f469}\u{200d}\u{1f52c}cd\n```", 4);

        assert_eq!(
            text(&lines),
            ["```", "ab\u{1f469}\u{200d}\u{1f52c}", "```"],
            "the emoji fits in the two columns left and is kept whole"
        );
    }

    #[test]
    fn a_word_longer_than_the_line_stands_on_its_own_rather_than_looping() {
        let lines = render("short supercalifragilistic end", 6);

        assert_eq!(
            text(&lines),
            ["short", "supercalifragilistic", "end"],
            "an unbreakable word overflows once instead of never terminating"
        );
    }

    #[test]
    fn an_ordered_list_numbers_its_items_and_indents_what_wraps() {
        let lines = render("1. first item here\n2. second\n", 12);

        assert_eq!(
            text(&lines),
            ["1. first", "   item here", "2. second"],
            "a continuation line lines up under its item's text"
        );
    }

    #[test]
    fn plain_text_with_no_markdown_in_it_survives_unchanged() {
        let lines = render("just a sentence.", 40);

        assert_eq!(text(&lines), ["just a sentence."]);
    }
}
