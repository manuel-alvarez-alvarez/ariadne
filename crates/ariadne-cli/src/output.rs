//! Output helpers: aligned tables for humans, raw JSON with `--format json`.
//!
//! Two rules the whole CLI follows. Tables are for eyes: cells are one line,
//! long ones are cut to the column's cap with `…` (docker-style, `--no-trunc`
//! restores them), columns the terminal has no room for are dropped (`-o
//! wide` keeps them), statuses are coloured and carry a glyph, and timestamps
//! read as local time. `--format json` is for scripts: it is the daemon's own
//! payload, RFC3339 timestamps and all — and so is `-q`, which prints one id
//! per line and nothing else.
//!
//! Anything that is not the payload — "no tasks yet", a confirmation prompt —
//! goes to stderr, so stdout stays exactly what a pipe wants.
//!
//! How much of that happens is one decision, taken once from the command line
//! and the environment ([`init`]) and read back by every renderer ([`view`]).

pub mod pager;
pub mod style;
pub mod table;

use std::io::Write;
use std::sync::OnceLock;

use ariadne_api::usage::TokenUsageDto;
use serde::Serialize;

pub use style::ColorChoice;
pub use table::{Column, UNCAPPED, View, col, render_table};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    Table,
    Json,
}

/// How this run renders, settled before the first line is printed.
static VIEW: OnceLock<View> = OnceLock::new();

/// Settle how this run renders. Called once, from `main`, before any command
/// prints: everything after it reads the answer rather than asking the
/// environment again, so one run cannot colour half its output.
pub fn init(view: View) {
    let _ = VIEW.set(view);
}

/// How this run renders — the plainest possible view where nothing was
/// settled, which is what a unit test and a fallback both want.
pub fn view() -> &'static View {
    VIEW.get_or_init(View::default)
}

/// How wide the terminal is, or `None` when there is no terminal to fit:
/// output into a pipe or a file is not laid out for a screen that is not
/// there, and gets every column.
pub fn terminal_width() -> Option<usize> {
    let stdout = std::io::stdout();
    fitted_width(
        std::io::IsTerminal::is_terminal(&stdout),
        std::env::var("COLUMNS").ok().as_deref(),
        || {
            rustix::termios::tcgetwinsize(&stdout)
                .ok()
                .map(|size| usize::from(size.ws_col))
        },
    )
}

