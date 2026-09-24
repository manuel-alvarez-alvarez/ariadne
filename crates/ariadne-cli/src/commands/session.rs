//! `ariadne session ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::sessions::{
    ConsoleInputRequest, SessionDto, SessionEntryDto, SessionKind, SessionPageDto, SessionPageQuery,
};
use ariadne_api::stream::EventStreamQuery;
use ariadne_client::{Client, SseEvent};
use ariadne_core::models::agent_of;
use ariadne_core::{AttentionReason, Seat, SessionStatus};

use super::attention::reason_label;
use super::follow;
use super::resolve::{self, Kind};
use super::{Subject, confirm, one_of, query_path};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, Kv, UNCAPPED, age, at, col, dash, empty_state, moment, note, ok_id_line, print,
    print_kv, print_list, short_id, status_line, tokens, usage_block, usage_cell, view,
};
use ariadne_console::transcript::{Filters, Since};

/// Columns of `session ls`.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 40).title(),
    col("status", UNCAPPED).status(),
    col("goal", UNCAPPED).id().rank(4),
    col("task", UNCAPPED).id().rank(3),
    col("agent", UNCAPPED).rank(2),
    col("age", UNCAPPED).rank(1),
    col("tokens", UNCAPPED).rank(0),
];

const LS_WITH_DIRECTORY: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 40).title(),
    col("status", UNCAPPED).status(),
    col("goal", UNCAPPED).id().rank(4),
    col("task", UNCAPPED).id().rank(3),
    col("agent", UNCAPPED).rank(2),
    col("age", UNCAPPED).rank(1),
    col("tokens", UNCAPPED).rank(0),
    col("directory", 40).rank(5),
];

fn session_columns(columns: &[String]) -> &'static [Column] {
    if columns
        .iter()
        .any(|column| column.eq_ignore_ascii_case("directory"))
    {
        LS_WITH_DIRECTORY
    } else {
        LS
    }
}

/// Where a continuation line of `session inspect` starts: [`print_kv`] pads
/// its keys to the longest one — `attention since` — and then two spaces, and
/// a block that spills over several lines lines them all up under the first.
const INDENT: &str = "\n                 ";

/// What `session ls --help` ends with.
const LS_EXAMPLES: &str = "\
Examples:
  ariadne session ls                                  # recent live and outside sessions
  ariadne session ls --all --task <task-id>           # that task's history
  ariadne session ls --kind outside --agent codex-acp # stored agent sessions
  ariadne session ls --cursor <token>
";

#[derive(Subcommand)]
pub(crate) enum SessionCommand {
    /// List Ariadne and outside agent sessions
    #[command(after_help = LS_EXAMPLES)]
    Ls {
        /// Only Ariadne or outside sessions
        #[arg(long, value_parser = ["ariadne", "outside"])]
        kind: Option<String>,
        /// Filter by registry agent id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_ids))]
        agent: Option<String>,
        /// Filter by task id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        task: Option<String>,
        /// Filter by goal id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        goal: Option<String>,
        /// Filter by status: names the statuses to list instead of the
        /// live/finished split --all makes, and so replaces it — a status is
        /// listed whether or not it is a live one. Repeatable and
        /// comma-separated
        #[arg(long = "status", value_parser = Spelling::<SessionStatus>::new(), value_delimiter = ',')]
        statuses: Vec<SessionStatus>,
        /// Filter by seat
        #[arg(long, value_parser = Spelling::<Seat>::new())]
        seat: Option<Seat>,
        /// Only sessions the daemon has flagged as needing a human: the
        /// same filter the UI's Attention page is built on
        #[arg(long)]
        attention: bool,
        /// Filter by absolute working directory and its descendants
        #[arg(long, value_name = "PATH")]
        dir: Option<String>,
        /// Filter by activity at or after an RFC 3339 time or date
        #[arg(long, value_name = "TIME", value_parser = parse_since)]
        since: Option<String>,
        /// Filter by activity at or before an RFC 3339 time or date
        #[arg(long, value_name = "TIME", value_parser = parse_until)]
        until: Option<String>,
        /// Filter by text in the session title
        #[arg(long, value_name = "TEXT")]
        search: Option<String>,
        /// Maximum sessions in one page (default 50, maximum 200)
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        /// Continue from a previous page
        #[arg(long, value_name = "TOKEN")]
        cursor: Option<String>,
        /// Refresh the daemon's session snapshot before listing
        #[arg(long)]
        refresh: bool,
        /// Fetch every page, including ended Ariadne sessions
        #[arg(short, long, conflicts_with = "cursor")]
        all: bool,
        /// Redraw the table whenever a session changes, until Ctrl-C
        #[arg(long)]
        watch: bool,
    },
    /// Show a session
    Inspect {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
    },
    /// Send a line to a live session, as the UI's console does
    ///
    /// While the agent waits on a permission request, the line answers it:
    /// an option's id or name. Otherwise it is the agent's next prompt, sent
    /// at once or queued behind the turn it is in.
    Send {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
        /// What to send
        text: String,
    },
    /// Show a session's transcript
    Logs {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
        /// Keep printing output until the session ends
        #[arg(short, long)]
        follow: bool,
        /// Print only the last N transcript items
        #[arg(long, value_name = "N")]
        tail: Option<usize>,
        /// Print items at or after an RFC 3339 time, or from a duration ago
        #[arg(long, value_name = "TIME")]
        since: Option<Since>,
        /// Print only this event kind; repeatable
        #[arg(id = "kind", long = "kind", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::transcript_kinds))]
        kinds: Vec<String>,
    },
    /// Revive an ended session: new agent process, same conversation
    Resume {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::ended_session_ids))]
        id: String,
    },
    /// Kill a session's agent process
    Kill {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::live_session_ids))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

