//! Tables that fit the terminal they are printed in.
//!
//! A table declares its columns once — header, the width its cells are cut
//! to, how important it is, and what kind of thing it holds — and the
//! renderer decides what actually fits: it drops the least important columns
//! first, then narrows what is left, so the row still reads on an 80-column
//! terminal instead of wrapping into three lines of rubble. `-o wide`,
//! `--columns` and `--no-trunc` each say "print it all" and turn that off.
//!
//! [`render_table`] returns the table rather than printing it, so a screen
//! that redraws — a follow mode, a watch — renders the same rows the same way
//! without going through stdout.

use anstyle::Style;
use anyhow::Result;

use super::{style, width};

/// Width for a column that is never truncated.
pub const UNCAPPED: usize = 0;

/// Importance of a column that is never dropped, whatever the terminal is:
/// the id and the title of the thing the row is about.
pub const KEEP: u8 = u8::MAX;

/// The ellipsis a cut cell ends with.
const ELLIPSIS: char = '…';

/// What separates two columns.
const GAP: usize = 3;

/// Narrowest a column is squeezed to before the renderer gives up on fitting:
/// past this a cell is more ellipsis than content.
const MIN_WIDTH: usize = 8;

/// What a cell holds, which is what says how it is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// Anything with no meaning of its own: counts, branches, flags.
    Plain,
    /// An id: there to be copied, not read.
    Id,
    /// What the row is about.
    Title,
    /// A lifecycle status — coloured, and prefixed with its glyph.
    Status,
    /// Why a row wants a person, in `ariadne attention`'s words.
    Attention,
}

/// One column of a table.
#[derive(Debug, Clone, Copy)]
pub struct Column {
    /// Printed uppercase as the header, and the name `--columns` takes.
    pub header: &'static str,
    /// Width its cells are cut to, or [`UNCAPPED`].
    pub cap: usize,
    /// What is dropped first when the table does not fit: the lowest rank
    /// goes first, and [`KEEP`] never goes.
    pub rank: u8,
    pub cell: Cell,
}

/// A column that is kept whatever the terminal's width: build it, then say
/// what it holds and how important it is.
pub const fn col(header: &'static str, cap: usize) -> Column {
    Column {
        header,
        cap,
        rank: KEEP,
        cell: Cell::Plain,
    }
}

impl Column {
    /// How droppable this column is: `0` goes first. Left off, it stays.
    pub const fn rank(self, rank: u8) -> Self {
        Self { rank, ..self }
    }

    pub const fn id(self) -> Self {
        Self {
            cell: Cell::Id,
            ..self
        }
    }

    pub const fn title(self) -> Self {
        Self {
            cell: Cell::Title,
            ..self
        }
    }

    pub const fn status(self) -> Self {
        Self {
            cell: Cell::Status,
            ..self
        }
    }

    pub const fn attention(self) -> Self {
        Self {
            cell: Cell::Attention,
            ..self
        }
    }
}

/// How the display flags settled: what a table may print, how wide it may be,
/// and whether any of it is coloured.
///
/// Resolved once from the command line and the environment, so every table,
/// block and diff of one run agrees — see [`super::init`].
#[derive(Debug, Clone, Default)]
pub struct View {
    pub color: bool,
    /// Print cells whole, and every column with them.
    pub no_trunc: bool,
    /// Print the first column of each row and nothing else.
    pub quiet: bool,
    /// Never drop a column to fit.
    pub wide: bool,
    /// Exactly these columns, by header, in this order.
    pub columns: Vec<String>,
    /// The terminal's width, or `None` where there is no terminal to fit.
    pub width: Option<usize>,
    /// Page long output through `$PAGER`.
    pub pager: bool,
}

impl View {
    /// A view that fits everything and colours nothing: what a unit test
    /// renders against, and what the CLI falls back to before `init`.
    #[cfg(test)]
    pub fn plain() -> Self {
        Self::default()
    }

    /// The same, with a terminal of `width` columns behind it.
    #[cfg(test)]
    pub fn at(width: usize) -> Self {
        Self {
            width: Some(width),
            ..Self::default()
        }
    }
}

