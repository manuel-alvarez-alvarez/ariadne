//! `ariadne session ...`

use std::collections::HashMap;

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::agents::{AcpAgentDto, AcpAgentStatus};
use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::{
    AssignOutsideSessionRequest, ConsoleInputRequest, OutsideSessionListQuery,
    OutsideSessionPageDto, SessionDto, SessionListQuery,
};
use ariadne_api::stream::EventStreamQuery;
use ariadne_api::tasks::TaskDto;
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
    print_kv, print_list, short_id, status_line, usage_block, usage_cell, view,
};

/// Columns of `session ls`. `title` is the one written by a human, so it is
/// capped the way `task ls` caps its titles. `attention` is next to `status`
/// because the two are orthogonal: an agent blocked on a permission prompt is
/// still `running`, and the status alone says nothing about it.
///
/// `tokens` is what the session spent, in over an up arrow and out over a
/// down one, with the share of the input the prompt cache served; the counts
/// to the digit are in `session inspect`, since a column is scanned rather
/// than read.
///
/// The worktree and the agent's own internal id are not here: they are what
/// one goes to `session inspect` for, and they cost a lot of a row nobody
/// reads them from.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 40).title(),
    col("status", UNCAPPED).status(),
    col("attention", UNCAPPED).attention().rank(4),
    col("age", UNCAPPED).rank(3),
    col("seat", UNCAPPED).rank(2),
    col("agent", UNCAPPED).rank(1),
    col("tokens", UNCAPPED).rank(0),
];

/// Columns of `session discover`. The internal id is first because it is the
/// value `session adopt` takes and therefore what quiet output must print.
const DISCOVER: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("agent", UNCAPPED).rank(3),
    col("directory", 40).title().rank(2),
    col("activity", UNCAPPED).rank(1),
    col("prompt", 50).rank(0),
];

/// Where a continuation line of `session inspect` starts: [`print_kv`] pads
/// its keys to the longest one — `attention since` — and then two spaces, and
/// a block that spills over several lines lines them all up under the first.
const INDENT: &str = "\n                 ";

/// What `session ls --help` ends with.
const LS_EXAMPLES: &str = "\
Examples:
  ariadne session ls                            # every live session
  ariadne session ls --all --task <task-id>     # that task's, history included
  ariadne session ls --status idle,exited       # named statuses, live or not
  ariadne session ls --goal <goal-id> --seat reviewer
";

#[derive(Subcommand)]
pub enum SessionCommand {
    /// List live agent sessions (docker-style; --all includes history)
    #[command(after_help = LS_EXAMPLES)]
    Ls {
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
        /// Filter by seat, once the rows are here: `GET /v1/sessions` takes
        /// no seat, so this narrows what it answered — as the UI's own seat
        /// filter does. Composes with the rest: it never widens the list
        #[arg(long, value_parser = Spelling::<Seat>::new())]
        seat: Option<Seat>,
        /// Only sessions the daemon has flagged as needing a human: the
        /// same filter the UI's Attention page is built on
        #[arg(long)]
        attention: bool,
        /// Include finished sessions (exited/failed), not just live ones;
        /// nothing to add once --status names one
        #[arg(short, long)]
        all: bool,
        /// Redraw the table whenever a session changes, until Ctrl-C
        #[arg(long)]
        watch: bool,
    },
    /// List agent sessions Ariadne did not start
    Discover {
        /// Filter by registry agent id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_ids))]
        agent: Option<String>,
        /// Filter by absolute working directory and its descendants
        #[arg(long, value_name = "PATH")]
        dir: Option<String>,
        /// Filter by activity at or after an RFC 3339 time or date
        #[arg(long, value_name = "TIME", value_parser = parse_since)]
        since: Option<String>,
        /// Filter by activity at or before an RFC 3339 time or date
        #[arg(long, value_name = "TIME", value_parser = parse_until)]
        until: Option<String>,
        /// Filter by text in the first prompt
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
        /// Fetch every page
        #[arg(long, conflicts_with = "cursor")]
        all: bool,
    },
    /// Resume an outside session as the author of a ready task
    Adopt {
        /// Internal session id from `ariadne session discover`
        session_id: String,
        /// Ready task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        task_id: String,
        /// The agent the session belongs to (`session discover`'s `agent`
        /// column)
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_ids))]
        agent: String,
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

