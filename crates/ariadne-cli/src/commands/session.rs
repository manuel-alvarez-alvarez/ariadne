//! `ariadne session ...`

use std::collections::HashMap;

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::{SessionDto, SessionInputRequest, SessionListQuery};
use ariadne_api::tasks::TaskDto;
use ariadne_client::Client;
use ariadne_core::{AttentionReason, Role, SessionStatus};

use super::attention::reason_label;
use super::{ProfileNames, confirm, one_of, query_path};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, UNCAPPED, age, at, col, dash, moment, pager, print, print_json, print_kv,
    print_list, usage_block, usage_cell,
};

/// Columns of `session ls`. `context` is the one written by a human, so it is
/// capped the way `task ls` caps its titles. `attention` is next to `status`
/// because the two are orthogonal: an agent blocked on a permission prompt is
/// still `running`, and the status alone says nothing about it.
///
/// `tokens` is what the session spent, in over an up arrow and out over a
/// down one, with the share of the input the prompt cache served; the counts
/// to the digit are in `session inspect`, since a column is scanned rather
/// than read.
///
/// The tmux session and the agent's own internal id are not here: they are
/// what one goes to `session inspect` for, and they cost eight columns each
/// of a row nobody reads them from.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("context", 40).title(),
    col("status", UNCAPPED).status(),
    col("attention", UNCAPPED).attention().rank(4),
    col("age", UNCAPPED).rank(3),
    col("role", UNCAPPED).rank(2),
    col("agent", UNCAPPED).rank(1),
    col("tokens", UNCAPPED).rank(0),
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
  ariadne session ls --goal <goal-id> --role reviewer
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
        /// Filter by role, once the rows are here: `GET /v1/sessions` takes
        /// no role, so this narrows what it answered — as the UI's own role
        /// filter does. Composes with the rest: it never widens the list
        #[arg(long, value_parser = Spelling::<Role>::new())]
        role: Option<Role>,
        /// Only sessions the daemon has flagged as needing a human: the
        /// same filter the UI's Attention page is built on
        #[arg(long)]
        attention: bool,
        /// Include finished sessions (exited/failed), not just live ones;
        /// nothing to add once --status names one
        #[arg(short, long)]
        all: bool,
    },
    /// Show a session
    Inspect {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
    },
    /// Type into a live session, as the UI's terminal panel does
    ///
    /// The text is typed into the agent's pane and submitted, which is what
    /// answering a question or a permission prompt from the terminal looks
    /// like. `--no-newline` leaves it in the prompt unsent.
    Send {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
        /// What to type
        text: String,
        /// Type the text without submitting it
        #[arg(long)]
        no_newline: bool,
    },
    /// Show recent terminal output of a session
    Logs {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
    },
    /// Revive an ended session: new tmux, same agent conversation
    Resume {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::ended_session_ids))]
        id: String,
    },
    /// Kill a session's tmux process
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
            role,
            attention,
            all,
        } => {
            let filtered = goal.is_some()
                || task.is_some()
                || !statuses.is_empty()
                || role.is_some()
                || attention;
            let query = SessionListQuery {
                goal,
                task,
                status: one_of(&statuses),
                // A flag that is not set is not a filter for sessions that
                // want nobody: it is no filter at all.
                attention: attention.then_some(true),
            };
            let sessions: Vec<SessionDto> = client
                .get_json(&query_path("/v1/sessions", &query)?)
                .await?;
            let sessions = visible(sessions, all, &statuses, role);
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
                        s.role.as_str().into(),
                        s.agent_kind.as_str().into(),
                        usage_cell(&s.usage),
                    ]
                },
                // A named status already says which sessions were asked for,
                // so --all has nothing left to offer.
                match (filtered, all || !statuses.is_empty()) {
                    (true, true) => "no sessions match that filter",
                    (true, false) => {
                        "no live sessions match that filter — finished ones are behind --all"
                    }
                    (false, true) => "no sessions yet",
                    (false, false) => "no live sessions — finished ones are behind --all",
                },
            )?;
        }
        SessionCommand::Inspect { id } => {
            let s: SessionDto = client.get_json(&session_path(&id)).await?;
            let profiles = ProfileNames::for_format(client, format).await;
            print(format, &s, || {
                print_kv(&[
                    ("id", s.id.clone()),
                    ("goal", s.goal_id.clone()),
                    ("task", dash(s.task_id.as_deref())),
                    ("role", s.role.as_str().into()),
                    ("profile", profiles.label(&s.profile_id)),
                    ("agent", s.agent_kind.as_str().into()),
                    // Recorded at launch, so it is what this session runs on
                    // even if the profile has moved on since.
                    ("model", s.model.clone().unwrap_or_else(|| "default".into())),
                    ("status", s.status.as_str().into()),
                    ("attention", attention_label(s.attention_reason)),
                    ("attention since", at(s.attention_since.as_deref())),
                    ("tmux", s.tmux_session.clone()),
                    ("worktree", dash(s.worktree_path.as_deref())),
                    (
                        "round",
                        s.review_round.map_or("-".into(), |r| r.to_string()),
                    ),
                    ("internal id", dash(s.internal_session_id.as_deref())),
                    ("tokens", usage_block(&s.usage, &[], INDENT)),
                    ("activity", at(s.last_activity_at.as_deref())),
                    ("created", moment(&s.created_at)),
                    ("ended", at(s.ended_at.as_deref())),
                ])
            })?;
        }
        SessionCommand::Send {
            id,
            text,
            no_newline,
        } => {
            let data = keystrokes(&text, no_newline);
            client
                .send_no_content(
                    http::Method::POST,
                    &format!("/v1/sessions/{id}/input"),
                    Some(&SessionInputRequest { data }),
                )
                .await?;
            print(
                format,
                &serde_json::json!({"sent": true, "session": id}),
                || println!("typed into session {id}"),
            )?;
        }
        SessionCommand::Logs { id } => {
            let logs: ariadne_api::sessions::SessionLogsResponse =
                client.get_json(&format!("/v1/sessions/{id}/logs")).await?;
            match format {
                Format::Json => print_json(&logs)?,
                // A pane's scrollback is longer than a screen: it goes
                // through the pager when there is somebody to page for.
                Format::Table => pager::page(&logs.logs)?,
            }
        }
        SessionCommand::Resume { id } => {
            // The daemon answers with this same session either way: relaunched
            // when it really resumed it, or untouched when its pane turned out
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
                || match resumed {
                    true => println!("session {} resumed ({})", s.id, s.tmux_session),
                    false => println!(
                        "session {} already has a running agent ({}); nothing to resume",
                        s.id, s.tmux_session
                    ),
                },
            )?;
        }
        SessionCommand::Kill { id, yes } => {
            let s: SessionDto = client.get_json(&session_path(&id)).await?;
            confirm(&kill_question(&s), yes)?;
            let s: SessionDto = client
                .post_empty(&format!("/v1/sessions/{id}/kill"))
                .await?;
            print(format, &s, || {
                println!("session {} is now {}", s.id, s.status.as_str())
            })?;
        }
    }
    Ok(())
}