/// The table as one string: the uppercase header, then one line per row.
///
/// Fails only on a `--columns` naming a column the table does not have —
/// which is a typo worth refusing, since silently printing a different table
/// is worse than saying so.
pub fn render_table(columns: &[Column], rows: &[Vec<String>], view: &View) -> Result<String> {
    let picked = pick(columns, &view.columns)?;
    let mut cells = cells(columns, rows, &picked, view);
    let widths = fit(columns, &cells, &picked, view);
    for row in &mut cells {
        for (cell, width) in row.iter_mut().zip(&widths) {
            cell.text = cut(&cell.text, *width);
        }
    }

    let header: Vec<Painted> = picked
        .iter()
        .map(|i| Painted {
            text: columns[*i].header.to_uppercase(),
            style: Style::new(),
        })
        .collect();
    let mut out = vec![line(&header, &widths, view.color)];
    out.extend(cells.iter().map(|row| line(row, &widths, view.color)));
    Ok(out.join("\n"))
}

/// A cell as it will be printed: the text, and what colours it.
struct Painted {
    text: String,
    style: Style,
}

/// The columns to print: the ones `--columns` names, in the order it names
/// them, else every column the table declares.
fn pick(columns: &[Column], wanted: &[String]) -> Result<Vec<usize>> {
    if wanted.is_empty() {
        return Ok((0..columns.len()).collect());
    }
    wanted
        .iter()
        .map(|name| {
            columns
                .iter()
                .position(|c| c.header.eq_ignore_ascii_case(name.trim()))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "no column \"{name}\" here — this table has: {}",
                        columns
                            .iter()
                            .map(|c| c.header)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
        })
        .collect()
}

/// Every row's cells, flattened to one line, decorated and cut to their
/// column's own cap — the width the terminal imposes comes later.
fn cells(
    columns: &[Column],
    rows: &[Vec<String>],
    picked: &[usize],
    view: &View,
) -> Vec<Vec<Painted>> {
    rows.iter()
        .map(|row| {
            picked
                .iter()
                .map(|i| {
                    let column = columns[*i];
                    let raw = row.get(*i).map_or("", String::as_str);
                    let (style, glyph) = match column.cell {
                        Cell::Id => (style::ID, None),
                        Cell::Title => (style::TITLE, None),
                        Cell::Status => style::status(raw),
                        Cell::Attention => style::attention(raw),
                        Cell::Plain => (Style::new(), None),
                    };
                    let text = match glyph {
                        Some(glyph) => format!("{glyph} {}", flatten(raw)),
                        None => flatten(raw),
                    };
                    let cap = match view.no_trunc {
                        true => UNCAPPED,
                        false => column.cap,
                    };
                    Painted {
                        text: cut(&text, cap),
                        style,
                    }
                })
                .collect()
        })
        .collect()
}

