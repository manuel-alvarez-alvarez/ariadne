//! Wrapping and cutting text by display width, shared by every renderer: the
//! pane's blocks and banner, the input box, the picker and the markdown
//! writer.
//!
//! Every cut here falls between grapheme clusters, never inside one, so a
//! wide character draws the two columns it takes and a multi-code-point
//! emoji sequence is always kept or dropped whole.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The columns `text` draws.
pub(crate) fn width(text: &str) -> usize {
    text.width()
}

/// The longest prefix of `text` that fits `room` columns, cut at a grapheme
/// boundary. Unlike [`cut`], a grapheme that alone is wider than `room` is
/// left out rather than forced in, so the prefix never overruns `room`.
fn clip_prefix(text: &str, room: usize) -> String {
    let mut cut = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let columns = grapheme.width();
        if used + columns > room.saturating_sub(1) {
            break;
        }
        used += columns;
        cut.push_str(grapheme);
    }
    cut
}

/// `text` cut to `room` columns, saying so. The cut falls between grapheme
/// clusters, each as wide as it draws, so an emoji of several characters is
/// kept or dropped whole.
pub(crate) fn clip(text: &str, room: usize) -> String {
    if text.width() <= room {
        return text.to_string();
    }
    let mut cut = clip_prefix(text, room);
    cut.push('…');
    cut
}

/// `parts`, each with its own style, clipped to `room` columns kept across
/// all of them together — as [`clip`] cuts one string — so a styled run
/// never overruns the room a plain line would have. The prefix that fits,
/// and the style the ellipsis after it takes: the style of the part the cut
/// fell in, or `None` where every one of them already fit.
pub(crate) fn clip_spans<S: Copy>(parts: &[(String, S)], room: usize) -> (String, Option<S>) {
    let full: String = parts.iter().map(|(text, _)| text.as_str()).collect();
    if full.width() <= room {
        return (full, None);
    }
    let prefix = clip_prefix(&full, room);
    let mut style = parts.first().map(|(_, style)| *style);
    let mut offset = 0;
    for (text, part_style) in parts {
        let (start, end) = (offset, offset + text.len());
        if start <= prefix.len() && prefix.len() < end {
            style = Some(*part_style);
        }
        offset = end;
    }
    (prefix, style)
}

/// `text`, with the longest prefix that fits `width` columns split off at a
/// grapheme boundary: the fitting prefix, and the rest where there is one. A
/// grapheme wider than `width` on its own is kept rather than dropped, so a
/// row of calls to `cut` always makes progress.
pub(crate) fn cut(text: &str, width: usize) -> (String, Option<&str>) {
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

/// `text`, one line, cut to `width`-column rows at grapheme boundaries
/// alone: no word is kept whole, and a row wider than `width` on its own —
/// one grapheme — is never split. An empty string is one empty row.
pub(crate) fn wrap_exact(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let width = width.max(1);
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let (part, next) = cut(rest, width);
        out.push(part);
        let Some(next) = next else {
            break;
        };
        rest = next;
    }
    out
}

/// `text` wrapped at its spaces, each line at most `width` columns: a word
/// wider than a line is cut, as [`wrap_exact`] would, rather than let out.
/// Whitespace runs collapse to one space between words, as a markdown table
/// cell's text does.
pub(crate) fn wrap_words(text: &str, width: usize) -> Vec<String> {
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

/// Hard-wrap `text` to `width` columns, keeping the line breaks it already
/// has. A word wider than a row is cut between grapheme clusters across
/// rows, so no character of it is lost past the edge. The space after a word
/// takes no room at the end of a row, where it is not drawn.
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
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

/// Wrap `text` to `width` columns and keep every character of it, spaces
/// included: a word goes to the next row with the space after it where the
/// two do not fit, and a word wider than a row starts where the row is and
/// is cut between grapheme clusters. The rows of one line, joined, are that
/// line.
pub(crate) fn wrap_whole(text: &str, width: usize) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A wide character, counted at its two columns, and never half-kept by
    /// a cut that landed inside it.
    #[test]
    fn a_wide_character_is_counted_at_two_columns_and_never_split() {
        assert_eq!(width("日"), 2);
        assert_eq!(cut("日本", 3), ("日".to_string(), Some("本")));
        assert_eq!(clip("日本語", 3), "日…");
        assert_eq!(wrap_exact("日本語", 3), ["日", "本", "語"]);
    }

    /// A woman scientist, three code points joined into one two-column
    /// grapheme cluster: kept whole by every cut, never split across rows.
    #[test]
    fn an_emoji_of_several_code_points_is_kept_whole() {
        let emoji = "\u{1f469}\u{200d}\u{1f52c}";
        let text = format!("ab{emoji}cd");
        assert_eq!(width(emoji), 2);
        let (first, rest) = cut(&text, 3);
        assert_eq!(first, "ab");
        assert_eq!(rest, Some(format!("{emoji}cd")).as_deref());
        assert_eq!(wrap_exact(&text, 2), ["ab", emoji, "cd"]);
        assert_eq!(clip(&text, 3), "ab…");
    }

    /// A word wider than the width it wraps at is cut rather than let out,
    /// whichever of the wrapping functions cuts it.
    #[test]
    fn a_word_wider_than_the_width_is_cut_not_let_out() {
        assert_eq!(wrap_words("abcdefgh ij", 3), ["abc", "def", "gh", "ij"]);
        assert_eq!(
            wrap("one abcdefgh two", 3),
            ["one", "abc", "def", "gh", "two"]
        );
        assert_eq!(wrap_whole("abcdefgh", 3), ["abc", "def", "gh"]);
    }

    /// An empty string wraps and clips to nothing, never a lone ellipsis or
    /// a missing row.
    #[test]
    fn an_empty_string_wraps_to_nothing() {
        assert_eq!(wrap_exact("", 10), [""]);
        assert_eq!(wrap_words("", 10), [""]);
        assert_eq!(wrap("", 10), [""]);
        assert_eq!(wrap_whole("", 10), [""]);
        assert_eq!(clip("", 10), "");
    }
}
