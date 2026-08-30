//! CLI command implementations.

pub mod agent;
pub mod agent_event;
pub mod attach;
pub mod attention;
pub mod completions;
pub mod daemon;
pub mod doctor;
pub mod events;
#[cfg(test)]
pub mod fixtures;
pub mod follow;
pub mod goal;
pub mod mcp;
pub mod models;
pub mod profile;
pub mod repo;
pub mod resolve;
pub mod session;
pub mod setup;
pub mod spawn;
pub mod task;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::json;

use ariadne_api::messages::{MessageDto, MessageRecipientDto};
use ariadne_api::profiles::ProfileDto;
use ariadne_client::{Client, endpoint};
use ariadne_core::models::ModelRef;
use ariadne_core::{RecipientKind, probe};

use crate::output::{Format, local_time, note, print, print_kv, short_id, style, view, warn};

/// `ariadne version` — client version always, daemon version when reachable.
///
/// A daemon that did not answer is not a failure of `version` itself, so it
/// stays on stdout — but it is still a line a person reads, so no "client
/// error (Connect)" in it.
pub async fn version(client: &Client, format: Format) -> Result<()> {
    let daemon = client.version().await;
    let client_version = env!("CARGO_PKG_VERSION");
    // Two builds talking to each other is where the odd 404 and the missing
    // field come from, so it is said out loud rather than left to be noticed.
    let mismatch = daemon
        .as_ref()
        .ok()
        .map(|v| v.version.as_str())
        .filter(|version| *version != client_version);
    let payload = json!({
        "client": {"name": "ariadne", "version": client_version},
        "daemon": match &daemon {
            Ok(v) => json!({"name": v.name, "version": v.version}),
            Err(e) => json!({"error": e.human()}),
        },
        "endpoint": client.endpoint(),
        "mismatch": mismatch.is_some(),
    });
    print(format, &payload, || {
        print_kv(&[
            ("client", format!("ariadne {client_version}")),
            (
                "daemon",
                match &daemon {
                    Ok(v) => format!("{} {}", v.name, v.version),
                    Err(e) => e.human(),
                },
            ),
            // The endpoint has been in `--format json` all along: which
            // daemon answered is half of what the two versions mean.
            ("endpoint", client.endpoint().to_string()),
        ]);
        if let Some(version) = mismatch {
            warn(&version_mismatch(client_version, version));
        }
    })
}

/// What a client and a daemon of different builds are told to do about it.
fn version_mismatch(client: &str, daemon: &str) -> String {
    format!(
        "warning: this client is {client} and the daemon is {daemon} — restart it \
         with: ariadne daemon stop && ariadne daemon start"
    )
}

/// What an irreversible command is about to act on: the kind of thing, the
/// title a person recognises it by, and the id that pins it down.
///
/// Both halves of [`confirm`] need it — the question a person answers and the
/// refusal a script gets — and neither may name something other than what is
/// about to go.
pub struct Subject {
    kind: &'static str,
    title: String,
    id: String,
}

impl Subject {
    pub fn new(kind: &'static str, title: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            id: id.into(),
        }
    }

    /// How a question names it: the title, and the short id every table and
    /// the whole UI show — which is what the reader has in front of them.
    pub fn named(&self) -> String {
        format!("\"{}\" ({})", self.title, short_id(&self.id))
    }
}

/// Ask before something irreversible, and take silence for "no".
///
/// `yes` (`-y`) answers for the caller. A stdin that is not a terminal has
/// nobody to ask, and used to be taken for a yes: `echo | ariadne goal rm
/// <id>` and a cron line deleted without ever saying `--yes`. It is refused
/// instead, the way docker and gh refuse it — the caller writes `--yes` when
/// that is what they meant. Declining is an error too, so `ariadne goal
/// cancel x && deploy` does not run the second half.
pub fn confirm(verb: &str, subject: &Subject, question: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        // The full id, not the short one the question shows: this line is
        // read out of a log, where the id is what identifies the thing.
        return Err(crate::error::Failure::usage(format!(
            "refusing to {verb} {} \"{}\" ({}) without --yes: stdin is not a terminal",
            subject.kind, subject.title, subject.id
        ))
        .err());
    }
    // The prompt is not output: it belongs on stderr with the other notes.
    eprint!("{question} [y/N] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("reading your answer")?;
    match answer.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => Err(crate::error::Failure::usage("aborted").err()),
    }
}