/// What `session send` types into the pane: the text, and the Return that
/// submits it unless the caller asked for the text alone.
///
/// A terminal's Return is a carriage return, which is what every agent TUI is
/// listening for — `\n` would land in the prompt as a newline in half of them
/// — and the endpoint types what it is given, byte for byte.
fn keystrokes(text: &str, no_newline: bool) -> String {
    match no_newline {
        true => text.to_string(),
        false => format!("{text}\r"),
    }
}

fn session_path(id: &str) -> String {
    format!("/v1/sessions/{id}")
}

/// Which of the sessions the daemon answered with `session ls` shows.
///
/// The default is docker's: live sessions, history behind --all. Named
/// statuses are that same choice made precisely, so they take over —
/// `--status exited` that then dropped every row for not being live would
/// answer nothing. The role narrows whatever those settled on: `GET
/// /v1/sessions` takes none, so it is applied to the answer rather than asked
/// for, and so is a second status, since it takes only one.
fn visible(
    sessions: Vec<SessionDto>,
    all: bool,
    statuses: &[SessionStatus],
    role: Option<Role>,
) -> Vec<SessionDto> {
    sessions
        .into_iter()
        .filter(|s| all || !statuses.is_empty() || s.status.is_live())
        .filter(|s| statuses.is_empty() || statuses.contains(&s.status))
        .filter(|s| role.is_none_or(|r| s.role == r))
        .collect()
}

