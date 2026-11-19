//! A list as one host answers it, held until it is printed.
//!
//! Each `ls` builds a [`Listing`] and prints it. `--host all` reads the same
//! list from each ready host and prints them as one list (spec 014): one
//! table fitted across every host and led by `HOST`, one summary line, and
//! one empty note. A host's own hint and incomplete-read note keep their
//! host's name.

use anyhow::Result;
use serde::Serialize;
use serde_json::{Map, Value};

use super::table::{Column, UNCAPPED, View, col, quiet_lines, render_table};
use super::{Format, note, view};

/// The column a list of every host leads with.
const HOST: Column = col("host", UNCAPPED);

/// One list, as one host answered it.
pub(crate) struct Listing {
    columns: &'static [Column],
    rows: Vec<Vec<String>>,
    /// The JSON payload, as the command prints it without `--host`.
    json: Value,
    /// The same payload in the field order of its type, which a `Value` loses.
    json_text: String,
    empty: String,
    count: Option<Count>,
    notes: Vec<Note>,
    section: Option<Section>,
    incomplete: Option<Incomplete>,
}

/// A summary line on stdout after the table: `3 of 17 sessions`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Count {
    pub(crate) shown: usize,
    pub(crate) total: usize,
    pub(crate) noun: &'static str,
}

/// A note on stderr after the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Note {
    /// How to read the list. A list of every host says it one time.
    Legend(String),
    /// The command that reads on from where the host stopped. A list of every
    /// host names the host, and adds `--host` to the command.
    Next(String),
}

/// A list that a person reads as bullets, not as a table: `attention`.
/// `--columns` still asks for the table.
struct Section {
    heading: &'static str,
    blocks: Vec<Block>,
}

/// One bullet of a [`Section`], and the indented lines under it.
pub(crate) struct Block {
    pub(crate) head: String,
    pub(crate) lines: Vec<Line>,
}

/// One indented line of a [`Block`]: `key: value`.
pub(crate) struct Line {
    key: &'static str,
    value: String,
    /// A command to type. A list of every host adds `--host` to it.
    command: bool,
}

impl Block {
    /// The block as it prints, on the list of `host` where one is given.
    #[cfg(test)]
    pub(crate) fn lines(&self, host: Option<&str>) -> Vec<String> {
        block_lines(self, host)
    }
}

impl Line {
    pub(crate) fn text(key: &'static str, value: impl Into<String>) -> Self {
        Self {
            key,
            value: value.into(),
            command: false,
        }
    }

    pub(crate) fn command(key: &'static str, value: impl Into<String>) -> Self {
        Self {
            command: true,
            ..Self::text(key, value)
        }
    }
}

/// A read that the daemon could not finish: the note a table gets, and the
/// failure of `-q`, which has no place for a note.
struct Incomplete {
    note: String,
    quiet_error: String,
}

/// One line of output, on the stream it goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Out {
    Stdout(String),
    Stderr(String),
}

/// What a list prints: its lines in order, and the failure it ends with.
#[derive(Debug, Default)]
struct Rendered {
    lines: Vec<Out>,
    error: Option<String>,
}

impl Listing {
    /// A list of `items`, one row each, and the note an empty list prints.
    pub(crate) fn of<T: Serialize>(
        items: &[T],
        columns: &'static [Column],
        row: impl Fn(&T) -> Vec<String>,
        empty: impl Into<String>,
    ) -> Result<Self> {
        let rows = items.iter().map(row).collect();
        Self::new(&items, columns, rows, empty)
    }

