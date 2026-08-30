//! What the caller typed, turned into the id the daemon holds.
//!
//! Ids are 26-character lowercase ULIDs, and nobody types one: they are
//! pasted — sometimes upper-cased on the way through a terminal — or read off
//! a table that shows them as `…last8`, which is the only spelling the web UI
//! and `ariadne attention` ever print. So every id argument is matched
//! leniently here before it is sent: lower-cased, then exact, then a unique
//! prefix, then a unique suffix.
//!
//! A whole id costs nothing — it is recognised by its shape and sent as it
//! is. Anything shorter is looked up in the daemon's own list, so an id that
//! names two things says which two, and one that names nothing says where the
//! list of them is.

use anyhow::Result;
use serde_json::Value;

use ariadne_client::Client;

use crate::error::Failure;

/// What kind of thing an id names, which is what decides the list to look
/// through and the words a failure is spelled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Goal,
    Task,
    Session,
    Repo,
    Profile,
}

impl Kind {
    /// The word one of them is called by.
    fn noun(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Task => "task",
            Self::Session => "session",
            Self::Repo => "repository",
            Self::Profile => "profile",
        }
    }

    /// The word several of them are called by.
    fn plural(self) -> &'static str {
        match self {
            Self::Goal => "goals",
            Self::Task => "tasks",
            Self::Session => "sessions",
            Self::Repo => "repositories",
            Self::Profile => "profiles",
        }
    }

    /// Where the whole list of them is.
    fn path(self) -> &'static str {
        match self {
            Self::Goal => "/v1/goals",
            Self::Task => "/v1/tasks",
            Self::Session => "/v1/sessions",
            Self::Repo => "/v1/repositories",
            Self::Profile => "/v1/profiles",
        }
    }

    /// The command that shows them, for whoever typed an id that names none.
    fn lister(self) -> &'static str {
        match self {
            Self::Goal => "ariadne goal ls",
            Self::Task => "ariadne task ls",
            Self::Session => "ariadne session ls",
            Self::Repo => "ariadne repo ls",
            Self::Profile => "ariadne profile ls",
        }
    }

    /// One row of that list as the resolver sees it.
    fn row(self, v: &Value) -> Row {
        let f = |key: &str| field(v, key);
        let (label, alias) = match self {
            Self::Goal | Self::Task => (f("title"), None),
            Self::Session => (format!("{} session ({})", f("role"), f("status")), None),
            Self::Repo => (
                format!("{} [{}]", f("path"), f("base_branch")),
                Some(f("path")),
            ),
            Self::Profile => (format!("{} ({})", f("name"), f("role")), Some(f("name"))),
        };
        Row {
            id: f("id"),
            label,
            alias,
        }
    }
}

/// One thing an id could name: the id itself, what a person recognises it by,
/// and the other spelling it answers to where it has one — a repository's
/// path, a profile's name.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub label: String,
    pub alias: Option<String>,
}

impl Row {
    /// A row of the list an ambiguous id is answered with.
    fn line(&self) -> String {
        format!("  {}  {}", self.id, self.label)
    }
}

/// The list an id is looked up in, and the words to talk about it in.
///
/// Built from the daemon's answer by [`catalog`]; built from a literal list by
/// the tests, which is the whole reason the matching does not fetch anything
/// itself.
pub struct Catalog {
    noun: String,
    plural: String,
    hint: String,
    rows: Vec<Row>,
}

