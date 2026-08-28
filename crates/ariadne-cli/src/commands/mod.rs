//! CLI command implementations.

pub mod agent;
pub mod agent_event;
pub mod attach;
pub mod attention;
pub mod doctor;
#[cfg(test)]
pub mod fixtures;
pub mod goal;
pub mod mcp;
pub mod profile;
pub mod repo;
pub mod session;
pub mod setup;
pub mod spawn;
pub mod task;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::json;

use ariadne_api::messages::{MessageDto, MessageRecipientDto};
use ariadne_api::profiles::ProfileDto;
use ariadne_client::{Client, endpoint};
use ariadne_core::models::ModelRef;
use ariadne_core::{AgentKind, RecipientKind, probe};

use crate::output::{Format, local_time, note, print};

/// `ariadne version` — client version always, daemon version when reachable.
///
/// A daemon that did not answer is not a failure of `version` itself, so it
/// stays on stdout — but it is still a line a person reads, so no "client
/// error (Connect)" in it.
pub async fn version(client: &Client, format: Format) -> Result<()> {
    let daemon = client.version().await;
    let payload = json!({
        "client": {"name": "ariadne", "version": env!("CARGO_PKG_VERSION")},
        "daemon": match &daemon {
            Ok(v) => json!({"name": v.name, "version": v.version}),
            Err(e) => json!({"error": e.human()}),
        },
        "endpoint": client.endpoint(),
    });
    print(format, &payload, || {
        println!("client:  ariadne {}", env!("CARGO_PKG_VERSION"));
        match daemon {
            Ok(v) => println!("daemon:  {} {}", v.name, v.version),
            Err(e) => println!("daemon:  {}", e.human()),
        }
    })
}

/// `ariadne daemon status`
///
/// A failure is reported as the client's own error: "daemon not running at X"
/// on top of "cannot reach the ariadne daemon at X" said the endpoint twice.
pub async fn daemon_status(client: &Client, format: Format) -> Result<()> {
    let h = client.health().await?;
    let payload = json!({
        "status": h.status,
        "uptime_secs": h.uptime_secs,
        "endpoint": client.endpoint(),
    });
    print(format, &payload, || {
        println!("status:  {}", h.status);
        println!("uptime:  {}s", h.uptime_secs);
        println!("socket:  {}", client.endpoint());
    })
}

/// `ariadne daemon start` — spawn ariadned detached and wait for it to answer.
///
/// Builds its own client for `home` rather than taking the caller's: the
/// daemon it spawns listens on that home's socket, and `--endpoint` /
/// `ARIADNE_SOCKET` — never passed to ariadned — would send both the
/// already-running check and the readiness poll at a different daemon.
pub async fn daemon_start(home: Option<PathBuf>, format: Format) -> Result<()> {
    let client = Client::for_home(home.clone());
    if client.health().await.is_ok() {
        let payload = json!({"started": false, "endpoint": client.endpoint()});
        return print(format, &payload, || {
            println!("daemon already running at {}", client.endpoint())
        });
    }

    let binary = find_ariadned()?;
    let mut cmd = Command::new(&binary);
    if let Some(home) = &home {
        cmd.arg("--home").arg(home);
    }
    // Daemon output goes to a log file, readable via `ariadne daemon logs`.
    let log_dir = ariadne_home(home.clone());
    std::fs::create_dir_all(&log_dir).ok();
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("ariadned.log"))
        .with_context(|| format!("opening log file in {}", log_dir.display()))?;
    cmd.stdin(Stdio::null())
        .stdout(log.try_clone().context("cloning log handle")?)
        .stderr(log);
    let child = cmd
        .spawn()
        .with_context(|| format!("spawning {}", binary.display()))?;

    // Poll for readiness.
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if client.health().await.is_ok() {
            let payload =
                json!({"started": true, "pid": child.id(), "endpoint": client.endpoint()});
            return print(format, &payload, || {
                println!("ariadned started (pid {})", child.id())
            });
        }
    }
    bail!(
        "ariadned (pid {}) did not answer on {} within 5s",
        child.id(),
        client.endpoint()
    );
}

/// `ariadne daemon stop` — SIGTERM via pidfile.
pub fn daemon_stop(format: Format) -> Result<()> {
    let pid_file = endpoint::pid_file(&ariadne_home(None));
    let pid = std::fs::read_to_string(&pid_file)
        .with_context(|| {
            format!(
                "no pidfile at {} — is the daemon running?",
                pid_file.display()
            )
        })?
        .trim()
        .to_string();

    let status = Command::new("kill")
        .arg(&pid)
        .status()
        .context("running kill")?;
    if !status.success() {
        bail!(
            "kill {pid} failed — stale pidfile? ({})",
            pid_file.display()
        );
    }
    print(format, &json!({"signalled": "SIGTERM", "pid": pid}), || {
        println!("sent SIGTERM to ariadned (pid {pid})")
    })
}