    /// A list whose JSON payload is `payload`, and whose table is `rows`.
    pub(crate) fn new(
        payload: &impl Serialize,
        columns: &'static [Column],
        rows: Vec<Vec<String>>,
        empty: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            columns,
            rows,
            json: serde_json::to_value(payload)?,
            json_text: serde_json::to_string_pretty(payload)?,
            empty: empty.into(),
            count: None,
            notes: Vec::new(),
            section: None,
            incomplete: None,
        })
    }

    pub(crate) fn count(mut self, count: Count) -> Self {
        self.count = Some(count);
        self
    }

    pub(crate) fn note(mut self, note: Note) -> Self {
        self.notes.push(note);
        self
    }

    /// Print the list as bullets under `heading`, one block for each row.
    pub(crate) fn section(mut self, heading: &'static str, blocks: Vec<Block>) -> Self {
        self.section = Some(Section { heading, blocks });
        self
    }

    /// Mark the read as incomplete.
    pub(crate) fn incomplete(
        mut self,
        note: impl Into<String>,
        quiet_error: impl Into<String>,
    ) -> Self {
        self.incomplete = Some(Incomplete {
            note: note.into(),
            quiet_error: quiet_error.into(),
        });
        self
    }

    /// The JSON payload, in the field order of its type.
    pub(crate) fn json_text(&self) -> &str {
        &self.json_text
    }

    /// The table columns and rows, for a command that prints its own table.
    pub(crate) fn table(&self) -> (&'static [Column], &[Vec<String>]) {
        (self.columns, &self.rows)
    }

    /// Print the list as the command prints it without `--host`.
    pub(crate) fn print(&self, format: Format) -> Result<()> {
        if format == Format::Json {
            outln!("{}", self.json_text);
            return Ok(());
        }
        emit(render(&[(None, self)], view())?)
    }
}

/// Print the lists of several hosts as one list, in the order given.
pub(crate) fn print_merged(parts: &[(String, Listing)], format: Format) -> Result<()> {
    if format == Format::Json {
        let parts: Vec<(&str, &Value)> = parts
            .iter()
            .map(|(host, listing)| (host.as_str(), &listing.json))
            .collect();
        return super::print_json(&merge_json(&parts));
    }
    let parts: Vec<(Option<&str>, &Listing)> = parts
        .iter()
        .map(|(host, listing)| (Some(host.as_str()), listing))
        .collect();
    emit(render(&parts, view())?)
}

fn emit(rendered: Rendered) -> Result<()> {
    for line in rendered.lines {
        match line {
            Out::Stdout(text) => outln!("{text}"),
            Out::Stderr(text) => note(&text),
        }
    }
    match rendered.error {
        Some(error) => Err(anyhow::anyhow!(error)),
        None => Ok(()),
    }
}

/// The text of one list, or of the lists of several hosts. A host name on a
/// part makes the list of every host: the `HOST` column, a `host:` line on
/// each block, and the host on each hint.
fn render(parts: &[(Option<&str>, &Listing)], view: &View) -> Result<Rendered> {
    let mut out = Rendered::default();
    let Some((_, first)) = parts.first() else {
        return Ok(out);
    };
    let hosted = parts.iter().any(|(host, _)| host.is_some());
    let empty = parts.iter().all(|(_, listing)| listing.rows.is_empty());
    let incomplete: Vec<String> = parts
        .iter()
        .filter_map(|(host, listing)| {
            let incomplete = listing.incomplete.as_ref()?;
            Some(on_host(*host, &incomplete.note))
        })
        .collect();

    if view.quiet {
        let rows: Vec<Vec<String>> = parts
            .iter()
            .flat_map(|(_, listing)| listing.rows.iter().cloned())
            .collect();
        if !rows.is_empty() {
            out.lines.push(Out::Stdout(quiet_lines(&rows)));
        } else if first.section.is_none() && !first.empty.is_empty() {
            out.lines.push(Out::Stderr(first.empty.clone()));
        }
        out.error = parts.iter().find_map(|(host, listing)| {
            let incomplete = listing.incomplete.as_ref()?;
            Some(on_host(*host, &incomplete.quiet_error))
        });
        return Ok(out);
    }

    if let Some(section) = &first.section {
        if empty {
            match incomplete.is_empty() {
                true => out.lines.push(Out::Stderr(first.empty.clone())),
                false => out.lines.extend(incomplete.into_iter().map(Out::Stderr)),
            }
            return Ok(out);
        }
        if view.columns.is_empty() {
            let mut lines = vec![section.heading.to_owned()];
            for (host, listing) in parts {
                let blocks = listing.section.as_ref().map_or(&[][..], |s| &s.blocks);
                for block in blocks {
                    lines.extend(block_lines(block, *host));
                }
            }
            out.lines.push(Out::Stdout(lines.join("\n")));
            if !incomplete.is_empty() {
                out.lines.push(Out::Stdout(String::new()));
            }
        } else {
            out.lines.push(Out::Stdout(table(parts, hosted, view)?));
        }
        out.lines.extend(incomplete.into_iter().map(Out::Stderr));
        return Ok(out);
    }

    out.lines.push(Out::Stdout(table(parts, hosted, view)?));
    if empty && incomplete.is_empty() && !first.empty.is_empty() {
        out.lines.push(Out::Stderr(first.empty.clone()));
    }
    out.lines.extend(incomplete.into_iter().map(Out::Stderr));
    if let Some(count) = first.count {
        let (shown, total) = parts
            .iter()
            .filter_map(|(_, listing)| listing.count)
            .fold((0, 0), |(shown, total), count| {
                (shown + count.shown, total + count.total)
            });
        out.lines
            .push(Out::Stdout(format!("{shown} of {total} {}", count.noun)));
    }
    let mut legends: Vec<&str> = Vec::new();
    for (host, listing) in parts {
        for note in &listing.notes {
            match note {
                Note::Legend(text) if !legends.contains(&text.as_str()) => {
                    legends.push(text);
                    out.lines.push(Out::Stderr(text.clone()));
                }
                Note::Legend(_) => {}
                Note::Next(command) => out.lines.push(Out::Stderr(match host {
                    Some(host) => format!("{host}: Next: {command} --host {host}"),
                    None => format!("Next: {command}"),
                })),
            }
        }
    }
    Ok(out)
}