/// The width of every printed column: what the content asks for, narrowed
/// until the row fits the terminal.
///
/// Nothing is narrowed when there is no terminal to fit (a pipe gets the whole
/// table), when `-o wide` or `--columns` asked for these columns by name, or
/// when `--no-trunc` asked for the cells whole.
fn fit(columns: &[Column], cells: &[Vec<Painted>], picked: &[usize], view: &View) -> Vec<usize> {
    let mut widths: Vec<usize> = picked
        .iter()
        .enumerate()
        .map(|(printed, i)| {
            let header = width(columns[*i].header);
            cells
                .iter()
                .filter_map(|row| row.get(printed))
                .map(|cell| width(&cell.text))
                .fold(header, usize::max)
        })
        .collect();

    let Some(terminal) = view.width else {
        return widths;
    };
    if view.wide || view.no_trunc || !view.columns.is_empty() {
        return widths;
    }
    // Only the cells the caller allowed to be cut may be squeezed: an id is
    // there to be copied, and half of one is worth nothing.
    let squeezable = |i: usize| columns[picked[i]].cap != UNCAPPED;
    let mut kept: Vec<bool> = vec![true; widths.len()];

    while total(&widths, &kept) > terminal {
        let dropped = kept
            .iter()
            .enumerate()
            .filter(|(i, keeping)| **keeping && columns[picked[*i]].rank != KEEP)
            .min_by_key(|(i, _)| (columns[picked[*i]].rank, std::cmp::Reverse(*i)))
            .map(|(i, _)| i);
        match dropped {
            Some(i) => kept[i] = false,
            None => break,
        }
    }
    // Dropping columns is coarse; the overflow that is left comes out of the
    // widest cell that may be cut, one character at a time, down to a width
    // there is no point going under.
    while total(&widths, &kept) > terminal {
        let widest = kept
            .iter()
            .enumerate()
            .filter(|(i, keeping)| **keeping && squeezable(*i) && widths[*i] > MIN_WIDTH)
            .max_by_key(|(i, _)| widths[*i])
            .map(|(i, _)| i);
        match widest {
            Some(i) => widths[i] -= 1,
            None => break,
        }
    }

    for (width, keeping) in widths.iter_mut().zip(&kept) {
        if !keeping {
            *width = 0;
        }
    }
    widths
}

/// What the row costs the terminal: the columns still standing, plus the gaps
/// between them.
fn total(widths: &[usize], kept: &[bool]) -> usize {
    let printed: Vec<usize> = widths
        .iter()
        .zip(kept)
        .filter(|(_, keeping)| **keeping)
        .map(|(w, _)| *w)
        .collect();
    printed.iter().sum::<usize>() + GAP * printed.len().saturating_sub(1)
}

/// One line: each cell in its colour, padded to its column — the padding
/// outside the escapes, so a styled cell is as wide as the word in it.
///
/// A column narrowed to nothing was dropped and prints nothing at all, gap
/// included.
fn line(cells: &[Painted], widths: &[usize], color: bool) -> String {
    let printed: Vec<(&Painted, usize)> = cells
        .iter()
        .zip(widths)
        .filter(|(_, width)| **width > 0)
        .map(|(cell, width)| (cell, *width))
        .collect();
    let mut out = String::new();
    for (i, (cell, width)) in printed.iter().enumerate() {
        if i > 0 {
            out.push_str(&" ".repeat(GAP));
        }
        out.push_str(&style::paint(color, cell.style, &cell.text));
        // The last column needs no padding, and padding it would leave every
        // row trailing blanks.
        if i + 1 < printed.len() {
            out.extend(std::iter::repeat_n(
                ' ',
                width.saturating_sub(super::width(&cell.text)),
            ));
        }
    }
    out
}