/// The width a table is laid out for, from the three things that decide it.
///
/// A terminal or nothing: whether there is a screen is the first question,
/// and a `COLUMNS` a shell happens to export is not a screen. Asking it the
/// other way round is how `ariadne task ls | cat` from an interactive shell
/// ends up cut to that shell's width — output a script is reading, quietly
/// short of columns nobody asked to drop.
///
/// With a terminal, `COLUMNS` wins over the ioctl: it is how a width is
/// forced for a screenshot or a demo, and how a shell reports a width the
/// kernel has not caught up with. Anything unreadable or zero falls through
/// to what the terminal itself says.
fn fitted_width(
    is_terminal: bool,
    columns: Option<&str>,
    ioctl: impl FnOnce() -> Option<usize>,
) -> Option<usize> {
    if !is_terminal {
        return None;
    }
    let readable = |columns: usize| (columns > 0).then_some(columns);
    columns
        .and_then(|columns| columns.trim().parse().ok())
        .and_then(readable)
        .or_else(|| ioctl().and_then(readable))
}

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
pub fn print<T: Serialize>(
    format: Format,
    payload: &T,
    table: impl FnOnce(),
) -> anyhow::Result<()> {
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
///
/// `-q` is the third answer, and the one a pipe wants: the first cell of every
/// row — the id — and nothing else, header included.
pub fn print_list<T: Serialize>(
    format: Format,
    items: &[T],
    columns: &[Column],
    row: impl Fn(&T) -> Vec<String>,
    empty: &str,
) -> anyhow::Result<()> {
    if let Format::Json = format {
        return print_json(&items);
    }
    let rows: Vec<Vec<String>> = items.iter().map(row).collect();
    match view().quiet {
        true if !rows.is_empty() => println!("{}", table::quiet_lines(&rows)),
        true => {}
        false => print_table(columns, &rows)?,
    }
    if items.is_empty() && !empty.is_empty() {
        note(empty);
    }
    Ok(())
}

/// Print rows as an aligned table with an uppercase header, laid out for this
/// terminal.
pub fn print_table(columns: &[Column], rows: &[Vec<String>]) -> anyhow::Result<()> {
    println!("{}", render_table(columns, rows, view())?);
    Ok(())
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

/// An id as every table's tail and the whole web UI print it: `…last8`.
///
/// Ids are 26-character ULIDs — unreadable in full, and the tail is enough to
/// tell two of them apart. Anything too short to shorten is left alone.
pub fn short_id(id: &str) -> String {
    match id.char_indices().nth_back(7) {
        Some((i, _)) if id.len() > 10 => format!("…{}", &id[i..]),
        _ => id.to_string(),
    }
}

/// An optional timestamp as an inspect block spells it, or a dash.
pub fn at(rfc3339: Option<&str>) -> String {
    rfc3339.map_or_else(|| "-".to_string(), moment)
}

/// A timestamp as an inspect block spells it: when it was, and how long ago
/// that was — `2026-08-29 01:34:33 (3h ago)`.
///
/// Both, because a block is read for either: the absolute time to line an
/// event up against a log, the age to see at a glance that it is stale. A
/// table has no room for both and carries the age alone.
pub fn moment(rfc3339: &str) -> String {
    let absolute = local_time(rfc3339);
    match relative(rfc3339, chrono::Utc::now()) {
        Some(age) => format!("{absolute} ({age} ago)"),
        None => absolute,
    }
}

/// How long ago that was, as a table cell: `12s`, `4m`, `3h`, `2d` — floored,
/// never rounded, so 89 seconds is "1m" and not the "2m" rounding would jump
/// to a second early. Anything unparseable is passed through, as
/// [`local_time`].
pub fn age(rfc3339: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    relative(rfc3339, now).unwrap_or_else(|| rfc3339.to_string())
}

/// The same, and `None` for a timestamp that is not one.
fn relative(rfc3339: &str, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
    let then = chrono::DateTime::parse_from_rfc3339(rfc3339).ok()?;
    let seconds = (now - then.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0);
    Some(match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    })
}

/// A span of seconds as a person reads it: `12s`, `4m 20s`, `3h 12m`,
/// `2d 3h`. Two units at most — a daemon that has been up for two days and
/// three hours has not been up for `2d 3h 7m 12s`.
pub fn duration(seconds: u64) -> String {
    let (days, hours, minutes, secs) = (
        seconds / 86_400,
        (seconds % 86_400) / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60,
    );
    match (days, hours, minutes) {
        (0, 0, 0) => format!("{secs}s"),
        (0, 0, _) => format!("{minutes}m {secs}s"),
        (0, _, _) => format!("{hours}h {minutes}m"),
        _ => format!("{days}d {hours}h"),
    }
}

/// A token count as a person reads it: the digits under a thousand, then
/// three figures at most in the largest unit that fits — `950`, `1.2k`,
/// `45k`, `1.2M`, `12M`, `1.2G`, `45G`, `1.5T`. `T` is where the units run
/// out, so a count past it is as long as it needs to be: `1234T`, and
/// `18446744T` for the largest a `u64` holds.
///
/// This is how a count is written everywhere the CLI prints one, block as
/// well as table: the digits past the first few are noise, and what a reader
/// wants from a spend is its order of magnitude, side by side with the row
/// above. The same rule as the web's `formatTokens`, character for character,
/// so a count reads the same in a terminal and on a screen.
///
/// The bands are cut where the rounding lands rather than where the unit
/// does, so a count that rounds up out of its band is spelled by the band
/// above it: 9_950 is `10k`, the way ten thousand itself is written, never
/// `10.0k`, and 999_500 is `1M` rather than `1000k`. The one place the three
/// figures give way is above `T`, which has no band above it to carry into:
/// there is no larger unit a reader would place, so the count keeps counting
/// in whole `T` however many digits that takes.
pub fn tokens(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 9_950 {
        tenths(count, 1_000, 'k')
    } else if count < 999_500 {
        whole(count, 1_000, 'k')
    } else if count < 9_950_000 {
        tenths(count, 1_000_000, 'M')
    } else if count < 999_500_000 {
        whole(count, 1_000_000, 'M')
    } else if count < 9_950_000_000 {
        tenths(count, 1_000_000_000, 'G')
    } else if count < 999_500_000_000 {
        whole(count, 1_000_000_000, 'G')
    } else if count < 9_950_000_000_000 {
        tenths(count, 1_000_000_000_000, 'T')
    } else {
        whole(count, 1_000_000_000_000, 'T')
    }
}

/// How much of what went in the prompt cache served, as a whole percent:
/// `92%`.
///
/// `0%` where nothing went in at all — a spend of nothing served nothing, and
/// a dash there would read as a figure the daemon does not have — and never
/// past `100%`, whatever an agent's own transcript says about a cache that
/// served more than the prompt it was serving. The same rule
/// `ui/src/lib/format.ts` renders, so the share reads the same in a terminal
/// and on a screen.
pub fn cached_share(usage: &TokenUsageDto) -> String {
    let share = match usage.input_tokens {
        0 => 0,
        input => {
            let (cached, input) = (u128::from(usage.cached_input_tokens), u128::from(input));
            ((cached * 100 + input / 2) / input).min(100)
        }
    };
    format!("{share}%")
}

/// A spend as a table cell: what went in under an up arrow, how much of it
/// the cache served, and what came out under a down one.
///
/// The share rides on the input it is a share of rather than sitting at the
/// end, because that is the half it qualifies: a column is read downwards,
/// and `↑1.2M 92%` is one figure to hold against the row above. The exact
/// counts are left to [`usage_block`] — a cell is for comparing rows, not
/// for reading one.
pub fn usage_cell(usage: &TokenUsageDto) -> String {
    format!(
        "↑{} {} ↓{}",
        tokens(usage.input_tokens),
        cached_share(usage),
        tokens(usage.output_tokens)
    )
}

/// What one agent, task or goal spent, as an inspect block: the total, and
/// then who spent it.
///
/// The counts are the same compact figures a cell carries — nobody reads a
/// spend to the digit, and seven of them in a column is a wall to count
/// zeroes in. The two parts of the total are labelled rather than spelled
/// with arrows, since there is no column here to keep narrow, and their
/// counts are right-aligned so they can be read against each other. The
/// cached share rides beside the `input` it is a share of, the way the table
/// cell carries it, rather than on a line of its own that would read as a
/// third figure to add up.
///
/// `breakdown` is who spent it — a goal's roles, a task's agents — each named
/// and carrying that same pair exactly; `session inspect` has nobody to break
/// down and passes none. `indent` is where a continuation line starts:
/// [`print_kv`] pads its keys, and every line after the first lines up under
/// the first.
pub fn usage_block(
    total: &TokenUsageDto,
    breakdown: &[(String, TokenUsageDto)],
    indent: &str,
) -> String {
    let counts = [
        (
            "input",
            tokens(total.input_tokens),
            Some(cached_share(total)),
        ),
        ("output", tokens(total.output_tokens), None),
    ];
    let label = counts
        .iter()
        .map(|(name, ..)| name.len())
        .max()
        .unwrap_or(0);
    // A compact count is digits, a dot and a unit, so bytes and characters
    // agree.
    let digits = counts.iter().map(|(_, c, _)| c.len()).max().unwrap_or(0);
    let mut lines: Vec<String> = counts
        .iter()
        .map(|(name, count, share)| match share {
            Some(share) => format!("{name:<label$}  {count:>digits$}  {share}"),
            None => format!("{name:<label$}  {count:>digits$}"),
        })
        .collect();

    let width = breakdown
        .iter()
        .map(|(name, _)| name.chars().count())
        .max()
        .unwrap_or(0);
    lines.extend(breakdown.iter().map(|(name, usage)| {
        format!(
            "{name:<width$}  ↑{} ↓{}",
            tokens(usage.input_tokens),
            tokens(usage.output_tokens)
        )
    }));
    lines.join(indent)
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

/// A note that something looks wrong, in the same place and the same colour
/// wherever it is said: stderr, so it never lands in what a pipe is reading.
pub fn warn(message: &str) {
    note(&style::paint(view().color, style::WARN, message));
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

/// Column width in characters — `str::len` counts bytes, and a title with an
/// accent in it would push its column one space out of line per byte.
fn width(cell: &str) -> usize {
    cell.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three figures at most while a unit is left to carry into, and the
    /// digits themselves while they still mean something: a table is read
    /// down a column, and 12_345 next to 1_234_567 says nothing that `12k`
    /// next to `1.2M` does not.
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
        assert_eq!(tokens(999_499_999), "999M");
        assert_eq!(tokens(999_500_000), "1G");
        assert_eq!(tokens(1_234_000_000), "1.2G");
        assert_eq!(tokens(9_949_999_999), "9.9G");
        assert_eq!(tokens(9_950_000_000), "10G");
        assert_eq!(tokens(45_000_000_000), "45G");
        assert_eq!(tokens(999_499_999_999), "999G");
        assert_eq!(tokens(999_500_000_000), "1T");
        assert_eq!(tokens(1_500_000_000_000), "1.5T");
        assert_eq!(tokens(9_950_000_000_000), "10T");
        assert_eq!(tokens(1_234_000_000_000_000), "1234T");
    }

    /// `T` is the last band, so the largest count there is stays a figure
    /// rather than an overflow: the rounding is done wide enough to hold it.
    #[test]
    fn the_largest_count_a_u64_holds_is_still_whole_teras() {
        assert_eq!(tokens(u64::MAX), "18446744T");
    }

    fn usage(input: u64, cached: u64, output: u64) -> TokenUsageDto {
        TokenUsageDto {
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
        }
    }

    /// Nothing spent is `0` spent: a blank or a dash in either of these would
    /// read as a figure the daemon does not have.
    ///
    /// A share of nothing is `0%` and not a division by zero: a session that
    /// has not sent a prompt yet has sent nothing the cache could serve.
    #[test]
    fn a_spend_of_nothing_is_zeros_and_not_a_dash() {
        assert_eq!(cached_share(&TokenUsageDto::default()), "0%");
        assert_eq!(usage_cell(&TokenUsageDto::default()), "↑0 0% ↓0");
        assert_eq!(
            usage_block(&TokenUsageDto::default(), &[], "\n"),
            ["input   0  0%", "output  0"].join("\n")
        );
    }

    /// A whole percent, rounded half away from zero the way every other
    /// figure here is, and never past `100%`: an agent that reports a cache
    /// serving more than the prompt it served is reporting `100%`, not a
    /// share no reader could place.
    #[test]
    fn the_cached_share_is_a_whole_percent_between_zero_and_a_hundred() {
        assert_eq!(cached_share(&usage(8, 3, 0)), "38%");
        assert_eq!(cached_share(&usage(3, 1, 0)), "33%");
        assert_eq!(cached_share(&usage(1_234_567, 1_100_000, 0)), "89%");
        assert_eq!(cached_share(&usage(100, 0, 0)), "0%");
        assert_eq!(cached_share(&usage(100, 100, 0)), "100%");
        assert_eq!(cached_share(&usage(100, 1_000, 0)), "100%");
    }

    /// The cell is the three figures a row is scanned by; the block is the
    /// same figures labelled and stacked, with the share beside the input it
    /// is a share of.
    #[test]
    fn a_spend_reads_as_arrows_in_a_cell_and_as_labelled_counts_in_a_block() {
        let spent = usage(1_234_567, 1_100_000, 45_300);
        assert_eq!(usage_cell(&spent), "↑1.2M 89% ↓45k");
        assert_eq!(
            usage_block(
                &spent,
                &[
                    ("engineer".into(), spent),
                    ("Reviewer".into(), usage(4_600, 0, 300)),
                ],
                "\n",
            ),
            [
                "input   1.2M  89%",
                "output   45k",
                "engineer  ↑1.2M ↓45k",
                "Reviewer  ↑4.6k ↓300",
            ]
            .join("\n")
        );
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

    /// A pipe gets every column, `COLUMNS` or no `COLUMNS`.
    ///
    /// An interactive shell may export its own width, and `ariadne task ls |
    /// wc -l` inherits it; reading it there would cut the output a script is
    /// parsing down to the width of a screen that is not in the picture. Only
    /// a terminal has a width worth fitting.
    #[test]
    fn there_is_no_width_to_fit_without_a_terminal() {
        let no_ioctl = || None;
        assert_eq!(fitted_width(false, Some("80"), no_ioctl), None);
        assert_eq!(fitted_width(false, None, || Some(120)), None);
        assert_eq!(fitted_width(false, Some("80"), || Some(120)), None);
    }

    /// With a terminal: `COLUMNS` if it says something, else what the
    /// terminal itself reports — and a value that is neither a number nor a
    /// width falls through to it rather than turning fitting off.
    #[test]
    fn a_terminal_is_measured_by_columns_then_by_the_ioctl() {
        let ioctl = || Some(120);
        assert_eq!(fitted_width(true, Some("80"), ioctl), Some(80));
        assert_eq!(fitted_width(true, Some(" 80 "), ioctl), Some(80));
        assert_eq!(fitted_width(true, None, ioctl), Some(120));
        assert_eq!(fitted_width(true, Some(""), ioctl), Some(120));
        assert_eq!(fitted_width(true, Some("wide"), ioctl), Some(120));
        assert_eq!(fitted_width(true, Some("0"), ioctl), Some(120));
        assert_eq!(fitted_width(true, Some("0"), || Some(0)), None);
        assert_eq!(fitted_width(true, None, || None), None);
    }

    /// Floored at every unit, clamped at zero for a stamp from the future
    /// (clock skew), passed through when unparseable — the same age
    /// `ariadne attention` has always shown, now in every `ls`.
    #[test]
    fn an_age_is_compact_and_floored() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-18T12:00:00Z")
            .expect("parse")
            .with_timezone(&chrono::Utc);
        let at = |s: &str| age(s, now);
        assert_eq!(at("2026-08-18T11:59:49Z"), "11s");
        assert_eq!(at("2026-08-18T11:58:31Z"), "1m");
        assert_eq!(at("2026-08-18T09:00:01Z"), "2h");
        assert_eq!(at("2026-08-15T12:00:00Z"), "3d");
        assert_eq!(at("2026-08-18T12:00:05Z"), "0s");
        assert_eq!(at("not a time"), "not a time");
    }

    /// An inspect block carries both halves: when, and how long ago — and
    /// only the first when the stamp is not one we can read.
    #[test]
    fn a_moment_is_the_time_and_the_age_together() {
        let rendered = moment("2026-08-17T00:08:50.415Z");
        assert!(
            rendered.starts_with(&local_time("2026-08-17T00:08:50.415Z")),
            "{rendered}"
        );
        assert!(rendered.ends_with(" ago)"), "{rendered}");
        assert_eq!(moment("not a time"), "not a time");
        assert_eq!(at(None), "-");
    }

    /// Two units at most, and seconds only while they are the whole story:
    /// `uptime: 2902s` is a number to divide, `48m 22s` is an answer.
    #[test]
    fn a_duration_reads_as_two_units_at_most() {
        assert_eq!(duration(0), "0s");
        assert_eq!(duration(45), "45s");
        assert_eq!(duration(2_902), "48m 22s");
        assert_eq!(duration(3_600), "1h 0m");
        assert_eq!(duration(11_520), "3h 12m");
        assert_eq!(duration(86_400), "1d 0h");
        assert_eq!(duration(183_600), "2d 3h");
    }
}