/// A `--to` as the daemon should receive it, or nothing where the message
/// addresses nobody.
///
/// The one profile argument that is not always a profile: `user` is the human
/// and travels as itself, and so does anything the profiles do not answer to
/// — the daemon answers that with the people this thread can address.
pub async fn recipient(client: &Client, to: Option<String>) -> Result<Option<String>> {
    match to {
        Some(to) => Ok(Some(resolve::Profiles::new(client).recipient(&to).await?)),
        None => Ok(None),
    }
}

/// Whom a message addresses, spelled the way `--to` and the MCP `to` spell it:
/// a profile's name, or `user`.
///
/// A profile the database no longer holds leaves its id: the message still
/// addressed somebody, and the id is all that is left to name them.
pub fn recipient_label(recipient: &MessageRecipientDto) -> String {
    match recipient.kind {
        RecipientKind::User => "user".to_string(),
        RecipientKind::Profile => recipient
            .profile_name
            .clone()
            .or_else(|| recipient.profile_id.clone())
            .unwrap_or_else(|| "profile".to_string()),
    }
}

/// The one status `GET /v1/tasks` and `GET /v1/sessions` filter by, when the
/// caller named exactly one: those two endpoints take a single status, so
/// several are narrowed on the answer instead — as `session ls --role`
/// already is. Asking for the one there is keeps the common filter where it
/// belongs, at the daemon.
pub fn one_of<T: Copy>(statuses: &[T]) -> Option<T> {
    match statuses {
        [only] => Some(*only),
        _ => None,
    }
}

/// One conversation message as `goal thread` and `task thread` print it:
/// `[time] role: body`, with the addressee after the author when there is
/// one.
///
/// `[` and `]` stay bare so the line still parses by eye the same way with
/// colour on: the time inside them dims to `META`, the role turns bold, and
/// `→ recipient` dims with it — body and the `:` between them are the
/// content and stay as plain as they always were.
pub fn message_line(message: &MessageDto, color: bool) -> String {
    let role = style::paint(color, style::TITLE, message.author_role.as_str());
    let addressee = match &message.recipient {
        Some(recipient) => style::paint(
            color,
            style::META,
            &format!(" → {}", recipient_label(recipient)),
        ),
        None => String::new(),
    };
    format!(
        "[{}] {role}{addressee}: {}",
        style::paint(color, style::META, &local_time(&message.created_at)),
        message.body
    )
}

/// A conversation as `goal thread` and `task thread` print it: the
/// daemon's own list for a script, one line per message for a person.
pub fn print_messages(messages: &[MessageDto], format: Format) -> Result<()> {
    print(format, &messages, || {
        let color = view().color;
        for message in messages {
            println!("{}", message_line(message, color));
        }
        if messages.is_empty() {
            note("no messages yet");
        }
    })
}

/// How much of a conversation is read when nothing was asked for.
pub const THREAD_LIMIT: u32 = 200;

/// The most `GET .../messages` answers in one request — `ariadne_api::Page`
/// clamps `limit` there — and so the size of a page when there is more to
/// read than that.
const PAGE: u32 = 200;

/// A conversation, from the daemon and onto the terminal: the oldest
/// `--limit` messages, or the newest `--tail` ones, with a word on stderr
/// when there were more.
///
/// The cap used to be silent, which is the worst of both: a thread that had
/// run past 200 messages was cut with nothing to say it had been. What is
/// left out is now said, and both ends of the thread are reachable — the
/// daemon pages, so neither is limited to what one request holds.
pub async fn print_thread(
    client: &Client,
    path: &str,
    limit: Option<u32>,
    tail: Option<u32>,
    format: Format,
) -> Result<()> {
    let want = tail.or(limit).unwrap_or(THREAD_LIMIT);
    let (messages, more) = match tail {
        // The end of a thread is only knowable from its start: keyset
        // pagination pages forward, so the tail is what is left after reading
        // the whole of it.
        Some(_) => {
            let all = read_thread(client, path, u32::MAX).await?;
            let more = all.len().saturating_sub(want as usize);
            let messages = all[all.len() - want.min(all.len() as u32) as usize..].to_vec();
            (messages, more)
        }
        None => {
            // One more than asked for, which is what says whether the answer
            // was the whole thread or the start of it.
            let mut all = read_thread(client, path, want.saturating_add(1)).await?;
            let more = all.len().saturating_sub(want as usize);
            all.truncate(want as usize);
            (all, more)
        }
    };
    print_messages(&messages, format)?;
    if more > 0 && matches!(format, Format::Table) {
        note(&match tail {
            Some(_) => format!(
                "{more} earlier message{} not shown — read from the start with --limit",
                plural(more)
            ),
            None => {
                format!("more messages follow — raise --limit, or read the end with --tail {want}")
            }
        });
    }
    Ok(())
}