/// A note as the list of every host says it: after the host's name.
fn on_host(host: Option<&str>, text: &str) -> String {
    match host {
        Some(host) => format!("{host}: {text}"),
        None => text.to_owned(),
    }
}

/// One table over every part, fitted one time. The list of every host leads
/// each row with the host, and keeps `HOST` first whatever `--columns` names.
fn table(parts: &[(Option<&str>, &Listing)], hosted: bool, view: &View) -> Result<String> {
    let columns = parts[0].1.columns;
    if !hosted {
        return render_table(columns, &parts[0].1.rows, view);
    }
    let columns: Vec<Column> = std::iter::once(HOST)
        .chain(columns.iter().copied())
        .collect();
    let rows: Vec<Vec<String>> = parts
        .iter()
        .flat_map(|(host, listing)| {
            listing.rows.iter().map(move |row| {
                std::iter::once(host.unwrap_or_default().to_owned())
                    .chain(row.iter().cloned())
                    .collect()
            })
        })
        .collect();
    let view = match view.columns.is_empty() {
        true => view.clone(),
        false => View {
            columns: std::iter::once(HOST.header.to_owned())
                .chain(
                    view.columns
                        .iter()
                        .filter(|name| !name.trim().eq_ignore_ascii_case(HOST.header))
                        .cloned(),
                )
                .collect(),
            ..view.clone()
        },
    };
    render_table(&columns, &rows, &view)
}

/// The lines of one bullet. On the list of every host, the host follows the
/// bullet, and each command names the host.
fn block_lines(block: &Block, host: Option<&str>) -> Vec<String> {
    let mut lines = vec![format!("- {}", block.head)];
    if let Some(host) = host {
        lines.push(format!("  host: {host}"));
    }
    for line in &block.lines {
        let value = match (line.command, host) {
            (true, Some(host)) => format!("{} --host {host}", line.value),
            _ => line.value.clone(),
        };
        lines.push(format!("  {}: {value}", line.key));
    }
    lines
}

/// The JSON of several hosts in the form of one host's JSON.
///
/// An array stays an array of every host's items. An object keeps its keys:
/// an array concatenates, a count adds up, a flag holds only where it holds on
/// every host, and `next_cursor` becomes one cursor for each host name. Each
/// item that is an object gets a `host` field.
fn merge_json(parts: &[(&str, &Value)]) -> Value {
    let Some((_, first)) = parts.first() else {
        return Value::Array(Vec::new());
    };
    match first {
        Value::Array(_) => Value::Array(hosted_items(parts.iter().copied())),
        Value::Object(object) => {
            let mut merged = Map::new();
            for (key, value) in object {
                let mut values = parts
                    .iter()
                    .map(|(host, part)| (*host, part.get(key).unwrap_or(&Value::Null)));
                let merged_value = if key == "next_cursor" {
                    Value::Object(
                        values
                            .map(|(host, cursor)| (host.to_owned(), cursor.clone()))
                            .collect(),
                    )
                } else {
                    match value {
                        Value::Array(_) => Value::Array(hosted_items(values)),
                        Value::Number(_) => {
                            let sum: u64 = values.filter_map(|(_, v)| v.as_u64()).sum();
                            sum.into()
                        }
                        Value::Bool(_) => {
                            Value::Bool(values.all(|(_, v)| v.as_bool().unwrap_or(false)))
                        }
                        other => other.clone(),
                    }
                };
                merged.insert(key.clone(), merged_value);
            }
            Value::Object(merged)
        }
        other => (*other).clone(),
    }
}