pub async fn run(client: &Client, cmd: SessionCommand, format: Format) -> Result<()> {
    match cmd {
        SessionCommand::Ls {
            task,
            goal,
            statuses,
            seat,
            attention,
            all,
            watch,
        } => {
            ls(
                client, task, goal, statuses, seat, attention, all, watch, format,
            )
            .await?
        }
        SessionCommand::Discover {
            agent,
            dir,
            since,
            until,
            search,
            limit,
            cursor,
            refresh,
            all,
        } => {
            discover(
                client,
                DiscoverOptions {
                    agent,
                    dir,
                    since,
                    until,
                    search,
                    limit,
                    cursor,
                    refresh,
                    all,
                },
                format,
            )
            .await?
        }
        SessionCommand::Adopt {
            session_id,
            task_id,
            agent,
        } => {
            let task_id = resolve::id(client, Kind::Task, &task_id).await?;
            let session: SessionDto = client
                .post_json(
                    &format!("/v1/tasks/{task_id}/author-session"),
                    &AssignOutsideSessionRequest {
                        agent_id: agent,
                        internal_session_id: session_id,
                    },
                )
                .await?;
            print(format, &session, || {
                println!(
                    "{}",
                    status_line(
                        view().color,
                        view().quiet,
                        "session",
                        &session.id,
                        session.status.as_str(),
                    )
                )
            })?;
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
        SessionCommand::Logs { id, follow } => {
            let id = resolve::id(client, Kind::Session, &id).await?;
            crate::commands::console::logs(client, &id, follow, format).await?;
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
struct DiscoverOptions {
    agent: Option<String>,
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
    parse_discovery_time(value, false)
}

fn parse_until(value: &str) -> Result<String, String> {
    parse_discovery_time(value, true)
}

fn parse_discovery_time(value: &str, end_of_day: bool) -> Result<String, String> {
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

fn outside_sessions_path(options: &DiscoverOptions, cursor: Option<&str>) -> Result<String> {
    let follows_all_page = cursor.is_some() && options.cursor.is_none();
    query_path(
        "/v1/outside-sessions",
        &OutsideSessionListQuery {
            agent: options.agent.clone(),
            dir: options.dir.clone(),
            since: options.since.clone(),
            until: options.until.clone(),
            q: options.search.clone(),
            limit: options.limit,
            cursor: cursor
                .map(str::to_string)
                .or_else(|| options.cursor.clone()),
            // Refresh once before an --all traversal. Following pages read
            // that fresh snapshot instead of replacing it each time.
            refresh: (options.refresh && !follows_all_page).then_some(true),
        },
    )
}

async fn fetch_outside_sessions(
    client: &Client,
    options: &DiscoverOptions,
) -> Result<OutsideSessionPageDto> {
    let mut page: OutsideSessionPageDto = client
        .get_json(&outside_sessions_path(options, None)?)
        .await?;
    if !options.all {
        return Ok(page);
    }
    while let Some(cursor) = page.next_cursor.clone() {
        let next: OutsideSessionPageDto = client
            .get_json(&outside_sessions_path(options, Some(&cursor))?)
            .await?;
        page.sessions.extend(next.sessions);
        page.next_cursor = next.next_cursor;
    }
    Ok(page)
}

fn discovery_count(page: &OutsideSessionPageDto) -> String {
    format!("{} of {} sessions", page.sessions.len(), page.total)
}

fn next_discovery_note(page: &OutsideSessionPageDto, options: &DiscoverOptions) -> Option<String> {
    page.next_cursor
        .as_deref()
        .map(|cursor| format!("Next: {}", next_discovery_command(options, cursor)))
}

fn next_discovery_command(options: &DiscoverOptions, cursor: &str) -> String {
    let mut args = vec!["ariadne".to_string(), "session".into(), "discover".into()];
    push_discovery_option(&mut args, "--agent", options.agent.as_deref());
    push_discovery_option(&mut args, "--dir", options.dir.as_deref());
    push_discovery_option(&mut args, "--since", options.since.as_deref());
    push_discovery_option(&mut args, "--until", options.until.as_deref());
    push_discovery_option(&mut args, "--search", options.search.as_deref());
    if let Some(limit) = options.limit {
        push_discovery_option(&mut args, "--limit", Some(&limit.to_string()));
    }
    push_discovery_option(&mut args, "--cursor", Some(cursor));
    args.join(" ")
}

fn push_discovery_option(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
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

/// `session discover`: the stored sessions of every ACP agent that can list
/// them, minus the ones the daemon already owns.
async fn discover(client: &Client, options: DiscoverOptions, format: Format) -> Result<()> {
    let page = fetch_outside_sessions(client, &options).await?;
    if format == Format::Json {
        return crate::output::print_json(&page);
    }
    print_list(
        format,
        &page.sessions,
        DISCOVER,
        |session| {
            vec![
                session.internal_session_id.clone(),
                session.agent_id.clone(),
                session.working_directory.clone(),
                at(Some(&session.last_activity_at)),
                session.first_prompt.clone(),
            ]
        },
        empty_state(
            "No outside sessions found.",
            Some("start an ACP agent that can list its sessions in a project"),
        ),
    )?;
    if !view().quiet {
        println!("{}", discovery_count(&page));
        if let Some(line) = next_discovery_note(&page, &options) {
            note(&line);
        }
    }
    let agents: Vec<AcpAgentDto> = client.get_json("/v1/acp-agents").await?;
    for line in unavailable_acp_agents(&agents) {
        note(&line);
    }
    Ok(())
}

/// Why each ACP agent that cannot list its sessions is missing from the
/// table above: rejected outright, or ready but without the capability.
fn unavailable_acp_agents(agents: &[AcpAgentDto]) -> Vec<String> {
    agents
        .iter()
        .filter(|agent| !agent.capabilities.session_list)
        .map(|agent| {
            let reason = match agent.status {
                AcpAgentStatus::Rejected => agent
                    .rejection_reason
                    .as_deref()
                    .unwrap_or("rejected")
                    .to_string(),
                AcpAgentStatus::Ready => "the agent does not support listing sessions".to_string(),
            };
            format!("{}: adoption unavailable — {reason}", agent.id)
        })
        .collect()
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
#[allow(clippy::too_many_arguments)]
async fn ls(
    client: &Client,
    task: Option<String>,
    goal: Option<String>,
    statuses: Vec<SessionStatus>,
    seat: Option<Seat>,
    attention: bool,
    all: bool,
    watch: bool,
    format: Format,
) -> Result<()> {
    // Resolved once rather than per redraw: what the caller typed names the
    // same goal and task every time round, and a watch is not a new question.
    let goal = match goal {
        Some(goal) => Some(resolve::id(client, Kind::Goal, &goal).await?),
        None => None,
    };
    let task = match task {
        Some(task) => Some(resolve::id(client, Kind::Task, &task).await?),
        None => None,
    };
    if !watch {
        return render(client, goal, task, &statuses, seat, attention, all, format).await;
    }
    let path = watch_path(goal.as_deref())?;
    follow::watch(client, &path, relevant, async || {
        render(
            client,
            goal.clone(),
            task.clone(),
            &statuses,
            seat,
            attention,
            all,
            format,
        )
        .await
    })
    .await
}

/// The table as it stands, read afresh.
#[allow(clippy::too_many_arguments)]
async fn render(
    client: &Client,
    goal: Option<String>,
    task: Option<String>,
    statuses: &[SessionStatus],
    seat: Option<Seat>,
    attention: bool,
    all: bool,
    format: Format,
) -> Result<()> {
    let filtered =
        goal.is_some() || task.is_some() || !statuses.is_empty() || seat.is_some() || attention;
    let query = SessionListQuery {
        goal,
        task,
        status: one_of(statuses),
        // A flag that is not set is not a filter for sessions that want
        // nobody: it is no filter at all.
        attention: attention.then_some(true),
    };
    let sessions: Vec<SessionDto> = client
        .get_json(&query_path("/v1/sessions", &query)?)
        .await?;
    let sessions = visible(sessions, all, statuses, seat);
    let context = match format {
        Format::Table => SessionContext::fetch_for(client, &sessions).await,
        Format::Json => SessionContext::default(),
    };
    let now = chrono::Utc::now();
    print_list(
        format,
        &sessions,
        LS,
        |s| {
            vec![
                s.id.clone(),
                context.label(s),
                s.status.as_str().into(),
                attention_label(s.attention_reason),
                age(&s.created_at, now),
                s.seat.as_str().into(),
                agent_of(&s.model).into(),
                usage_cell(&s.usage),
            ]
        },
        // A named status already says which sessions were asked for, so
        // --all has nothing left to offer.
        match (filtered, all || !statuses.is_empty()) {
            (true, true) => {
                empty_state("No sessions match that filter.", Some("ariadne session ls"))
            }
            (true, false) => empty_state(
                "No live sessions match that filter.",
                Some("ariadne session ls --all"),
            ),
            (false, true) => empty_state("No sessions yet.", Some("ariadne goal create --help")),
            (false, false) => empty_state("No live sessions.", Some("ariadne session ls --all")),
        },
    )
}

/// Which of the sessions the daemon answered with `session ls` shows.
///
/// The default is docker's: live sessions, history behind --all. Named
/// statuses are that same choice made precisely, so they take over —
/// `--status exited` that then dropped every row for not being live would
/// answer nothing. The seat narrows whatever those settled on: `GET
/// /v1/sessions` takes none, so it is applied to the answer rather than asked
/// for, and so is a second status, since it takes only one.
fn visible(
    sessions: Vec<SessionDto>,
    all: bool,
    statuses: &[SessionStatus],
    seat: Option<Seat>,
) -> Vec<SessionDto> {
    sessions
        .into_iter()
        .filter(|s| all || !statuses.is_empty() || s.status.is_live())
        .filter(|s| statuses.is_empty() || statuses.contains(&s.status))
        .filter(|s| seat.is_none_or(|r| s.seat == r))
        .collect()
}

/// The goal and task titles behind a table's sessions: which piece of work
/// each agent was run for, which the ids cannot say. One list call each, and a
/// title is a courtesy — a daemon that will not answer leaves the ids in
/// place.
#[derive(Default)]
struct SessionContext {
    goals: HashMap<String, String>,
    tasks: HashMap<String, String>,
}

impl SessionContext {
    /// The titles for these sessions, or nothing to look up when there are no
    /// sessions — an empty table asks the daemon nothing.
    async fn fetch_for(client: &Client, sessions: &[SessionDto]) -> Self {
        if sessions.is_empty() {
            return Self::default();
        }
        let goals: Vec<GoalDto> = client.get_json("/v1/goals").await.unwrap_or_default();
        let tasks: Vec<TaskDto> = client.get_json("/v1/tasks").await.unwrap_or_default();
        Self {
            goals: goals.into_iter().map(|g| (g.id, g.title)).collect(),
            tasks: tasks.into_iter().map(|t| (t.id, t.title)).collect(),
        }
    }

    /// What one session was run for: its task, or — for an orchestrator
    /// session, which has none — the goal itself, prefixed so a whole goal is
    /// never read as a task of that name. An id stands in for a title the
    /// daemon did not answer with.
    fn label(&self, s: &SessionDto) -> String {
        match &s.task_id {
            Some(task) => self
                .tasks
                .get(task)
                .cloned()
                .unwrap_or_else(|| task.clone()),
            None => format!("goal: {}", self.goals.get(&s.goal_id).unwrap_or(&s.goal_id)),
        }
    }
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
    vec![
        ("id", Kv::id(s.id.clone())),
        ("goal", Kv::id(s.goal_id.clone())),
        ("task", Kv::id(dash(s.task_id.as_deref()))),
        ("seat", s.seat.as_str().into()),
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
        ("activity", Kv::meta(at(s.last_activity_at.as_deref()))),
        ("created", Kv::meta(moment(&s.created_at))),
        ("ended", Kv::meta(at(s.ended_at.as_deref()))),
    ]
}

/// Whose agent it is: a session has no title, and the seat and the piece
/// of work it was spawned for are what stand in for one.
fn what_for(s: &SessionDto) -> String {
    match &s.task_id {
        Some(task) => format!("{} on task {}", s.seat.as_str(), short_id(task)),
        None => format!("{} of goal {}", s.seat.as_str(), short_id(&s.goal_id)),
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

    use ariadne_core::Seat;

    use crate::commands::fixtures::session;
    use crate::output::{View, kv_block, style};

    fn discover_options() -> DiscoverOptions {
        DiscoverOptions {
            agent: None,
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

    #[test]
    fn every_discover_flag_reaches_its_query_parameter() {
        let options = DiscoverOptions {
            agent: Some("codex-acp".into()),
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
            outside_sessions_path(&options, None).unwrap(),
            "/v1/outside-sessions?agent=codex-acp&dir=%2Fwork%2Fapi&since=2026-09-01T00%3A00%3A00Z&until=2026-09-12T12%3A30%3A00%2B02%3A00&q=rate+limit&limit=25&cursor=next%2Fpage&refresh=true"
        );
    }

    #[test]
    fn a_date_is_the_utc_day_boundary_for_discovery() {
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

    fn outside_session(id: &str) -> ariadne_api::sessions::OutsideSessionDto {
        ariadne_api::sessions::OutsideSessionDto {
            agent_id: "codex-acp".into(),
            internal_session_id: id.into(),
            working_directory: "/work/api".into(),
            last_activity_at: "2026-09-12T12:00:00Z".into(),
            first_prompt: format!("Prompt for {id}"),
        }
    }

    fn outside_page(
        ids: &[&str],
        next_cursor: Option<&str>,
        total: usize,
    ) -> OutsideSessionPageDto {
        OutsideSessionPageDto {
            sessions: ids.iter().map(|id| outside_session(id)).collect(),
            next_cursor: next_cursor.map(str::to_string),
            total,
            snapshot_at: "2026-09-12T12:00:00Z".into(),
        }
    }

    async fn outside_api(
        pages: Vec<OutsideSessionPageDto>,
    ) -> (
        Client,
        tokio::task::JoinHandle<()>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        use axum::Router;
        use axum::extract::{RawQuery, State};
        use axum::routing::get;

        #[derive(Clone)]
        struct Api {
            pages:
                std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<OutsideSessionPageDto>>>,
            queries: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        }

        async fn list(
            State(api): State<Api>,
            RawQuery(query): RawQuery,
        ) -> axum::Json<OutsideSessionPageDto> {
            api.queries.lock().unwrap().push(query.unwrap_or_default());
            axum::Json(api.pages.lock().unwrap().pop_front().expect("page"))
        }

        let queries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let api = Api {
            pages: std::sync::Arc::new(std::sync::Mutex::new(pages.into())),
            queries: queries.clone(),
        };
        let app = Router::new()
            .route("/v1/outside-sessions", get(list))
            .with_state(api);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::tcp(format!("http://{address}")), server, queries)
    }

    #[tokio::test]
    async fn all_fetches_every_page_and_keeps_each_session_once() {
        let (client, server, queries) = outside_api(vec![
            outside_page(&["one", "two"], Some("after-two"), 3),
            outside_page(&["three"], None, 3),
        ])
        .await;
        let options = DiscoverOptions {
            agent: Some("codex-acp".into()),
            limit: Some(2),
            refresh: true,
            all: true,
            ..discover_options()
        };

        let page = fetch_outside_sessions(&client, &options).await.unwrap();
        server.abort();

        assert_eq!(
            page.sessions
                .iter()
                .map(|session| session.internal_session_id.as_str())
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
        assert_eq!(page.next_cursor, None);
        assert_eq!(page.total, 3);
        assert_eq!(
            *queries.lock().unwrap(),
            [
                "agent=codex-acp&limit=2&refresh=true",
                "agent=codex-acp&limit=2&cursor=after-two",
            ]
        );
    }

    #[test]
    fn a_next_cursor_prints_the_command_for_the_next_page() {
        let options = DiscoverOptions {
            agent: Some("codex-acp".into()),
            dir: Some("/work/api service".into()),
            search: Some("rate limit".into()),
            limit: Some(25),
            refresh: true,
            ..discover_options()
        };
        let page = outside_page(&["one"], Some("after-one"), 8);

        assert_eq!(
            next_discovery_note(&page, &options).as_deref(),
            Some(
                "Next: ariadne session discover --agent codex-acp --dir '/work/api service' --search 'rate limit' --limit 25 --cursor after-one"
            )
        );
    }

    #[test]
    fn the_last_page_prints_no_next_command() {
        assert_eq!(
            next_discovery_note(&outside_page(&["one"], None, 1), &discover_options()),
            None
        );
    }

    #[test]
    fn the_discovery_count_is_shown_over_the_total() {
        assert_eq!(
            discovery_count(&outside_page(&["one", "two"], Some("more"), 17)),
            "2 of 17 sessions"
        );
    }

    fn context() -> SessionContext {
        SessionContext {
            goals: HashMap::from([("01GOAL".to_string(), "Ship the board".to_string())]),
            tasks: HashMap::from([("01TASK".to_string(), "Wire the screen".to_string())]),
        }
    }

    #[test]
    fn the_session_subject_column_is_title() {
        let table = crate::output::render_table(
            LS,
            &[vec![String::new(); LS.len()]],
            &crate::output::View::plain(),
        )
        .expect("table");
        assert!(
            table
                .lines()
                .next()
                .is_some_and(|line| line.contains("TITLE")),
            "{table}"
        );
        assert!(
            !table
                .lines()
                .next()
                .is_some_and(|line| line.contains("CONTEXT")),
            "{table}"
        );
    }

    #[test]
    fn a_session_on_a_task_is_named_by_the_task() {
        assert_eq!(
            context().label(&session("01SESS", "01GOAL", Some("01TASK"))),
            "Wire the screen"
        );
    }

    /// The orchestrator runs for the goal itself, and the row says so rather
    /// than leaving a goal title where every other row carries a task.
    #[test]
    fn an_orchestrator_session_is_named_by_its_goal() {
        assert_eq!(
            context().label(&session("01SESS", "01GOAL", None)),
            "goal: Ship the board"
        );
    }

    /// Titles are a courtesy — a daemon that would not answer the lists, or a
    /// task created since they were read, still leaves a usable row.
    #[test]
    fn an_unknown_id_stands_in_for_its_title() {
        let empty = SessionContext::default();
        assert_eq!(
            empty.label(&session("01SESS", "01GOAL", Some("01OTHER"))),
            "01OTHER"
        );
        assert_eq!(
            empty.label(&session("01SESS", "01GOAL", None)),
            "goal: 01GOAL"
        );
    }

    /// The `--watch` stream is scoped to the goal alone: the daemon's routing
    /// filter drops a `goal_deleted` event for any subscriber that named a
    /// task, since the event carries no task id of its own, and a watch that
    /// missed it would show a goal's sessions long after the goal — and the
    /// task and its sessions with it — was deleted. Taking no `task` at all
    /// is what keeps that mistake from being made again.
    #[test]
    fn the_watch_stream_is_scoped_to_the_goal_alone_never_the_task() {
        assert_eq!(watch_path(None).unwrap(), "/v1/events/stream");
        assert_eq!(
            watch_path(Some("01GOAL")).unwrap(),
            "/v1/events/stream?goal=01GOAL"
        );
    }

    /// One session per seat and per liveness, as `session ls` receives them
    /// from the daemon: the orchestrator is running, the author has exited.
    fn listed() -> Vec<SessionDto> {
        let author = SessionDto {
            status: SessionStatus::Exited,
            ..session("01ENG", "01GOAL", Some("01TASK"))
        };
        vec![session("01PLAN", "01GOAL", None), author]
    }

    fn ids(sessions: Vec<SessionDto>) -> Vec<String> {
        sessions.into_iter().map(|s| s.id).collect()
    }

    /// The default view is unchanged: live sessions, and history only once
    /// --all or a named --status asks for it.
    #[test]
    fn the_default_view_is_the_live_one() {
        assert_eq!(ids(visible(listed(), false, &[], None)), ["01PLAN"]);
        assert_eq!(ids(visible(listed(), true, &[], None)), ["01PLAN", "01ENG"]);
        assert_eq!(
            ids(visible(listed(), false, &[SessionStatus::Exited], None)),
            ["01ENG"],
            "a named status takes over from the live/finished split"
        );
    }

    /// Several statuses list a session in any of them: `GET /v1/sessions`
    /// takes one, so this is the narrowing the CLI does itself.
    #[test]
    fn several_statuses_list_a_session_in_any_of_them() {
        assert_eq!(
            ids(visible(
                listed(),
                false,
                &[SessionStatus::Running, SessionStatus::Exited],
                None
            )),
            ["01PLAN", "01ENG"]
        );
    }

    /// The seat narrows whatever the rest of the flags settled on, and never
    /// widens it: a finished author stays behind --all even when --seat
    /// names authors.
    #[test]
    fn a_seat_narrows_the_view_it_is_used_with() {
        assert_eq!(
            ids(visible(listed(), false, &[], Some(Seat::Orchestrator))),
            ["01PLAN"]
        );
        assert_eq!(
            ids(visible(listed(), false, &[], Some(Seat::Author))),
            [] as [String; 0],
            "the only author here has exited"
        );
        assert_eq!(
            ids(visible(listed(), true, &[], Some(Seat::Author))),
            ["01ENG"]
        );
        assert_eq!(
            ids(visible(listed(), false, &[], Some(Seat::Reviewer))),
            [] as [String; 0]
        );
    }

    /// An agent this discovery run cannot list sessions for is named with why:
    /// its own rejection reason where discovery rejected it outright, and a
    /// fixed line where it is ready but simply lacks the capability. An agent
    /// that can list sessions is left off the notes entirely.
    #[test]
    fn an_agent_without_the_capability_is_named_with_its_reason() {
        use ariadne_api::agents::{AcpAgentStatus, AcpCapabilitiesDto};

        let agent = |id: &str, status, session_list, rejection_reason: Option<&str>| AcpAgentDto {
            id: id.to_string(),
            command: vec![id.to_string()],
            builtin: false,
            status,
            capabilities: AcpCapabilitiesDto {
                session_list,
                ..Default::default()
            },
            degraded: Vec::new(),
            rejection_reason: rejection_reason.map(str::to_string),
        };
        let agents = [
            agent("listable", AcpAgentStatus::Ready, true, None),
            agent("degraded", AcpAgentStatus::Ready, false, None),
            agent(
                "broken",
                AcpAgentStatus::Rejected,
                false,
                Some("no model option"),
            ),
        ];

        let notes = unavailable_acp_agents(&agents);

        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(
            notes.iter().any(|line| line.contains("degraded")
                && line.contains("does not support listing sessions")),
            "{notes:?}"
        );
        assert!(
            notes
                .iter()
                .any(|line| line.contains("broken") && line.contains("no model option")),
            "{notes:?}"
        );
        assert!(
            !notes.iter().any(|line| line.contains("listable")),
            "{notes:?}"
        );
    }

    /// `ls` and `inspect` spell a reason the way `ariadne attention` does —
    /// which is the UI's wording — and say nothing at all when there is none.
    #[test]
    fn a_session_carries_the_attention_wording_of_the_attention_list() {
        assert_eq!(
            attention_label(Some(AttentionReason::WaitingPermission)),
            "waiting for permission"
        );
        assert_eq!(
            attention_label(Some(AttentionReason::WaitingInput)),
            "waiting for input"
        );
        assert_eq!(
            attention_label(Some(AttentionReason::AgentError)),
            "agent error"
        );
        assert_eq!(
            attention_label(Some(AttentionReason::Disconnected)),
            "disconnected"
        );
        assert_eq!(attention_label(Some(AttentionReason::Stalled)), "stalled");
        assert_eq!(attention_label(None), "-");
    }

    /// `session inspect` types its id, its goal, its task and its status the
    /// way a row of `session ls` would: the ids dimmed, the status carrying
    /// its glyph inside its colour, a waiting attention carrying its own.
    /// Colour is escapes and nothing else — strip them and the block reads
    /// exactly as it does with `--color never`.
    #[test]
    fn the_inspect_block_types_its_ids_status_and_attention() {
        let s = SessionDto {
            attention_reason: Some(AttentionReason::WaitingInput),
            ..session("01SESS", "01GOAL", Some("01TASK"))
        };
        let pairs = inspect_pairs(&s);

        let coloured = kv_block(
            &pairs,
            &View {
                color: true,
                ..View::plain()
            },
        );
        assert!(
            coloured.contains(&style::paint(true, style::ID, &s.id)),
            "{coloured}"
        );
        assert!(
            coloured.contains(&style::paint(true, style::ID, "01TASK")),
            "the task id is dimmed too: {coloured}"
        );
        assert!(
            coloured.contains(&style::paint(true, style::status("running").0, "● running")),
            "{coloured}"
        );
        assert!(
            coloured.contains(&style::paint(
                true,
                style::attention("waiting for input").0,
                "? waiting for input"
            )),
            "{coloured}"
        );

        let plain = kv_block(&pairs, &View::plain());
        assert!(!plain.contains('\u{1b}'), "{plain}");
        assert_eq!(strip_escapes(&coloured), plain, "colour adds only escapes");
    }

    /// The escapes taken back out of a line, the way a reader's terminal
    /// would show it: what is left is what `--color never` prints outright.
    fn strip_escapes(line: &str) -> String {
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
    }
}