/// Up to `want` messages of a thread, oldest first, in as many requests as
/// the daemon's page size makes necessary.
async fn read_thread(client: &Client, path: &str, want: u32) -> Result<Vec<MessageDto>> {
    let mut out: Vec<MessageDto> = Vec::new();
    loop {
        let remaining = want.saturating_sub(out.len() as u32);
        if remaining == 0 {
            return Ok(out);
        }
        let limit = remaining.min(PAGE);
        let after = out.last().map(|m| m.id.clone());
        let query = match &after {
            Some(id) => format!("?limit={limit}&after={id}"),
            None => format!("?limit={limit}"),
        };
        let page: Vec<MessageDto> = client.get_json(&format!("{path}{query}")).await?;
        let last = page.len() < limit as usize;
        out.extend(page);
        if last {
            return Ok(out);
        }
    }
}

/// An `s` where there is more than one of something.
fn plural(count: usize) -> &'static str {
    match count {
        1 => "",
        _ => "s",
    }
}

/// Profile ids paired with the names they are known by.
///
/// Profiles are name-addressable everywhere else in the CLI, so an inspect
/// block that prints a bare ULID names nobody.
#[derive(Default)]
pub struct ProfileNames(std::collections::HashMap<String, String>);

impl ProfileNames {
    /// One list call for the whole block. A name is a courtesy: a daemon that
    /// will not answer leaves the ids bare rather than failing the inspect.
    pub async fn fetch(client: &Client) -> Self {
        let profiles: Vec<ProfileDto> = client.get_json("/v1/profiles").await.unwrap_or_default();
        Self(profiles.into_iter().map(|p| (p.id, p.name)).collect())
    }

    /// The same map, built from pairs rather than from the daemon: what a
    /// unit test over a block that names profiles needs, since there is no
    /// daemon behind it.
    #[cfg(test)]
    pub fn from_pairs<I: IntoIterator<Item = (String, String)>>(pairs: I) -> Self {
        Self(pairs.into_iter().collect())
    }

    /// The names for a block that is about to be rendered, or nothing to look
    /// up: `--format json` prints the daemon's payload, where ids are ids.
    pub async fn for_format(client: &Client, format: Format) -> Self {
        match format {
            Format::Table => Self::fetch(client).await,
            Format::Json => Self::default(),
        }
    }

    /// `Name (id)`, or the bare id when no profile answers to it.
    pub fn label(&self, id: &str) -> String {
        match self.0.get(id) {
            Some(name) => format!("{name} ({id})"),
            None => id.to_string(),
        }
    }

    /// `Name (id) · model @ effort`: the mention, plus the two strings that
    /// say what the agent behind it runs on and how deeply it reasons there.
    ///
    /// A profile is editable and a pin is not, so the two drift: what a task's
    /// engineer, a task's reviewer or a goal's planner runs on is the snapshot
    /// taken when it was assigned, not what the profile says today — and where
    /// nothing was pinned, whatever the profile says at spawn time, which is
    /// what "the profile's own" stands for.
    ///
    /// An effort that was never pinned says nothing at all rather than a word
    /// for it: the model is then run at whatever its agent CLI runs it at, and
    /// a `@` with a guess after it would read as a choice somebody made.
    pub fn pinned_label(&self, id: &str, model: Option<&str>, effort: Option<&str>) -> String {
        let pin = model.unwrap_or("the profile's own");
        match effort {
            Some(effort) => format!("{} · {pin} @ {effort}", self.label(id)),
            None => format!("{} · {pin}", self.label(id)),
        }
    }
}

/// Resolve the ariadne home directory the same way the daemon does.
fn ariadne_home(home_override: Option<PathBuf>) -> PathBuf {
    endpoint::home(home_override).unwrap_or_else(|| PathBuf::from(".ariadne"))
}

/// Find the ariadned binary: next to the current executable, else on PATH.
pub fn find_ariadned() -> Result<PathBuf> {
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let sibling = dir.join("ariadned");
        if probe::is_executable(&sibling) {
            return Ok(sibling);
        }
    }
    on_path("ariadned").context("ariadned not found next to ariadne or on PATH")
}