pub(crate) async fn run(client: &Client, cmd: SessionCommand, format: Format) -> Result<()> {
    match cmd {
        SessionCommand::Ls {
            kind,
            agent,
            task,
            goal,
            statuses,
            seat,
            attention,
            dir,
            since,
            until,
            search,
            limit,
            cursor,
            refresh,
            all,
            watch,
        } => {
            ls(
                client,
                ListOptions {
                    kind,
                    agent,
                    task,
                    goal,
                    statuses,
                    seat,
                    attention,
                    dir,
                    since,
                    until,
                    search,
                    limit,
                    cursor,
                    refresh,
                    all,
                },
                watch,
                format,
            )
            .await?
        }
        SessionCommand::Inspect { id } => {
            let id = resolve::id(client, Kind::Session, &id).await?;
            let s: SessionDto = client.get_json(&session_path(&id)).await?;
            print(format, &s, || print_kv(&inspect_pairs(&s)))?;
        }
        SessionCommand::Send { id, text } => {
            let id = resolve::id(client, Kind::Session, &id).await?;
            client
                .send_no_content(
                    http::Method::POST,
                    &format!("/v1/sessions/{id}/console/input"),
                    Some(&ConsoleInputRequest { text }),
                )
                .await?;
            print(
                format,
                &serde_json::json!({"sent": true, "session": id}),
                || println!("{}", ok_id_line(view().color, view().quiet, "sent", &id)),
            )?;
        }
        SessionCommand::Logs {
            id,
            follow,
            tail,
            since,
            kinds,
        } => {
            let id = resolve::id(client, Kind::Session, &id).await?;
            crate::commands::console::logs(
                client,
                &id,
                follow,
                Filters { tail, since, kinds },
                format,
            )
            .await?;
        }
        SessionCommand::Resume { id } => {
            let id = resolve::id(client, Kind::Session, &id).await?;
            // The daemon answers with this same session either way: relaunched
            // when it really resumed it, or untouched when its agent turned out
            // to be alive already. What the row said before the call is what
            // tells a relaunch from a session that never needed one.
            let before: SessionDto = client.get_json(&session_path(&id)).await?;
            let s: SessionDto = client
                .post_empty(&format!("/v1/sessions/{id}/resume"))
                .await?;
            let resumed = !before.status.is_live() && s.status.is_live();
            print(
                format,
                &serde_json::json!({"resumed": resumed, "session": s}),
                || {
                    println!(
                        "{}",
                        status_line(
                            view().color,
                            view().quiet,
                            "session",
                            &s.id,
                            s.status.as_str(),
                        )
                    )
                },
            )?;
        }
        SessionCommand::Kill { id, yes } => {
            let id = resolve::id(client, Kind::Session, &id).await?;
            let s: SessionDto = client.get_json(&session_path(&id)).await?;
            let subject = Subject::new("session", what_for(&s), &s.id);
            confirm("kill", &subject, &kill_question(&s, &subject), yes)?;
            let s: SessionDto = client
                .post_empty(&format!("/v1/sessions/{id}/kill"))
                .await?;
            print(format, &s, || {
                println!(
                    "{}",
                    status_line(
                        view().color,
                        view().quiet,
                        "session",
                        &s.id,
                        s.status.as_str(),
                    )
                )
            })?;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct ListOptions {
    kind: Option<String>,
    agent: Option<String>,
    task: Option<String>,
    goal: Option<String>,
    statuses: Vec<SessionStatus>,
    seat: Option<Seat>,
    attention: bool,
    dir: Option<String>,
    since: Option<String>,
    until: Option<String>,
    search: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
    refresh: bool,
    all: bool,
}

fn parse_since(value: &str) -> Result<String, String> {
    parse_day_bound(value, false)
}

fn parse_until(value: &str) -> Result<String, String> {
    parse_day_bound(value, true)
}

fn parse_day_bound(value: &str, end_of_day: bool) -> Result<String, String> {
    if chrono::DateTime::parse_from_rfc3339(value).is_ok() {
        return Ok(value.to_string());
    }
    let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "must be RFC 3339 or YYYY-MM-DD".to_string())?;
    let time = match end_of_day {
        true => date.and_hms_nano_opt(23, 59, 59, 999_999_999),
        false => date.and_hms_opt(0, 0, 0),
    }
    .expect("a calendar day has these times")
    .and_utc();
    Ok(time.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
}

fn session_kind(kind: Option<&str>) -> Option<SessionKind> {
    match kind {
        Some("ariadne") => Some(SessionKind::Ariadne),
        Some("outside") => Some(SessionKind::Outside),
        None => None,
        Some(_) => unreachable!("clap validates session kinds"),
    }
}

fn sessions_path(options: &ListOptions, cursor: Option<&str>) -> Result<String> {
    let follows_all_page = cursor.is_some() && options.cursor.is_none();
    query_path(
        "/v1/sessions",
        &SessionPageQuery {
            kind: session_kind(options.kind.as_deref()),
            agent: options.agent.clone(),
            goal: options.goal.clone(),
            task: options.task.clone(),
            status: one_of(&options.statuses),
            seat: options.seat,
            attention: options.attention.then_some(true),
            dir: options.dir.clone(),
            since: options.since.clone(),
            until: options.until.clone(),
            q: options.search.clone(),
            all: (options.all || options.statuses.len() > 1).then_some(true),
            limit: options.limit,
            cursor: cursor
                .map(str::to_string)
                .or_else(|| options.cursor.clone()),
            refresh: (options.refresh && !follows_all_page).then_some(true),
        },
    )
}

async fn fetch_sessions(client: &Client, options: &ListOptions) -> Result<SessionPageDto> {
    let mut page: SessionPageDto = client.get_json(&sessions_path(options, None)?).await?;
    let needs_status_filter = options.statuses.len() > 1;
    if !options.all && !needs_status_filter {
        return Ok(page);
    }
    while let Some(cursor) = page.next_cursor.clone() {
        let next: SessionPageDto = client
            .get_json(&sessions_path(options, Some(&cursor))?)
            .await?;
        page.sessions.extend(next.sessions);
        page.next_cursor = next.next_cursor;
    }
    if needs_status_filter {
        narrow_statuses(&mut page, &options.statuses);
    }
    Ok(page)
}

fn narrow_statuses(page: &mut SessionPageDto, statuses: &[SessionStatus]) {
    page.sessions.retain(|session| {
        session
            .status
            .is_some_and(|status| statuses.contains(&status))
    });
    page.total = page.sessions.len();
}

fn session_count(page: &SessionPageDto) -> String {
    format!("{} of {} sessions", page.sessions.len(), page.total)
}

fn next_session_note(page: &SessionPageDto, options: &ListOptions) -> Option<String> {
    page.next_cursor
        .as_deref()
        .map(|cursor| format!("Next: {}", next_session_command(options, cursor)))
}

fn next_session_command(options: &ListOptions, cursor: &str) -> String {
    let mut args = vec!["ariadne".to_string(), "session".into(), "ls".into()];
    push_option(&mut args, "--kind", options.kind.as_deref());
    push_option(&mut args, "--agent", options.agent.as_deref());
    push_option(&mut args, "--goal", options.goal.as_deref());
    push_option(&mut args, "--task", options.task.as_deref());
    if let Some(status) = one_of(&options.statuses) {
        push_option(&mut args, "--status", Some(status.as_str()));
    }
    if let Some(seat) = options.seat {
        push_option(&mut args, "--seat", Some(seat.as_str()));
    }
    if options.attention {
        args.push("--attention".into());
    }
    push_option(&mut args, "--dir", options.dir.as_deref());
    push_option(&mut args, "--since", options.since.as_deref());
    push_option(&mut args, "--until", options.until.as_deref());
    push_option(&mut args, "--search", options.search.as_deref());
    if let Some(limit) = options.limit {
        push_option(&mut args, "--limit", Some(&limit.to_string()));
    }
    push_option(&mut args, "--cursor", Some(cursor));
    args.join(" ")
}

fn push_option(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        args.push(flag.to_string());
        args.push(shell_arg(value));
    }
}

fn shell_arg(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._/:@+".contains(c))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn session_row(session: &SessionEntryDto, now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    vec![
        session.id.clone(),
        session.title.clone().unwrap_or_default(),
        session
            .status
            .map_or_else(String::new, |status| status.as_str().into()),
        session.goal_id.clone().unwrap_or_default(),
        session.task_id.clone().unwrap_or_default(),
        session.agent_id.clone(),
        session
            .last_activity_at
            .as_deref()
            .or(session.created_at.as_deref())
            .map_or_else(String::new, |at| age(at, now)),
        session.usage.as_ref().map_or_else(String::new, usage_cell),
        session.working_directory.clone().unwrap_or_default(),
    ]
}

async fn render_page(client: &Client, options: &ListOptions, format: Format) -> Result<()> {
    let page = fetch_sessions(client, options).await?;
    if format == Format::Json {
        return crate::output::print_json(&page);
    }
    print_list(
        format,
        &page.sessions,
        session_columns(&view().columns),
        |session| session_row(session, chrono::Utc::now()),
        empty_state(
            "No sessions match that filter.",
            Some("ariadne session ls --all"),
        ),
    )?;
    if !view().quiet {
        println!("{}", session_count(&page));
        if let Some(line) = next_session_note(&page, options) {
            note(&line);
        }
    }
    Ok(())
}

fn session_path(id: &str) -> String {
    format!("/v1/sessions/{id}")
}

/// The events that change what `session ls` shows. A goal going takes its
/// sessions with it, which no `session_*` event says.
fn relevant(frame: &SseEvent) -> bool {
    matches!(
        frame.event.as_str(),
        "session_created" | "session_updated" | "goal_deleted"
    )
}

/// Where a `session ls --watch` subscribes: scoped to the goal alone, never
/// to the task, even though `session ls` itself also takes `--task`.
///
/// `BusEvent::matches` in the daemon requires a task id to equal the
/// filter's, and `goal_deleted` carries none — so a stream asked for one
/// task would never hear that the goal it belongs to, and so the task and
/// its sessions, is gone. The goal alone still keeps a watch from being
/// woken by every other goal in the system; narrowing to the one task is
/// `render`'s job, on what the stream only signalled had changed.
fn watch_path(goal: Option<&str>) -> Result<String> {
    query_path(
        "/v1/events/stream",
        &EventStreamQuery {
            goal: goal.map(str::to_string),
            task: None,
        },
    )
}

/// `session ls [--watch]`: the table, and with `--watch` the table again
/// every time a session changes.
async fn ls(client: &Client, mut options: ListOptions, watch: bool, format: Format) -> Result<()> {
    // Resolved once rather than per redraw: what the caller typed names the
    // same goal and task every time round, and a watch is not a new question.
    options.goal = match options.goal {
        Some(goal) => Some(resolve::id(client, Kind::Goal, &goal).await?),
        None => None,
    };
    options.task = match options.task {
        Some(task) => Some(resolve::id(client, Kind::Task, &task).await?),
        None => None,
    };
    if !watch {
        return render_page(client, &options, format).await;
    }
    let path = watch_path(options.goal.as_deref())?;
    follow::watch(client, &path, relevant, async || {
        render_page(client, &options, format).await
    })
    .await
}

/// Why this session wants the user, in `ariadne attention`'s own words — and
/// `-` when it does not.
fn attention_label(reason: Option<AttentionReason>) -> String {
    reason.map_or("-".into(), |r| reason_label(r).to_string())
}

/// The key/value pairs `session inspect` prints, in the order it prints them
/// — pulled out of the `Inspect` arm so the block's own content is testable
/// without a daemon behind it.
fn inspect_pairs(s: &SessionDto) -> Vec<(&'static str, Kv)> {
    let mut pairs = vec![
        ("id", Kv::id(s.id.clone())),
        ("goal", Kv::id(dash(s.goal_id.as_deref()))),
        ("task", Kv::id(dash(s.task_id.as_deref()))),
        ("seat", s.seat.map_or("-", |seat| seat.as_str()).into()),
        ("agent id", Kv::id(dash(s.task_agent_id.as_deref()))),
        ("agent", agent_of(&s.model).into()),
        // Recorded at launch, so it is what this session runs on even if the
        // agent has been re-pinned since.
        ("model", s.model.clone().into()),
        // How deeply it reasons there, recorded with the model it belongs
        // to; `default` is whatever the agent runs that model at.
        (
            "effort",
            s.effort.clone().unwrap_or_else(|| "default".into()).into(),
        ),
        ("status", Kv::status(s.status.as_str())),
        (
            "attention",
            Kv::attention(attention_label(s.attention_reason)),
        ),
        (
            "attention since",
            Kv::meta(at(s.attention_since.as_deref())),
        ),
        ("worktree", dash(s.worktree_path.as_deref()).into()),
        ("internal id", dash(s.internal_session_id.as_deref()).into()),
        ("tokens", usage_block(&s.usage, &[], INDENT).into()),
    ];
    if let Some((used, size)) = s.context_used.zip(s.context_size) {
        pairs.push((
            "context",
            format!("{} / {}", tokens(used), tokens(size)).into(),
        ));
    }
    pairs.extend([
        ("activity", Kv::meta(at(s.last_activity_at.as_deref()))),
        ("created", Kv::meta(moment(&s.created_at))),
        ("ended", Kv::meta(at(s.ended_at.as_deref()))),
    ]);
    pairs
}

/// Whose agent it is: a session has no title, and the seat and the piece
/// of work it was spawned for are what stand in for one.
fn what_for(s: &SessionDto) -> String {
    match &s.task_id {
        Some(task) => format!(
            "{} on task {}",
            s.seat.map_or("-", |seat| seat.as_str()),
            short_id(task)
        ),
        None => format!(
            "{} of goal {}",
            s.seat.map_or("-", |seat| seat.as_str()),
            s.goal_id
                .as_deref()
                .map(short_id)
                .unwrap_or_else(|| "-".into())
        ),
    }
}

/// What `session kill` asks: a live agent is about to be stopped, and
/// the id alone does not say whose.
fn kill_question(s: &SessionDto, subject: &Subject) -> String {
    format!(
        "Kill the {} session {}?",
        s.status.as_str(),
        subject.named()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::commands::fixtures::session;
    use crate::output::{View, kv_block};

    fn options() -> ListOptions {
        ListOptions {
            kind: None,
            agent: None,
            task: None,
            goal: None,
            statuses: Vec::new(),
            seat: None,
            attention: false,
            dir: None,
            since: None,
            until: None,
            search: None,
            limit: None,
            cursor: None,
            refresh: false,
            all: false,
        }
    }

    fn outside(id: &str) -> SessionEntryDto {
        SessionEntryDto {
            kind: SessionKind::Outside,
            id: id.into(),
            agent_id: "codex-acp".into(),
            title: Some(format!("Prompt for {id}")),
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: None,
            effort: None,
            internal_session_id: Some(id.into()),
            working_directory: Some("/work/api".into()),
            status: None,
            attention_reason: None,
            attention_since: None,
            last_activity_at: Some("2026-09-12T12:00:00Z".into()),
            usage: None,
            context_used: None,
            context_size: None,
            created_at: None,
            ended_at: None,
        }
    }

    fn page(ids: &[&str], next_cursor: Option<&str>, total: usize) -> SessionPageDto {
        SessionPageDto {
            sessions: ids.iter().map(|id| outside(id)).collect(),
            next_cursor: next_cursor.map(str::to_string),
            total,
            snapshot_at: "2026-09-12T12:00:00Z".into(),
        }
    }

    fn ariadne(id: &str, status: SessionStatus) -> SessionEntryDto {
        let mut session = outside(id);
        session.kind = SessionKind::Ariadne;
        session.status = Some(status);
        session.model = Some("codex-acp:model".into());
        session.usage = Some(Default::default());
        session.created_at = Some("2026-09-12T12:00:00Z".into());
        session
    }

    #[test]
    fn every_session_flag_reaches_its_query_parameter() {
        let options = ListOptions {
            kind: Some("outside".into()),
            agent: Some("codex-acp".into()),
            goal: Some("01GOAL".into()),
            task: Some("01TASK".into()),
            statuses: vec![SessionStatus::Idle],
            seat: Some(Seat::Author),
            attention: true,
            dir: Some("/work/api".into()),
            since: Some("2026-09-01T00:00:00Z".into()),
            until: Some("2026-09-12T12:30:00+02:00".into()),
            search: Some("rate limit".into()),
            limit: Some(25),
            cursor: Some("next/page".into()),
            refresh: true,
            all: false,
        };
        assert_eq!(
            sessions_path(&options, None).unwrap(),
            "/v1/sessions?kind=outside&agent=codex-acp&goal=01GOAL&task=01TASK&status=idle&seat=author&attention=true&dir=%2Fwork%2Fapi&since=2026-09-01T00%3A00%3A00Z&until=2026-09-12T12%3A30%3A00%2B02%3A00&q=rate+limit&limit=25&cursor=next%2Fpage&refresh=true"
        );
    }

    #[test]
    fn a_date_is_the_utc_day_boundary_for_session_listing() {
        assert_eq!(parse_since("2026-09-12").unwrap(), "2026-09-12T00:00:00Z");
        assert_eq!(
            parse_until("2026-09-12").unwrap(),
            "2026-09-12T23:59:59.999999999Z"
        );
        assert_eq!(
            parse_since("2026-09-12T09:30:00+02:00").unwrap(),
            "2026-09-12T09:30:00+02:00"
        );
        assert!(parse_until("12/09/2026").is_err());
    }

    #[test]
    fn several_statuses_narrow_the_combined_page() {
        let mut page = SessionPageDto {
            sessions: vec![
                ariadne("running", SessionStatus::Running),
                ariadne("idle", SessionStatus::Idle),
                ariadne("exited", SessionStatus::Exited),
                outside("outside"),
            ],
            next_cursor: None,
            total: 4,
            snapshot_at: "2026-09-12T12:00:00Z".into(),
        };

        narrow_statuses(&mut page, &[SessionStatus::Idle, SessionStatus::Exited]);

        assert_eq!(
            page.sessions
                .iter()
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>(),
            ["idle", "exited"]
        );
        assert_eq!(page.total, 2);
    }

    #[tokio::test]
    async fn all_fetches_every_page_and_keeps_each_session_once() {
        use axum::Router;
        use axum::extract::{RawQuery, State};
        use axum::routing::get;

        #[derive(Clone)]
        struct Api {
            pages: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<SessionPageDto>>>,
            queries: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        }
        async fn list(
            State(api): State<Api>,
            RawQuery(query): RawQuery,
        ) -> axum::Json<SessionPageDto> {
            api.queries.lock().unwrap().push(query.unwrap_or_default());
            axum::Json(api.pages.lock().unwrap().pop_front().expect("page"))
        }

        let queries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let api = Api {
            pages: std::sync::Arc::new(std::sync::Mutex::new(
                vec![
                    page(&["one", "two"], Some("after-two"), 3),
                    page(&["three"], None, 3),
                ]
                .into(),
            )),
            queries: queries.clone(),
        };
        let app = Router::new()
            .route("/v1/sessions", get(list))
            .with_state(api);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Client::tcp(format!("http://{address}"));
        let options = ListOptions {
            agent: Some("codex-acp".into()),
            limit: Some(2),
            refresh: true,
            all: true,
            ..options()
        };
        let page = fetch_sessions(&client, &options).await.unwrap();
        server.abort();

        assert_eq!(
            page.sessions
                .iter()
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
        assert_eq!(page.next_cursor, None);
        assert_eq!(page.total, 3);
        assert_eq!(
            *queries.lock().unwrap(),
            [
                "agent=codex-acp&all=true&limit=2&refresh=true",
                "agent=codex-acp&all=true&limit=2&cursor=after-two"
            ]
        );
    }

    #[test]
    fn a_next_cursor_prints_the_session_command_for_the_next_page() {
        let options = ListOptions {
            kind: Some("outside".into()),
            agent: Some("codex-acp".into()),
            dir: Some("/work/api service".into()),
            search: Some("rate limit".into()),
            limit: Some(25),
            refresh: true,
            ..options()
        };
        assert_eq!(
            next_session_note(&page(&["one"], Some("after-one"), 8), &options).as_deref(),
            Some(
                "Next: ariadne session ls --kind outside --agent codex-acp --dir '/work/api service' --search 'rate limit' --limit 25 --cursor after-one"
            )
        );
        assert_eq!(next_session_note(&page(&["one"], None, 1), &options), None);
        assert_eq!(
            session_count(&page(&["one", "two"], Some("more"), 17)),
            "2 of 17 sessions"
        );
    }

    #[test]
    fn the_session_table_has_the_unified_columns_and_empty_outside_fields() {
        let table = crate::output::render_table(
            LS,
            &[session_row(&outside("outside-id"), chrono::Utc::now())],
            &View::plain(),
        )
        .unwrap();
        let header: Vec<_> = table.lines().next().unwrap().split_whitespace().collect();
        assert_eq!(
            header[..8],
            [
                "ID", "TITLE", "STATUS", "GOAL", "TASK", "AGENT", "AGE", "TOKENS"
            ]
        );
        assert_eq!(
            session_row(&outside("outside-id"), chrono::Utc::now())[2..5],
            ["", "", ""]
        );
    }

    #[test]
    fn directory_is_available_as_a_session_column() {
        assert_eq!(session_columns(&[]).len(), LS.len());
        assert_eq!(
            session_columns(&["directory".into()])
                .last()
                .map(|column| column.header),
            Some("directory")
        );
    }

    #[test]
    fn the_watch_stream_is_scoped_to_the_goal_alone_never_the_task() {
        assert_eq!(watch_path(None).unwrap(), "/v1/events/stream");
        assert_eq!(
            watch_path(Some("01GOAL")).unwrap(),
            "/v1/events/stream?goal=01GOAL"
        );
    }

    #[test]
    fn a_loose_session_prints_dashes_for_missing_fields() {
        let loose = SessionDto {
            goal_id: None,
            task_id: None,
            seat: None,
            ..session("01LOOSE", "01GOAL", None)
        };
        let block = kv_block(&inspect_pairs(&loose), &View::plain());
        for field in ["goal", "task", "seat"] {
            let line = block.lines().find(|line| line.starts_with(field)).unwrap();
            assert_eq!(line.split_whitespace().collect::<Vec<_>>(), [field, "-"]);
        }
    }

    #[test]
    fn the_inspect_block_shows_the_reported_context_window() {
        let s = SessionDto {
            context_used: Some(20_713),
            context_size: Some(1_000_000),
            ..session("01SESS", "01GOAL", Some("01TASK"))
        };
        assert!(kv_block(&inspect_pairs(&s), &View::plain()).contains("21k / 1M"));
    }
}