/// A cell as one line: a newline in it would tear the table apart, so control
/// characters become spaces whether or not the cell is cut.
fn flatten(cell: &str) -> String {
    cell.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// `text` at no more than `cap` characters ([`UNCAPPED`] for no limit), the
/// last one `…` when it had to be cut.
fn cut(text: &str, cap: usize) -> String {
    if cap == UNCAPPED || width(text) <= cap {
        return text.to_string();
    }
    let mut out: String = text.chars().take(cap.saturating_sub(1)).collect();
    out.push(ELLIPSIS);
    out
}

/// The first cell of every row, one per line: `-q`, which is there so a list
/// can be piped into the next command.
pub fn quiet_lines(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| row.first().cloned().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `task ls`-shaped table: two columns that stay, and four that go in
    /// rank order when the terminal is not wide enough.
    const COLS: &[Column] = &[
        col("id", UNCAPPED).id(),
        col("title", 48).title(),
        col("status", UNCAPPED).status(),
        col("age", UNCAPPED).rank(4),
        col("round", UNCAPPED).rank(3),
        col("tokens", UNCAPPED).rank(1),
        col("branch", 40).rank(0),
    ];

    fn row(title: &str) -> Vec<String> {
        vec![
            "01M0R9EPJK7QYAGYCN31E8EF58".into(),
            title.into(),
            "in_progress".into(),
            "3h".into(),
            "0".into(),
            "↑1.2M 89% ↓45k".into(),
            "add-the-frobnicator-01m0r9epjk".into(),
        ]
    }

    fn headers(table: &str) -> Vec<String> {
        table
            .lines()
            .next()
            .expect("a header")
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }

    fn render(rows: &[Vec<String>], view: &View) -> String {
        render_table(COLS, rows, view).expect("render")
    }

    /// Nothing is dropped when nothing has to be: a pipe has no width to fit
    /// and gets the whole table.
    #[test]
    fn a_table_with_no_terminal_behind_it_keeps_every_column() {
        let table = render(&[row("Add the frobnicator")], &View::plain());
        assert_eq!(
            headers(&table),
            ["ID", "TITLE", "STATUS", "AGE", "ROUND", "TOKENS", "BRANCH"]
        );
    }

    /// The least important column goes first, and the two that carry what the
    /// row is about never go.
    #[test]
    fn columns_are_dropped_by_rank_until_the_row_fits() {
        let rows = [row("Add the frobnicator")];
        assert_eq!(
            headers(&render(&rows, &View::at(120))),
            ["ID", "TITLE", "STATUS", "AGE", "ROUND", "TOKENS"],
            "the branch is the first to go"
        );
        assert_eq!(
            headers(&render(&rows, &View::at(90))),
            ["ID", "TITLE", "STATUS", "AGE", "ROUND"],
            "then the spend"
        );
        assert_eq!(
            headers(&render(&rows, &View::at(70))),
            ["ID", "TITLE", "STATUS", "AGE"]
        );
        assert_eq!(
            headers(&render(&rows, &View::at(60))),
            ["ID", "TITLE", "STATUS"],
            "and past that only what the row is about is left"
        );
    }

    /// Every line of an 80-column table is at most 80 columns wide, whatever
    /// the titles are: what dropping cannot do, narrowing the title does.
    #[test]
    fn nothing_wraps_at_eighty_columns() {
        let rows = [
            row("A title that runs on and on and would push the status clean off the screen"),
            row("short"),
        ];
        let table = render(&rows, &View::at(80));
        for line in table.lines() {
            assert!(width(line) <= 80, "{} columns: {line:?}", width(line));
        }
        assert!(table.contains('…'), "the long title was cut: {table}");
    }

    /// `-o wide` is the answer to a dropped column: every column, whatever
    /// the terminal.
    #[test]
    fn wide_keeps_every_column_at_any_width() {
        let view = View {
            wide: true,
            ..View::at(80)
        };
        assert_eq!(
            headers(&render(&[row("Add the frobnicator")], &view)),
            ["ID", "TITLE", "STATUS", "AGE", "ROUND", "TOKENS", "BRANCH"]
        );
    }

    /// `--columns` is the other answer: exactly these, in this order, and a
    /// name the table does not have is a refusal rather than a smaller table.
    #[test]
    fn columns_names_what_is_printed_and_refuses_what_it_cannot() {
        let view = View {
            columns: vec!["status".into(), "id".into()],
            ..View::at(80)
        };
        let table = render(&[row("Add the frobnicator")], &view);
        assert_eq!(headers(&table), ["STATUS", "ID"]);

        let view = View {
            columns: vec!["colour".into()],
            ..View::plain()
        };
        let err = render_table(COLS, &[], &view).expect_err("no such column");
        assert!(err.to_string().contains("no column \"colour\""), "{err}");
        assert!(err.to_string().contains("branch"), "{err}");
    }

    /// `--no-trunc` prints the cells whole, so the columns come with them.
    #[test]
    fn no_trunc_keeps_the_whole_cell_and_every_column() {
        let long = "A title that runs on and on and would push the status clean off the screen";
        let view = View {
            no_trunc: true,
            ..View::at(80)
        };
        let table = render(&[row(long)], &view);
        assert!(table.contains(long), "{table}");
        assert_eq!(headers(&table).len(), COLS.len());
    }

    /// A status cell leads with its glyph, whether or not there is colour to
    /// go with it — that is what makes the table readable in a pipe.
    #[test]
    fn a_status_cell_carries_its_glyph_without_colour() {
        let table = render(&[row("Add the frobnicator")], &View::plain());
        assert!(table.contains("● in_progress"), "{table}");
        assert!(
            !table.contains('\u{1b}'),
            "no escapes with colour off: {table:?}"
        );
    }

    /// With colour on, the id is dim, the title bold and the status coloured
    /// — and the columns still line up, since the padding is counted on the
    /// word rather than on the escapes around it.
    #[test]
    fn colour_wraps_the_cells_and_leaves_the_columns_aligned() {
        let view = View {
            color: true,
            ..View::plain()
        };
        let table = render(&[row("Add the frobnicator"), row("short")], &view);
        assert!(table.contains(&style::paint(true, style::ID, "01M0R9EPJK7QYAGYCN31E8EF58")));
        assert!(table.contains(&style::paint(true, style::TITLE, "Add the frobnicator")));
        assert!(table.contains(&style::paint(
            true,
            style::status("in_progress").0,
            "● in_progress"
        )));

        let plain = render(&[row("Add the frobnicator"), row("short")], &View::plain());
        let visible = |line: &str| {
            let mut out = String::new();
            let mut escaped = false;
            for c in line.chars() {
                match (escaped, c) {
                    (false, '\u{1b}') => escaped = true,
                    (true, 'm') => escaped = false,
                    (true, _) => {}
                    (false, c) => out.push(c),
                }
            }
            out
        };
        let stripped: Vec<String> = table.lines().map(visible).collect();
        assert_eq!(stripped, plain.lines().collect::<Vec<_>>());
    }

    #[test]
    fn the_header_is_uppercase_and_a_row_never_trails_spaces() {
        let table = render(&[row("short")], &View::plain());
        for line in table.lines() {
            assert_eq!(line.trim_end(), line, "trailing space in {line:?}");
        }
        assert!(table.starts_with("ID  "), "{table}");
    }

    /// The arrows of a spend are three bytes each, and an accented title is
    /// two: a column padded by bytes would leave every row carrying one short
    /// of the row above.
    #[test]
    fn columns_stay_aligned_when_a_cell_is_not_ascii() {
        let rows = [
            vec![
                "01ID".into(),
                "Revisor Estrícto".into(),
                "merged".into(),
                "3h".into(),
                "0".into(),
                "↑1.2M 89% ↓45k".into(),
                "one".into(),
            ],
            vec![
                "01ID".into(),
                "plain".into(),
                "merged".into(),
                "3h".into(),
                "0".into(),
                "↑0 0% ↓0".into(),
                "two".into(),
            ],
        ];
        let table = render(&rows, &View::plain());
        let branch = |line: &str| {
            line.find("one")
                .or_else(|| line.find("two"))
                .map(|byte| width(&line[..byte]))
        };
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(branch(lines[1]), branch(lines[2]), "{table}");
    }

    /// Cutting counts characters, not bytes: an accented title would
    /// otherwise be cut short and pushed out of line.
    #[test]
    fn a_cell_is_cut_by_characters() {
        assert_eq!(cut("a title far too long to fit", 10), "a title f…");
        assert_eq!(cut("a title f", 10), "a title f");
        assert_eq!(cut("ááááááááá", 10), "ááááááááá");
        assert_eq!(cut("ááááááááááá", 10), "ááááááááá…");
        assert_eq!(cut("first second", UNCAPPED), "first second");
        assert_eq!(flatten("first\nsecond"), "first second");
    }

    /// `-q` is the whole point of a pipe: one id per line, nothing else.
    #[test]
    fn quiet_prints_one_id_per_line() {
        let rows = [row("Add the frobnicator"), row("short")];
        assert_eq!(
            quiet_lines(&rows),
            "01M0R9EPJK7QYAGYCN31E8EF58\n01M0R9EPJK7QYAGYCN31E8EF58"
        );
        assert_eq!(quiet_lines(&[]), "");
    }
}