/// A runnable binary of that name on *this shell's* `PATH`.
///
/// [`ariadne_core::probe`] takes the `PATH` as a parameter because the daemon's
/// is not this one; everything on this side of the wire means the environment's.
pub fn on_path(name: &str) -> Option<PathBuf> {
    probe::which(&std::env::var_os("PATH")?, name)
}

/// Append `query` to `base` as a URL-encoded query string. Filters that are
/// `None` are omitted; when nothing remains, `base` is returned untouched
/// (no stray `?`).
pub fn query_path(base: &str, query: &impl serde::Serialize) -> Result<String> {
    let qs = serde_urlencoded::to_string(query)?;
    Ok(match qs.is_empty() {
        true => base.to_string(),
        false => format!("{base}?{qs}"),
    })
}

/// The word every `--model` that takes one writes for "pin nothing at all",
/// which is also how the daemon reads it: `task update --model default` runs
/// the engineer on whatever its profile is on, and `profile update --model
/// default` puts the profile itself back on auto.
pub const DEFAULT: &str = "default";

/// One `--model <agent>[:<model>]` off the command line, as the daemon spells
/// it back: the agent CLI is the choice, and a `:` after it narrows that CLI to
/// one model of it.
///
/// [`ModelRef`] is where the spelling lives, so a typo is refused here in the
/// same words the daemon would have refused it in — and never leaves the shell.
/// The hyphenated spelling of a CLI (`claude-code`) names the same one, and
/// travels as the daemon writes it.
pub fn parse_model(s: &str) -> Result<String, String> {
    s.parse::<ModelRef>().map(|m| m.to_string())
}

/// The same, plus the one word an update takes beside a model: [`DEFAULT`].
pub fn parse_model_or_default(s: &str) -> Result<String, String> {
    if s == DEFAULT {
        return Ok(DEFAULT.to_string());
    }
    parse_model(s).map_err(|e| format!("{e}; or \"{DEFAULT}\" to pin nothing at all"))
}

/// One `--effort <EFFORT>` off the command line: the reasoning effort the
/// pinned model is run at.
///
/// Which efforts a model takes is the model's own business — `ariadne models
/// ls` lists them, and they differ between agent CLIs and between models of
/// one CLI — so the only thing settled here is that an effort was written at
/// all. The daemon knows the model this effort will run at, and refuses one
/// that does not belong to it in words this side could not have written.
pub fn parse_effort(s: &str) -> Result<String, String> {
    match s.trim().is_empty() {
        true => Err(
            "no effort was named — write one of the efforts `ariadne models ls` \
             lists for the model"
                .to_string(),
        ),
        false => Ok(s.to_string()),
    }
}

/// The same, plus the one word an update takes beside an effort: [`DEFAULT`],
/// which runs the model at whatever its agent CLI runs it at.
pub fn parse_effort_or_default(s: &str) -> Result<String, String> {
    if s == DEFAULT {
        return Ok(DEFAULT.to_string());
    }
    parse_effort(s).map_err(|e| format!("{e}; or \"{DEFAULT}\" to pin no effort at all"))
}

/// What a pin says, taken apart: the agent CLI, and the model of it where one
/// was pinned.
///
/// None where nothing is pinned — auto — and also where the string is one this
/// build cannot read, which a reader shows as auto rather than failing on.
pub fn pinned(model: Option<&str>) -> Option<ModelRef> {
    model?.parse().ok()
}