impl Catalog {
    /// The one row `typed` names: the id itself, a name or path it answers to,
    /// a unique prefix of an id, or the `…last8` tail every table shows.
    ///
    /// Case never matters: ids are lowercase, and a terminal that upper-cases
    /// a paste must not turn a correct id into a missing one.
    pub fn pick(&self, typed: &str) -> Result<&Row> {
        let needle = typed.trim().to_lowercase();
        // Every id starts with it, so an empty argument would name whatever
        // happens to be the only row.
        if needle.is_empty() {
            return Err(Failure::usage(format!("no {} named", self.noun))
                .hint(&self.hint)
                .err());
        }
        let id = |r: &Row| r.id.to_lowercase();
        if let Some(row) = self.rows.iter().find(|r| id(r) == needle) {
            return Ok(row);
        }
        let by_alias =
            self.matching(|r| r.alias.as_ref().is_some_and(|a| a.to_lowercase() == needle));
        let matches = match by_alias.is_empty() {
            false => by_alias,
            // A prefix decides only when it is the only one: the tail a table
            // shows can happen to be the head of other ids too, and it is
            // still that one row's tail. Neither spelling being unique is
            // ambiguous over both of them.
            true => {
                let prefixes = self.matching(|r| id(r).starts_with(&needle));
                let suffixes = self.matching(|r| id(r).ends_with(&needle));
                match (prefixes.as_slice(), suffixes.as_slice()) {
                    ([one], _) | (_, [one]) => vec![*one],
                    _ => either(prefixes, suffixes),
                }
            }
        };
        match matches.as_slice() {
            [row] => Ok(row),
            [] => Err(
                Failure::not_found(format!("no {} matches \"{typed}\"", self.noun))
                    .hint(&self.hint)
                    .err(),
            ),
            several => Err(Failure::usage(format!(
                "\"{typed}\" matches {} {} — say which:\n{}",
                several.len(),
                self.plural,
                several
                    .iter()
                    .map(|r| r.line())
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
            .err()),
        }
    }

    fn matching(&self, is_match: impl Fn(&Row) -> bool) -> Vec<&Row> {
        self.rows.iter().filter(|r| is_match(r)).collect()
    }
}

/// Everything either spelling matched, each row once: what an id that is the
/// head of some and the tail of others is answered with.
fn either<'a>(prefixes: Vec<&'a Row>, suffixes: Vec<&'a Row>) -> Vec<&'a Row> {
    let mut out = prefixes;
    for row in suffixes {
        if !out.iter().any(|already| already.id == row.id) {
            out.push(row);
        }
    }
    out
}

/// A catalog over rows the caller has already fetched: `goal create --repo`
/// and `task create --repo` read the repository list for other reasons
/// anyway, and it is what the tests match against.
pub fn among(kind: Kind, rows: impl IntoIterator<Item = Row>) -> Catalog {
    Catalog {
        noun: kind.noun().into(),
        plural: kind.plural().into(),
        hint: format!("{} lists them", kind.lister()),
        rows: rows.into_iter().collect(),
    }
}

/// One row of a list the caller already has, matched on its id alone — a
/// repository's path is its own command's to answer for, in its own words.
pub fn row(id: impl Into<String>, label: impl Into<String>) -> Row {
    Row {
        id: id.into(),
        label: label.into(),
        alias: None,
    }
}

/// The profiles the arguments of one command name.
///
/// A profile is named by its name as often as by its id — `--planner`,
/// `--engineer`, a `--reviewer`'s half — so the list is what all of them are
/// matched against, read once however many there are and not at all when
/// every one of them is already a whole id.
pub enum Profiles<'a> {
    /// Nothing read yet, and where to read it from.
    Daemon(&'a Client),
    /// The profiles, once something has needed them — which is also how a
    /// test hands over a list with no daemon behind it.
    List(Catalog),
}

impl<'a> Profiles<'a> {
    pub fn new(client: &'a Client) -> Self {
        Self::Daemon(client)
    }

    /// One profile argument as the daemon should receive it: the id behind a
    /// name, a whole id, or an id in any of the short spellings it is shown
    /// in — which the daemon's own exact id-or-name lookup cannot take.
    pub async fn id(&mut self, typed: &str) -> Result<String> {
        if let Some(id) = whole_id(typed) {
            return Ok(id);
        }
        if let Self::Daemon(client) = self {
            *self = Self::List(catalog(client, Kind::Profile).await?);
        }
        let Self::List(list) = self else {
            unreachable!("the profiles have just been read")
        };
        Ok(list.pick(typed)?.id.clone())
    }
}

/// The id the daemon holds for what was typed, fetching its list only when
/// the typed value is not already a whole id.
pub async fn id(client: &Client, kind: Kind, typed: &str) -> Result<String> {
    if let Some(id) = whole_id(typed) {
        return Ok(id);
    }
    Ok(catalog(client, kind).await?.pick(typed)?.id.clone())
}

/// The same for a repeatable id argument (`task update --depends-on`), with
/// one list fetch behind however many were typed.
pub async fn ids(client: &Client, kind: Kind, typed: &[String]) -> Result<Vec<String>> {
    if typed.iter().all(|t| whole_id(t).is_some()) {
        return Ok(typed.iter().filter_map(|t| whole_id(t)).collect());
    }
    let catalog = catalog(client, kind).await?;
    typed
        .iter()
        .map(|t| Ok(catalog.pick(t)?.id.clone()))
        .collect()
}

/// The same where the id may name any of the three things `ariadne attach`
/// takes: what it names is the caller's to know, so a short id that names one
/// of each is refused rather than guessed at.
pub async fn attachable(client: &Client, typed: &str) -> Result<String> {
    if let Some(id) = whole_id(typed) {
        return Ok(id);
    }
    let mut rows = Vec::new();
    for kind in [Kind::Session, Kind::Task, Kind::Goal] {
        rows.extend(list(client, kind).await?.into_iter().map(|mut row| {
            row.label = format!("{}: {}", kind.noun(), row.label);
            row
        }));
    }
    Ok(Catalog {
        noun: "session, task or goal".into(),
        plural: "of them".into(),
        hint: "ariadne session ls, ariadne task ls and ariadne goal ls list them".into(),
        rows,
    }
    .pick(typed)?
    .id
    .clone())
}

/// Everything of one kind the daemon holds, as rows to match against.
async fn catalog(client: &Client, kind: Kind) -> Result<Catalog> {
    Ok(Catalog {
        noun: kind.noun().into(),
        plural: kind.plural().into(),
        hint: format!("{} lists them", kind.lister()),
        rows: list(client, kind).await?,
    })
}

/// One list endpoint, read as rows — the same fetch every listing command
/// makes, with only the fields a lookup needs read out of it.
async fn list(client: &Client, kind: Kind) -> Result<Vec<Row>> {
    let rows: Vec<Value> = client.get_json(kind.path()).await?;
    Ok(rows.iter().map(|v| kind.row(v)).collect())
}

/// A whole id, in the one spelling the daemon stores: 26 Crockford base32
/// characters, lower-cased, so an upper-cased paste is the same id.
///
/// Nothing else is this shape — a profile name of 26 characters would have to
/// avoid `i`, `l`, `o` and `u` and start with a digit — and recognising it is
/// what keeps `task inspect <id>` at one request.
fn whole_id(typed: &str) -> Option<String> {
    const ALPHABET: &str = "0123456789abcdefghjkmnpqrstvwxyz";
    let id = typed.trim().to_lowercase();
    let mut chars = id.chars();
    // A ULID's first character carries the top bits of a 48-bit millisecond
    // timestamp: '0'..='7' until the year 10889.
    let first = chars.next()?;
    (id.len() == 26 && ('0'..='7').contains(&first) && chars.all(|c| ALPHABET.contains(c)))
        .then_some(id)
}

/// One string field of a listed row, empty where the daemon did not send one.
fn field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::error::Exit;

    /// Two tasks that share their first eight characters and differ in their
    /// last eight, plus one that shares neither: the three cases a short id
    /// has to tell apart.
    fn tasks() -> Catalog {
        Catalog {
            noun: "task".into(),
            plural: "tasks".into(),
            hint: "ariadne task ls lists them".into(),
            rows: vec![
                row("01m15jmta93b130wka2qdn2p1x", "CLI: short ids resolve"),
                row("01m15jmtb7zzzzzzzzzz9f4k2c", "UI: keep the diff live"),
                row("01m0zzzz00000000000000abcd", "Daemon: heartbeat"),
            ],
        }
    }

    fn picked(typed: &str) -> String {
        tasks().pick(typed).expect("one task").id.clone()
    }

    fn failed(typed: &str) -> anyhow::Error {
        tasks().pick(typed).expect_err("no single task")
    }

    #[test]
    fn a_whole_id_is_the_id_itself() {
        assert_eq!(
            picked("01m15jmta93b130wka2qdn2p1x"),
            "01m15jmta93b130wka2qdn2p1x"
        );
    }

    /// Some terminals upper-case a paste, and every id in the system is
    /// lowercase.
    #[test]
    fn case_never_decides_whether_an_id_is_found() {
        assert_eq!(
            picked("01M15JMTA93B130WKA2QDN2P1X"),
            "01m15jmta93b130wka2qdn2p1x"
        );
        assert_eq!(picked("01M0ZZZZ"), "01m0zzzz00000000000000abcd");
    }

    /// The head of an id, as a `ls` table shows it in full.
    #[test]
    fn a_unique_prefix_is_enough() {
        assert_eq!(picked("01m15jmta"), "01m15jmta93b130wka2qdn2p1x");
    }

    /// The tail of an id, which is all the UI and `ariadne attention` print.
    #[test]
    fn the_last_eight_characters_are_enough() {
        assert_eq!(picked("2qdn2p1x"), "01m15jmta93b130wka2qdn2p1x");
        assert_eq!(picked("zz9f4k2c"), "01m15jmtb7zzzzzzzzzz9f4k2c");
    }

    /// A tail is still a tail when it happens to be the head of other ids:
    /// what a table shows is the last eight characters, and a prefix decides
    /// only when it is the one prefix there is.
    #[test]
    fn a_shared_prefix_does_not_hide_a_unique_suffix() {
        let catalog = Catalog {
            noun: "task".into(),
            plural: "tasks".into(),
            hint: "ariadne task ls lists them".into(),
            rows: vec![
                // The needle "01m15jmt" heads these two...
                row("01m15jmta93b130wka2qdn2p1x", "CLI: short ids resolve"),
                row("01m15jmtb7zzzzzzzzzz9f4k2c", "UI: keep the diff live"),
                // ...and is the tail of this one, which is the row a table
                // showed as "…01m15jmt".
                row("01m0zzzz0000000000001m15jmt", "Daemon: heartbeat"),
            ],
        };
        assert_eq!(
            catalog.pick("01m15jmt").expect("the one tail").id,
            "01m0zzzz0000000000001m15jmt"
        );
    }

    /// Neither spelling naming one row is ambiguous over all of them, each
    /// listed once however many ways it matched.
    #[test]
    fn an_id_that_is_no_ones_only_head_or_tail_lists_every_match() {
        let catalog = Catalog {
            noun: "task".into(),
            plural: "tasks".into(),
            hint: "ariadne task ls lists them".into(),
            rows: vec![
                row("01aa0000000000000000000001", "heads it"),
                row("01aa0000000000000000000002", "heads it too"),
                row("01bb00000000000000000001aa", "tails it"),
                // Both the head and the tail of this one, and still one row.
                row("01aa00000000000000000001aa", "heads it and tails it"),
            ],
        };
        let err = catalog.pick("01aa").expect_err("no single task");
        let line = err.to_string();
        assert!(line.starts_with("\"01aa\" matches 4 tasks"), "{line}");
        assert_eq!(
            line.matches("01aa00000000000000000001aa").count(),
            1,
            "listed once however many ways it matched: {line}"
        );
    }

    /// A prefix two tasks share names neither: the answer says which two, by
    /// the titles that tell them apart, and is a usage failure — the caller
    /// has to type more.
    #[test]
    fn a_shared_prefix_names_both_matches() {
        let err = failed("01m15jmt");
        let line = err.to_string();
        assert!(line.starts_with("\"01m15jmt\" matches 2 tasks"), "{line}");
        assert!(line.contains("CLI: short ids resolve"), "{line}");
        assert!(line.contains("UI: keep the diff live"), "{line}");
        assert_eq!(crate::error::exit(&err), Exit::Usage);
    }

    /// An id that names nothing is a missing thing, not a broken command, and
    /// says where the list of them is.
    #[test]
    fn an_id_that_names_nothing_points_at_the_listing() {
        let err = failed("nope");
        assert_eq!(err.to_string(), "no task matches \"nope\"");
        assert_eq!(crate::error::exit(&err), Exit::NotFound);
        assert!(
            crate::error::human_line(&err).contains("ariadne task ls lists them"),
            "{err}"
        );
    }

    /// An empty argument is every id's prefix, and naming nothing is not the
    /// same as naming the only thing there is.
    #[test]
    fn an_empty_id_names_nothing() {
        let err = failed("");
        assert_eq!(err.to_string(), "no task named");
        assert_eq!(crate::error::exit(&err), Exit::Usage);
    }

    /// A repository answers to its path and a profile to its name, exactly —
    /// the spellings they were resolvable by before short ids existed.
    #[test]
    fn a_name_or_path_still_names_its_own_row() {
        let repos = Catalog {
            noun: "repository".into(),
            plural: "repositories".into(),
            hint: "ariadne repo ls lists them".into(),
            rows: vec![Row {
                alias: Some("/home/me/api".into()),
                ..row("01m0repo00000000000000abcd", "/home/me/api [main]")
            }],
        };
        assert_eq!(
            repos.pick("/home/me/api").expect("the repo").id,
            "01m0repo00000000000000abcd"
        );
        assert!(repos.pick("/home/me/other").is_err());
    }

    /// The one shape that skips the list fetch, and everything that must not
    /// be mistaken for it.
    #[test]
    fn only_a_whole_ulid_travels_without_a_lookup() {
        assert_eq!(
            whole_id("01M15JMTA93B130WKA2QDN2P1X").as_deref(),
            Some("01m15jmta93b130wka2qdn2p1x")
        );
        assert_eq!(whole_id("01m15jmta"), None, "a prefix is not a whole id");
        assert_eq!(whole_id("2qdn2p1x"), None, "nor is a tail");
        assert_eq!(
            whole_id("SecurityReviewerProfileAb"),
            None,
            "nor is a 26-character name: it starts outside the timestamp"
        );
        assert_eq!(
            whole_id("01m15jmta93b130wka2qdn2p1i"),
            None,
            "i, l, o and u are not in the alphabet"
        );
    }

    /// Two profiles, as `/v1/profiles` lists them: the id, and the name that
    /// is what a caller normally types.
    fn profiles() -> Profiles<'static> {
        Profiles::List(among(
            Kind::Profile,
            [
                Row {
                    id: "01m0prof0000000000000abcde".into(),
                    label: "Reviewer (reviewer)".into(),
                    alias: Some("Reviewer".into()),
                },
                Row {
                    id: "01m0prof0000000000000fghjk".into(),
                    label: "My Engineer (engineer)".into(),
                    alias: Some("My Engineer".into()),
                },
            ],
        ))
    }