/// The goal and task titles behind a table's sessions: which piece of work
/// each agent was run for, which the ids cannot say. One list call each, the
/// way [`ProfileNames`] resolves profile names, and a title is a courtesy — a
/// daemon that will not answer leaves the ids in place.
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

    /// What one session was run for: its task, or — for a planner session,
    /// which has none — the goal itself, prefixed so a whole goal is never
    /// read as a task of that name. An id stands in for a title the daemon did
    /// not answer with.
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

/// What `session kill` asks: a live agent is about to lose its terminal, and
/// the id alone does not say whose.
fn kill_question(s: &SessionDto) -> String {
    let what = match &s.task_id {
        Some(task) => format!("{} on task {task}", s.role.as_str()),
        None => format!("{} of goal {}", s.role.as_str(), s.goal_id),
    };
    format!("Kill session {} ({what}, {})?", s.id, s.status.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_core::Role;

    use crate::commands::fixtures::session;

    fn context() -> SessionContext {
        SessionContext {
            goals: HashMap::from([("01GOAL".to_string(), "Ship the board".to_string())]),
            tasks: HashMap::from([("01TASK".to_string(), "Wire the screen".to_string())]),
        }
    }

    #[test]
    fn a_session_on_a_task_is_named_by_the_task() {
        assert_eq!(
            context().label(&session("01SESS", "01GOAL", Some("01TASK"))),
            "Wire the screen"
        );
    }

    /// The planner runs for the goal itself, and the row says so rather than
    /// leaving a goal title where every other row carries a task.
    #[test]
    fn a_planner_session_is_named_by_its_goal() {
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

    /// One session per role and per liveness, as `session ls` receives them
    /// from the daemon: the planner is running, the engineer has exited.
    fn listed() -> Vec<SessionDto> {
        let engineer = SessionDto {
            status: SessionStatus::Exited,
            ..session("01ENG", "01GOAL", Some("01TASK"))
        };
        vec![session("01PLAN", "01GOAL", None), engineer]
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

    /// The role narrows whatever the rest of the flags settled on, and never
    /// widens it: a finished engineer stays behind --all even when --role
    /// names engineers.
    #[test]
    fn a_role_narrows_the_view_it_is_used_with() {
        assert_eq!(
            ids(visible(listed(), false, &[], Some(Role::Planner))),
            ["01PLAN"]
        );
        assert_eq!(
            ids(visible(listed(), false, &[], Some(Role::Engineer))),
            [] as [String; 0],
            "the only engineer here has exited"
        );
        assert_eq!(
            ids(visible(listed(), true, &[], Some(Role::Engineer))),
            ["01ENG"]
        );
        assert_eq!(
            ids(visible(listed(), false, &[], Some(Role::Reviewer))),
            [] as [String; 0]
        );
    }

    /// What `session send` types is the text and the Return that submits it:
    /// a carriage return, which is what a TUI reads as Enter, and nothing at
    /// all when the caller wants the text left in the prompt.
    #[test]
    fn what_is_typed_carries_its_own_return() {
        assert_eq!(keystrokes("approve", false), "approve\r");
        assert_eq!(keystrokes("approve", true), "approve");
        assert_eq!(
            keystrokes("", false),
            "\r",
            "a bare Return is a legitimate keystroke"
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
}