/// One caller-typed value as a single path segment.
///
/// Profiles answer to their name as well as their id, and a name is free text
/// — a profile named `My Reviewer` has a space in it, and a space is not a
/// character a URI may carry: `ariadne profile inspect "My Reviewer"` used
/// to reach the client with it raw and panic on the URI it could not build.
/// Everything outside the unreserved set (RFC 3986 §2.3) is escaped rather
/// than only what is known to hurt, and `/` with it: the value is one
/// whole segment, so a slash inside it is data, never structure.
pub fn path_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0xf) as usize] as char);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_api::sessions::SessionListQuery;
    use ariadne_api::tasks::TaskListQuery;
    use ariadne_core::{AuthorRole, SessionStatus, TaskStatus};

    fn message(recipient: Option<MessageRecipientDto>) -> MessageDto {
        MessageDto {
            id: "01MSG".into(),
            goal_id: "01GOAL".into(),
            task_id: Some("01TASK".into()),
            author_role: AuthorRole::Engineer,
            author_session_id: Some("01SESSION".into()),
            recipient,
            body: "rebased onto main".into(),
            created_at: "not a time".into(),
        }
    }

    fn profile_recipient(id: Option<&str>, name: Option<&str>) -> MessageRecipientDto {
        MessageRecipientDto {
            kind: RecipientKind::Profile,
            profile_id: id.map(str::to_owned),
            profile_name: name.map(str::to_owned),
        }
    }

    /// The addressee reads as the word that would have addressed it, so what a
    /// listing shows is what `--to` takes — and a profile that is gone leaves
    /// its id, which still names somebody.
    #[test]
    fn a_recipient_reads_as_the_name_that_addresses_it() {
        assert_eq!(
            recipient_label(&profile_recipient(Some("01PROF"), Some("Reviewer"))),
            "Reviewer"
        );
        assert_eq!(
            recipient_label(&profile_recipient(Some("01PROF"), None)),
            "01PROF"
        );
        assert_eq!(
            recipient_label(&MessageRecipientDto {
                kind: RecipientKind::User,
                profile_id: None,
                profile_name: None,
            }),
            "user"
        );
    }

    /// An unaddressed message prints exactly as it always did; an addressed
    /// one names its addressee after the author.
    #[test]
    fn only_an_addressed_message_names_a_recipient() {
        assert_eq!(
            message_line(&message(None), false),
            "[not a time] engineer: rebased onto main"
        );
        assert_eq!(
            message_line(
                &message(Some(profile_recipient(Some("01PROF"), Some("Reviewer")))),
                false
            ),
            "[not a time] engineer → Reviewer: rebased onto main"
        );
    }

    /// With colour on, the time and the addressee dim and the role turns
    /// bold; the brackets and the colon stay bare, so the line still reads
    /// the same way at a glance.
    #[test]
    fn a_coloured_message_line_paints_time_role_and_addressee() {
        let addressed = message(Some(profile_recipient(Some("01PROF"), Some("Reviewer"))));
        let painted = message_line(&addressed, true);
        assert!(
            painted.starts_with(&format!(
                "[{}]",
                style::paint(true, style::META, "not a time")
            )),
            "{painted}"
        );
        assert!(
            painted.contains(&style::paint(true, style::TITLE, "engineer")),
            "{painted}"
        );
        assert!(
            painted.contains(&style::paint(true, style::META, " → Reviewer")),
            "{painted}"
        );
        assert!(painted.ends_with(": rebased onto main"), "{painted}");
    }

    /// A client and a daemon of different builds is the cause of the odd 404
    /// and the missing field, so the warning names both versions and the two
    /// commands that fix it.
    #[test]
    fn a_version_mismatch_says_which_two_and_what_to_do() {
        let warning = version_mismatch("0.4.0", "0.3.1");
        assert!(warning.contains("0.4.0"), "{warning}");
        assert!(warning.contains("0.3.1"), "{warning}");
        assert!(warning.contains("ariadne daemon stop"), "{warning}");
        assert!(warning.contains("ariadne daemon start"), "{warning}");
    }

    /// `-y` answers for the caller: nothing is read, and nothing blocks.
    #[test]
    fn yes_skips_the_confirmation() {
        let subject = Subject::new("goal", "Ship the board", "01m15hg1d4j6de91a4amkhsfgt");
        assert!(confirm("delete", &subject, "Delete it?", true).is_ok());
    }

    /// Nobody is there to answer in a pipe or a cron line, so the command
    /// refuses rather than acting: exit 2, and a line naming what it did not
    /// touch and the flag that would have let it.
    #[test]
    fn a_pipe_is_refused_rather_than_taken_for_a_yes() {
        let subject = Subject::new("goal", "Ship the board", "01m15hg1d4j6de91a4amkhsfgt");
        // Cargo runs tests with stdin closed, which is the case this is about.
        let err = confirm("delete", &subject, "Delete it?", false).expect_err("a refusal");
        assert_eq!(
            err.to_string(),
            "refusing to delete goal \"Ship the board\" (01m15hg1d4j6de91a4amkhsfgt) \
             without --yes: stdin is not a terminal"
        );
        assert_eq!(crate::error::exit(&err), crate::error::Exit::Usage);
    }

    /// A question names the short id beside the title: the id a table or the
    /// UI put in front of whoever is about to answer it.
    #[test]
    fn a_subject_is_named_by_its_title_and_its_short_id() {
        let subject = Subject::new("goal", "Ship the board", "01m15hg1d4j6de91a4amkhsfgt");
        assert_eq!(subject.named(), "\"Ship the board\" (…amkhsfgt)");
    }

    /// Filters that were not given leave no trace, and the ones that were are
    /// URL-encoded in the daemon's own spelling.
    #[test]
    fn a_query_carries_only_the_filters_that_were_given() {
        assert_eq!(
            query_path("/v1/tasks", &TaskListQuery::default()).unwrap(),
            "/v1/tasks"
        );
        assert_eq!(
            query_path(
                "/v1/tasks",
                &TaskListQuery {
                    goal: Some("a b&c".into()),
                    status: Some(TaskStatus::UnderReview),
                }
            )
            .unwrap(),
            "/v1/tasks?goal=a+b%26c&status=under_review"
        );
        assert_eq!(
            query_path(
                "/v1/sessions",
                &SessionListQuery {
                    status: Some(SessionStatus::Failed),
                    ..SessionListQuery::default()
                }
            )
            .unwrap(),
            "/v1/sessions?status=failed"
        );
    }

    /// What a model may be run at is the model's own business — the daemon
    /// holds the catalogue and refuses what does not belong — so this side
    /// only insists that an effort was written at all.
    #[test]
    fn an_effort_is_only_checked_for_being_one() {
        assert_eq!(parse_effort("high").as_deref(), Ok("high"));
        assert_eq!(
            parse_effort("ultra").as_deref(),
            Ok("ultra"),
            "an effort no claude model takes still travels: the daemon knows \
             which model it is about to run at"
        );
        assert_eq!(
            parse_effort("gpt-5-codex-high").as_deref(),
            Ok("gpt-5-codex-high"),
            "an opencode variant is an effort like any other"
        );
        let err = parse_effort("   ").expect_err("no effort at all");
        assert!(err.contains("no effort was named"), "{err}");
        assert!(err.contains("ariadne models ls"), "{err}");
    }

    /// The one word an update writes beside an effort, and it is the same word
    /// `--model` takes: the pin goes back to whatever the agent CLI runs the
    /// model at.
    #[test]
    fn an_update_takes_default_beside_an_effort() {
        assert_eq!(parse_effort_or_default("default").as_deref(), Ok("default"));
        assert_eq!(parse_effort_or_default("xhigh").as_deref(), Ok("xhigh"));
        let err = parse_effort_or_default("").expect_err("no effort at all");
        assert!(err.contains("no effort was named"), "{err}");
        assert!(err.contains("\"default\""), "{err}");
    }

    /// A pin reads as the two things it says: what the agent runs on, and —
    /// only where one was pinned — how deeply it reasons there.
    #[test]
    fn a_pinned_label_says_the_model_and_the_effort_beside_it() {
        let profiles = ProfileNames::from_pairs([("01PROF".to_string(), "Reviewer".to_string())]);
        assert_eq!(
            profiles.pinned_label("01PROF", Some("codex:gpt-5.6-luna"), Some("high")),
            "Reviewer (01PROF) · codex:gpt-5.6-luna @ high"
        );
        assert_eq!(
            profiles.pinned_label("01PROF", Some("codex:gpt-5.6-luna"), None),
            "Reviewer (01PROF) · codex:gpt-5.6-luna",
            "no effort pinned is the CLI's own, which is not a choice to print"
        );
        assert_eq!(
            profiles.pinned_label("01PROF", None, Some("max")),
            "Reviewer (01PROF) · the profile's own @ max",
            "an effort stands on its own: the profile's model, run deeper"
        );
        assert_eq!(
            profiles.pinned_label("01PROF", None, None),
            "Reviewer (01PROF) · the profile's own"
        );
    }

    /// Ids pass through untouched; a name — free text, which is what made
    /// this necessary — is escaped whole, so nothing in it reads as structure.
    #[test]
    fn a_path_segment_escapes_everything_that_is_not_unreserved() {
        assert_eq!(
            path_segment("01M0R9EPJK7QYAGYCN31E8EF58"),
            "01M0R9EPJK7QYAGYCN31E8EF58"
        );
        assert_eq!(path_segment("My Reviewer"), "My%20Reviewer");
        assert_eq!(path_segment("../goals/01G"), "..%2Fgoals%2F01G");
        assert_eq!(path_segment("a?b#c"), "a%3Fb%23c");
        assert_eq!(path_segment("Revisor Estrícto"), "Revisor%20Estr%C3%ADcto");
    }
}