    /// `--planner`, `--engineer`, a `--reviewer`'s half: all documented as
    /// taking an id or a name, and the daemon's own lookup is exact — so the
    /// short and upper-cased spellings are resolved here or not at all.
    #[tokio::test]
    async fn a_profile_argument_takes_a_name_or_an_id_in_any_spelling() {
        let mut profiles = profiles();
        assert_eq!(
            profiles.id("Reviewer").await.expect("a name"),
            "01m0prof0000000000000abcde"
        );
        assert_eq!(
            profiles.id("my engineer").await.expect("a name, any case"),
            "01m0prof0000000000000fghjk"
        );
        assert_eq!(
            profiles.id("0000abcde").await.expect("the tail of an id"),
            "01m0prof0000000000000abcde"
        );
        assert_eq!(
            profiles
                .id("01M0PROF0000000000000ABCDE")
                .await
                .expect("an upper-cased paste"),
            "01m0prof0000000000000abcde"
        );
        let err = profiles.id("Nobody").await.expect_err("no such profile");
        assert_eq!(err.to_string(), "no profile matches \"Nobody\"");
    }

    /// The rows come off the daemon's own listing payloads, which is where
    /// the titles in an ambiguity answer come from.
    #[test]
    fn a_listed_row_is_read_for_its_id_and_its_title() {
        let task = serde_json::json!({"id": "01m0t", "title": "Wire the screen"});
        let row = Kind::Task.row(&task);
        assert_eq!(row.id, "01m0t");
        assert_eq!(row.label, "Wire the screen");
        assert_eq!(row.alias, None);

        let profile = serde_json::json!({"id": "01m0p", "name": "Reviewer", "role": "reviewer"});
        let row = Kind::Profile.row(&profile);
        assert_eq!(row.label, "Reviewer (reviewer)");
        assert_eq!(row.alias.as_deref(), Some("Reviewer"));
    }
}