/// Every host's items in order, each object with the `host` it came from.
fn hosted_items<'a>(parts: impl Iterator<Item = (&'a str, &'a Value)>) -> Vec<Value> {
    parts
        .flat_map(|(host, value)| {
            value
                .as_array()
                .into_iter()
                .flatten()
                .map(move |item| match item {
                    Value::Object(object) => {
                        let mut object = object.clone();
                        object.insert("host".into(), host.into());
                        Value::Object(object)
                    }
                    other => other.clone(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const COLUMNS: &[Column] = &[col("id", UNCAPPED).id(), col("title", 48).title()];

    fn listing(rows: &[(&str, &str)]) -> Listing {
        let items: Vec<Value> = rows
            .iter()
            .map(|(id, title)| json!({"id": id, "title": title}))
            .collect();
        Listing::of(
            &items,
            COLUMNS,
            |item| {
                vec![
                    item["id"].as_str().unwrap().into(),
                    item["title"].as_str().unwrap().into(),
                ]
            },
            "No items.",
        )
        .unwrap()
    }

    fn stdout(rendered: &Rendered) -> Vec<String> {
        rendered
            .lines
            .iter()
            .filter_map(|line| match line {
                Out::Stdout(text) => Some(text.clone()),
                Out::Stderr(_) => None,
            })
            // `println!` of an empty text is one empty line.
            .flat_map(|text| text.split('\n').map(str::to_owned).collect::<Vec<_>>())
            .collect()
    }

    fn stderr(rendered: &Rendered) -> Vec<String> {
        rendered
            .lines
            .iter()
            .filter_map(|line| match line {
                Out::Stderr(text) => Some(text.clone()),
                Out::Stdout(_) => None,
            })
            .collect()
    }

    #[test]
    fn every_host_is_one_table_led_by_host_and_aligned_across_hosts() {
        let local = listing(&[("a1", "short")]);
        let build = listing(&[("b-very-long-id", "a much longer title")]);
        let rendered = render(
            &[(Some("local"), &local), (Some("build"), &build)],
            &View::plain(),
        )
        .unwrap();
        assert_eq!(
            stdout(&rendered),
            [
                "HOST    ID               TITLE",
                "local   a1               short",
                "build   b-very-long-id   a much longer title",
            ]
        );
    }

    #[test]
    fn columns_keep_host_first() {
        let local = listing(&[("a1", "short")]);
        let view = View {
            columns: vec!["title".into(), "HOST".into()],
            ..View::plain()
        };
        let rendered = render(&[(Some("local"), &local)], &view).unwrap();
        assert_eq!(stdout(&rendered), ["HOST    TITLE", "local   short"]);
    }

    #[test]
    fn one_summary_counts_every_host_and_is_no_row() {
        let count = |shown, total| Count {
            shown,
            total,
            noun: "sessions",
        };
        let local = listing(&[("a1", "one")]).count(count(1, 4));
        let build = listing(&[("b1", "two"), ("b2", "three")]).count(count(2, 9));
        let rendered = render(
            &[(Some("local"), &local), (Some("build"), &build)],
            &View::plain(),
        )
        .unwrap();
        let lines = stdout(&rendered);
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert_eq!(lines.last().unwrap(), "3 of 13 sessions");
    }

    #[test]
    fn an_empty_list_on_every_host_prints_the_one_empty_note() {
        let local = listing(&[]);
        let build = listing(&[]);
        let rendered = render(
            &[(Some("local"), &local), (Some("build"), &build)],
            &View::plain(),
        )
        .unwrap();
        assert_eq!(stdout(&rendered), ["HOST   ID   TITLE"]);
        assert_eq!(stderr(&rendered), ["No items."]);
    }

    #[test]
    fn a_hint_names_its_host_and_a_legend_prints_one_time() {
        let local = listing(&[("a1", "one")])
            .note(Note::Legend("* default".into()))
            .note(Note::Next("ariadne session ls --cursor c1".into()));
        let build = listing(&[("b1", "two")]).note(Note::Legend("* default".into()));
        let rendered = render(
            &[(Some("local"), &local), (Some("build"), &build)],
            &View::plain(),
        )
        .unwrap();
        assert_eq!(
            stderr(&rendered),
            [
                "* default",
                "local: Next: ariadne session ls --cursor c1 --host local"
            ]
        );
    }

    #[test]
    fn quiet_prints_only_ids_of_every_host() {
        let local = listing(&[("a1", "one")]);
        let build = listing(&[("b1", "two")]).count(Count {
            shown: 1,
            total: 1,
            noun: "sessions",
        });
        let view = View {
            quiet: true,
            ..View::plain()
        };
        let rendered = render(&[(Some("local"), &local), (Some("build"), &build)], &view).unwrap();
        assert_eq!(stdout(&rendered), ["a1", "b1"]);
        assert!(stderr(&rendered).is_empty());
    }

    #[test]
    fn a_section_names_the_host_of_each_item_and_on_each_command() {
        let block = |id: &str| Block {
            head: format!("[access] {id}"),
            lines: vec![
                Line::text("action", "sign in"),
                Line::command("dismiss", format!("ariadne attention dismiss {id}")),
            ],
        };
        let local = listing(&[("a1", "one")]).section("NEEDS ATTENTION", vec![block("a1")]);
        let build = listing(&[("b1", "two")])
            .section("NEEDS ATTENTION", vec![block("b1")])
            .incomplete("Some could not be read.", "not the whole list");
        let rendered = render(
            &[(Some("local"), &local), (Some("build"), &build)],
            &View::plain(),
        )
        .unwrap();
        assert_eq!(
            stdout(&rendered),
            [
                "NEEDS ATTENTION",
                "- [access] a1",
                "  host: local",
                "  action: sign in",
                "  dismiss: ariadne attention dismiss a1 --host local",
                "- [access] b1",
                "  host: build",
                "  action: sign in",
                "  dismiss: ariadne attention dismiss b1 --host build",
                "",
            ]
        );
        assert_eq!(stderr(&rendered), ["build: Some could not be read."]);
    }

    #[test]
    fn one_host_without_a_name_prints_the_list_as_it_was() {
        let local = listing(&[("a1", "one")]).note(Note::Next("ariadne x --cursor c".into()));
        let rendered = render(&[(None, &local)], &View::plain()).unwrap();
        assert_eq!(stdout(&rendered), ["ID   TITLE", "a1   one"]);
        assert_eq!(stderr(&rendered), ["Next: ariadne x --cursor c"]);
    }

    #[test]
    fn json_keeps_the_form_of_one_host() {
        let array = merge_json(&[
            ("local", &json!([{"id": "a1"}])),
            ("build", &json!([{"id": "b1"}])),
        ]);
        assert_eq!(
            array,
            json!([{"id": "a1", "host": "local"}, {"id": "b1", "host": "build"}])
        );
        let page = merge_json(&[
            (
                "local",
                &json!({"sessions": [{"id": "a1"}], "total": 4, "next_cursor": "c1"}),
            ),
            (
                "build",
                &json!({"sessions": [{"id": "b1"}], "total": 3, "next_cursor": null}),
            ),
        ]);
        assert_eq!(
            page,
            json!({
                "sessions": [{"id": "a1", "host": "local"}, {"id": "b1", "host": "build"}],
                "total": 7,
                "next_cursor": {"local": "c1", "build": null},
            })
        );
        let attention = merge_json(&[
            (
                "local",
                &json!({"complete": true, "count": 1, "recovery": [{"id": "a"}]}),
            ),
            (
                "build",
                &json!({"complete": false, "count": 0, "recovery": []}),
            ),
        ]);
        assert_eq!(
            attention,
            json!({"complete": false, "count": 1, "recovery": [{"id": "a", "host": "local"}]})
        );
    }
}
