//! Output helpers: aligned tables for humans, raw JSON with `--format json`.
//!
//! Two rules the whole CLI follows. Tables are for eyes: cells are one line,
//! long ones are cut to the column's cap with `…` (docker-style, `--no-trunc`
//! restores them), and timestamps read as local time. `--format json` is for
//! scripts: it is the daemon's own payload, RFC3339 timestamps and all.
//!
//! Anything that is not the payload — "no tasks yet", a confirmation prompt —
//! goes to stderr, so stdout stays exactly what a pipe wants.

use std::io::Write;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    Table,
    Json,
}

/// A table column: its header, and the width its cells are cut to
/// ([`UNCAPPED`] for the ones that must stay whole, like ids).
pub type Column = (&'static str, usize);

/// Width for a column that is never truncated.
pub const UNCAPPED: usize = 0;

/// The ellipsis a cut cell ends with.
const ELLIPSIS: char = '…';

/// Print any serializable value as pretty JSON.
pub fn print_json<T: Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Print rows as an aligned table with an uppercase header.
pub fn print_table(columns: &[Column], rows: &[Vec<String>], no_trunc: bool) {
    for line in table_lines(columns, rows, no_trunc) {
        println!("{line}");
    }
}

/// The table as lines: header first, then one line per row.
///
/// Cells are truncated to their column's cap unless `no_trunc` is set, and
/// always flattened to a single line: a multi-line cell would tear the table
/// apart, so its newlines become spaces whether or not it is cut.
fn table_lines(columns: &[Column], rows: &[Vec<String>], no_trunc: bool) -> Vec<String> {
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(i, cell)| {
                    let cap = columns.get(i).map_or(UNCAPPED, |c| c.1);
                    fit(cell, if no_trunc { UNCAPPED } else { cap })
                })
                .collect()
        })
        .collect();

    let mut widths: Vec<usize> = columns.iter().map(|(h, _)| width(h)).collect();
    for row in &cells {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(width(cell));
            }
        }
    }
    let line = |cells: Vec<String>| {
        let mut out = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i > 0 {
                out.push_str("   ");
            }
            out.push_str(cell);
            // The last column needs no padding, and padding it would leave
            // every row trailing blanks.
            if i + 1 < cells.len() {
                let pad = widths.get(i).copied().unwrap_or(0);
                out.extend(std::iter::repeat_n(' ', pad.saturating_sub(width(cell))));
            }
        }
        out
    };

    let mut out = vec![line(
        columns.iter().map(|(h, _)| h.to_uppercase()).collect(),
    )];
    out.extend(cells.into_iter().map(line));
    out
}

/// Print a key/value inspect block.
pub fn print_kv(pairs: &[(&str, String)]) {
    let width = pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (k, v) in pairs {
        println!("{k:<width$}  {v}");
    }
}

/// A word to the person at the terminal — "no tasks yet", "aborted" — never
/// part of the output a script is reading. Always stderr, so `ls | wc -l`
/// counts rows and nothing else.
pub fn note(message: &str) {
    let _ = writeln!(std::io::stderr(), "{message}");
}

/// An RFC3339 timestamp as local time, for human output. `--format json`
/// keeps the daemon's RFC3339 spelling; this is the table/inspect one.
///
/// Anything unparseable is passed through: a timestamp we cannot read is
/// still better shown than swallowed.
pub fn local_time(rfc3339: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(rfc3339) {
        Ok(t) => t
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        Err(_) => rfc3339.to_string(),
    }
}

/// A cell as a table can print it: one line, at most `cap` characters
/// (`cap` = [`UNCAPPED`] for no limit), the last one `…` when cut.
fn fit(cell: &str, cap: usize) -> String {
    let flat: String = cell
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if cap == UNCAPPED || flat.chars().count() <= cap {
        return flat;
    }
    let mut out: String = flat.chars().take(cap.saturating_sub(1)).collect();
    out.push(ELLIPSIS);
    out
}

/// Column width in characters — `str::len` counts bytes, and a title with an
/// accent in it would push its column one space out of line per byte.
fn width(cell: &str) -> usize {
    cell.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLS: &[Column] = &[("id", UNCAPPED), ("title", 10), ("status", UNCAPPED)];

    fn row(id: &str, title: &str, status: &str) -> Vec<String> {
        vec![id.into(), title.into(), status.into()]
    }

    #[test]
    fn a_long_cell_is_cut_to_the_cap_with_an_ellipsis() {
        assert_eq!(fit("a title far too long to fit", 10), "a title f…");
        assert_eq!(fit("a title f", 10), "a title f");
    }

    /// Cutting on bytes would both cut an accented title short and risk
    /// splitting a character in half.
    #[test]
    fn a_cell_is_cut_by_characters_not_bytes() {
        assert_eq!(fit("ááááááááá", 10), "ááááááááá");
        assert_eq!(fit("ááááááááááá", 10), "ááááááááá…");
    }

    #[test]
    fn a_multiline_cell_stays_on_one_line() {
        assert_eq!(fit("first\nsecond", UNCAPPED), "first second");
    }

    #[test]
    fn the_header_is_uppercase() {
        assert_eq!(table_lines(COLS, &[], false), ["ID   TITLE   STATUS"]);
    }

    #[test]
    fn no_trunc_keeps_the_whole_cell() {
        let rows = [row("id1", "a title far too long to fit", "ready")];
        let out = table_lines(COLS, &rows, true);
        assert!(out[1].contains("a title far too long to fit"), "{out:?}");
    }

    #[test]
    fn columns_stay_aligned_when_a_cell_is_cut() {
        let rows = [
            row("id1", "a title far too long to fit", "ready"),
            row("identifier2", "short", "merged"),
        ];
        let out = table_lines(COLS, &rows, false);
        // In characters: `…` is three bytes, and a byte offset would call
        // these two lines misaligned when the terminal shows them level.
        let status = |line: &String| {
            line.find("ready")
                .or_else(|| line.find("merged"))
                .map(|byte| line[..byte].chars().count())
        };
        assert_eq!(status(&out[1]), status(&out[2]), "{out:?}");
        assert!(out[1].contains("a title f…"), "{out:?}");
    }

    #[test]
    fn a_row_never_ends_in_trailing_spaces() {
        let rows = [
            row("id1", "short", "ready"),
            row("identifier2", "s", "merged"),
        ];
        for line in table_lines(COLS, &rows, false) {
            assert_eq!(line.trim_end(), line, "trailing space in {line:?}");
        }
    }

    #[test]
    fn a_timestamp_that_is_not_rfc3339_is_printed_as_it_came() {
        assert_eq!(local_time("not a time"), "not a time");
    }

    /// Same instant, whatever the machine's zone: only the spelling changes.
    #[test]
    fn a_timestamp_is_rendered_as_local_time() {
        let rendered = local_time("2026-08-17T00:08:50.415Z");
        let expected = chrono::DateTime::parse_from_rfc3339("2026-08-17T00:08:50.415Z")
            .expect("parse")
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        assert_eq!(rendered, expected);
        assert!(!rendered.contains('T'), "{rendered}");
    }
}