/// `ariadne daemon logs [-f]` — show the daemon log via tail.
pub fn daemon_logs(follow: bool) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let log = ariadne_home(None).join("ariadned.log");
    if !log.is_file() {
        bail!("no daemon log at {}", log.display());
    }
    let mut cmd = Command::new("tail");
    cmd.arg("-n").arg("200");
    if follow {
        cmd.arg("-f");
    }
    let err = cmd.arg(&log).exec();
    bail!("failed to exec tail: {err}");
}

/// Ask before something irreversible, and take silence for "no".
///
/// `yes` (`-y`) answers for the caller, and so does a stdin that is not a
/// terminal: a script has nobody to ask, and a prompt written into a pipe
/// would hang a cron job rather than fail it. Declining is an error, so
/// `ariadne goal cancel x && deploy` does not run the second half.
pub fn confirm(question: &str, yes: bool) -> Result<()> {
    if yes || !std::io::stdin().is_terminal() {
        return Ok(());
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
        _ => bail!("aborted"),
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

/// One conversation message as `goal messages` and `task messages` print it:
/// `[time] role: body`, with the addressee after the author when there is one.
pub fn message_line(message: &MessageDto) -> String {
    let author = match &message.recipient {
        Some(recipient) => format!(
            "{} → {}",
            message.author_role.as_str(),
            recipient_label(recipient)
        ),
        None => message.author_role.as_str().to_string(),
    };
    format!(
        "[{}] {author}: {}",
        local_time(&message.created_at),
        message.body
    )
}

/// A conversation as `goal messages` and `task messages` print it: the
/// daemon's own list for a script, one line per message for a person.
pub fn print_messages(messages: &[MessageDto], format: Format) -> Result<()> {
    print(format, &messages, || {
        for message in messages {
            println!("{}", message_line(message));
        }
        if messages.is_empty() {
            note("no messages yet");
        }
    })
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

    /// `Name (id) · agent · model`: the mention, plus what the agent behind it
    /// is pinned to run on, out of the one string that carries both halves.
    ///
    /// A profile is editable and a pin is not, so the two drift: what a task's
    /// engineer, a task's reviewer or a goal's planner runs on is the snapshot
    /// taken when it was assigned, not what the profile says today. `auto` and
    /// `default` are the same two words `profile inspect` and the web use.
    pub fn pinned_label(&self, id: &str, model: Option<&str>) -> String {
        let pin = pinned(model);
        format!(
            "{} · {} · {}",
            self.label(id),
            pin.as_ref().map_or("auto", |p| p.agent_kind.as_str()),
            pin.as_ref()
                .and_then(|p| p.model.as_deref())
                .unwrap_or("default"),
        )
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

/// One `--agent <kind>` off the command line, in either of its spellings: the
/// wire one the daemon uses (`claude_code`) and the hyphenated one a shell
/// user reaches for (`claude-code`) name the same CLI.
fn parse_agent(s: &str) -> Result<AgentKind, String> {
    s.replace('-', "_").parse()
}

/// What a pin says, taken apart: the agent CLI, and the model of it where one
/// was pinned.
///
/// None where nothing is pinned — auto — and also where the string is one this
/// build cannot read, which a reader shows as auto rather than failing on.
pub fn pinned(model: Option<&str>) -> Option<ModelRef> {
    model?.parse().ok()
}

/// The one `model` a request carries, out of the `--agent` and `--model` the
/// command line still spells apart: `<agent_kind>[:<model>]`.
///
/// The daemon defines that spelling and is what refuses a bad one, so nothing
/// is validated here — an agent this build does not know travels as typed and
/// is named in the refusal that comes back. The words for "nothing chosen"
/// are one word wherever they are typed: `--agent auto`, `--agent default`
/// and a bare `--model default` all ask for the profile's own.
pub fn qualified_model(agent: Option<&str>, model: Option<&str>) -> Option<String> {
    /// What either flag writes to choose nothing at all.
    const NOTHING: [&str; 3] = ["", "default", "auto"];
    match agent {
        // No agent to hang it on: what was typed travels as typed, whether
        // that is a whole `agent:model` or a model the daemon will refuse.
        None => model.map(str::to_string),
        Some(agent) if NOTHING.contains(&agent) => Some("default".to_string()),
        Some(agent) => Some(match model.filter(|m| !NOTHING.contains(m)) {
            Some(model) => format!("{agent}:{model}"),
            None => agent.to_string(),
        }),
    }
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
            message_line(&message(None)),
            "[not a time] engineer: rebased onto main"
        );
        assert_eq!(
            message_line(&message(Some(profile_recipient(
                Some("01PROF"),
                Some("Reviewer")
            )))),
            "[not a time] engineer → Reviewer: rebased onto main"
        );
    }

    /// `-y` answers for the caller: nothing is read, and nothing blocks.
    #[test]
    fn yes_skips_the_confirmation() {
        assert!(confirm("Delete it?", true).is_ok());
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
