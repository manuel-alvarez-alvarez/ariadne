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

use ariadne_api::usage::TokenUsageDto;
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

/// What a command answers with: the daemon's own payload for a script, or
/// `table` for a person.
///
/// Nearly every command is this shape — fetch, then render one way or the
/// other — and writing the `match` out each time is how the two halves drift.
pub fn print<T: Serialize>(format: Format, payload: &T, table: impl FnOnce()) -> anyhow::Result<()> {
    match format {
        Format::Json => print_json(payload),
        Format::Table => {
            table();
            Ok(())
        }
    }
}

/// A listing: the payload as json, an aligned table otherwise — with `empty`
/// said to the terminal when no row came back, since an empty table on its own
/// does not say whether that is a filter or an empty system.
pub fn print_list<T: Serialize>(
    format: Format,
    items: &[T],
    columns: &[Column],
    no_trunc: bool,
    row: impl Fn(&T) -> Vec<String>,
    empty: &str,
) -> anyhow::Result<()> {
    print(format, &items, || {
        print_table(columns, &items.iter().map(row).collect::<Vec<_>>(), no_trunc);
        if items.is_empty() && !empty.is_empty() {
            note(empty);
        }
    })
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

/// An optional cell: the value, or a dash where there is none.
pub fn dash(value: Option<&str>) -> String {
    value.unwrap_or("-").to_string()
}

/// An optional timestamp, in local time, or a dash.
pub fn at(rfc3339: Option<&str>) -> String {
    rfc3339.map_or_else(|| "-".to_string(), local_time)
}

/// A token count as a person reads it: the digits under a thousand, then
/// three figures at most — `950`, `1.2k`, `45k`, `1.2M`, `12M`.
///
/// Counts run from zero to tens of millions, and the digits past the first
/// few are noise in a table: what a reader wants from them is the order of
/// magnitude, side by side with the row above. The same rule as the web's
/// `formatTokens`, character for character, so a count reads the same in a
/// terminal and on a screen.
///
/// The bands are cut where the rounding lands rather than where the unit
/// does, so a count that rounds up out of its band is spelled by the band
/// above it: 9_950 is `10k`, the way ten thousand itself is written, never
/// `10.0k`, and 999_500 is `1M` rather than `1000k`.
pub fn tokens(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 9_950 {
        tenths(count, 1_000, 'k')
    } else if count < 999_500 {
        whole(count, 1_000, 'k')
    } else if count < 9_950_000 {
        tenths(count, 1_000_000, 'M')
    } else {
        whole(count, 1_000_000, 'M')
    }
}

/// What one agent, task or goal spent, in a line: the cell, and then how much
/// of what went in the prompt cache served.
///
/// It opens on exactly what the table cell shows, so the figure a row was
/// scanned for is the figure the block starts with.
pub fn usage_summary(usage: &TokenUsageDto) -> String {
    format!(
        "{} ({} cached)",
        usage_cell(usage),
        tokens(usage.cached_input_tokens),
    )
}

/// The same spend as a table cell: what went in under an up arrow and what
/// came out under a down one, with the cache left to [`usage_summary`] — a
/// column is for comparing rows, not for reading one.
pub fn usage_cell(usage: &TokenUsageDto) -> String {
    format!(
        "↑{} ↓{}",
        tokens(usage.input_tokens),
        tokens(usage.output_tokens)
    )
}

/// `count` over `unit` to a tenth, with a trailing `.0` dropped: `1.2k`, `2k`.
fn tenths(count: u64, unit: u64, suffix: char) -> String {
    let tenths = rounded(count, unit / 10);
    match tenths % 10 {
        0 => format!("{}{suffix}", tenths / 10),
        frac => format!("{}.{frac}{suffix}", tenths / 10),
    }
}

/// `count` over `unit` as a whole number: `45k`, `12M`.
fn whole(count: u64, unit: u64, suffix: char) -> String {
    format!("{}{suffix}", rounded(count, unit))
}

/// `count` over `step`, rounded half away from zero — so the last digit shown
/// is the nearest one, and not the one that fell out of a float.
fn rounded(count: u64, step: u64) -> u128 {
    (u128::from(count) + u128::from(step) / 2) / u128::from(step)
}

/// A flag as a cell: `yes`, or `no_word` — `-` in a table, `no` in a block.
pub fn yes_no(flag: bool, no_word: &str) -> String {
    match flag {
        true => "yes".into(),
        false => no_word.into(),
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

    /// One line, at most `cap` characters — counted in characters, since
    /// cutting on bytes would both cut an accented title short and risk
    /// splitting a character in half.
    #[test]
    fn a_cell_is_flattened_to_one_line_and_cut_to_its_cap() {
        assert_eq!(fit("a title far too long to fit", 10), "a title f…");
        assert_eq!(fit("a title f", 10), "a title f");
        assert_eq!(fit("ááááááááá", 10), "ááááááááá");
        assert_eq!(fit("ááááááááááá", 10), "ááááááááá…");
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

    /// Three figures at most, and the digits themselves while they still mean
    /// something: a table is read down a column, and 12_345 next to
    /// 1_234_567 says nothing that `12k` next to `1.2M` does not.
    ///
    /// The same table the web's `formatTokens` is held to, count for count:
    /// a figure that reads one way in a terminal and another on a screen is
    /// two figures to whoever is comparing them.
    #[test]
    fn a_token_count_is_spelled_the_way_the_web_spells_it() {
        assert_eq!(tokens(0), "0");
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(999), "999");
        assert_eq!(tokens(1_000), "1k");
        assert_eq!(tokens(1_234), "1.2k");
        assert_eq!(tokens(1_950), "2k");
        assert_eq!(tokens(9_949), "9.9k");
        assert_eq!(tokens(9_950), "10k");
        assert_eq!(tokens(45_300), "45k");
        assert_eq!(tokens(516_000), "516k");
        assert_eq!(tokens(999_499), "999k");
        assert_eq!(tokens(999_500), "1M");
        assert_eq!(tokens(1_234_567), "1.2M");
        assert_eq!(tokens(1_950_000), "2M");
        assert_eq!(tokens(9_949_999), "9.9M");
        assert_eq!(tokens(9_950_000), "10M");
        assert_eq!(tokens(12_345_678), "12M");
    }

    /// Nothing spent is `0` spent: a blank or a dash in this line would read
    /// as a figure the daemon does not have.
    #[test]
    fn a_spend_of_nothing_is_zeros_and_not_a_dash() {
        assert_eq!(usage_cell(&TokenUsageDto::default()), "↑0 ↓0");
        assert_eq!(usage_summary(&TokenUsageDto::default()), "↑0 ↓0 (0 cached)");
    }

    /// The cell is what went in and what came out, the block line is that
    /// same pair plus the cache: one is read on its own, the other against
    /// the rows around it, and the second opens on the first.
    #[test]
    fn a_spend_reads_as_arrows_in_a_cell_and_carries_the_cache_in_a_block() {
        let usage = TokenUsageDto {
            input_tokens: 1_234_567,
            cached_input_tokens: 1_100_000,
            output_tokens: 45_300,
        };
        assert_eq!(usage_cell(&usage), "↑1.2M ↓45k");
        assert_eq!(usage_summary(&usage), "↑1.2M ↓45k (1.1M cached)");
    }

    /// The arrows are three bytes each, and a column padded by bytes would
    /// leave every row with a spend in it short of the one above.
    #[test]
    fn columns_stay_aligned_when_a_cell_carries_the_arrows() {
        let usage = |input, output| {
            usage_cell(&TokenUsageDto {
                input_tokens: input,
                cached_input_tokens: 0,
                output_tokens: output,
            })
        };
        let cols: &[Column] = &[("tokens", UNCAPPED), ("branch", UNCAPPED)];
        let rows = [
            vec![usage(1_234_567, 45_300), "one".into()],
            vec![usage(0, 0), "two".into()],
        ];
        let out = table_lines(cols, &rows, false);
        let branch = |line: &String| {
            line.find("one")
                .or_else(|| line.find("two"))
                .map(|byte| line[..byte].chars().count())
        };
        assert_eq!(branch(&out[1]), branch(&out[2]), "{out:?}");
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
