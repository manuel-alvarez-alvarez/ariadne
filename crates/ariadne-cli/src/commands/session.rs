//! `ariadne session ...`

use std::collections::HashMap;

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::{SessionDto, SessionListQuery};
use ariadne_api::tasks::TaskDto;
use ariadne_client::Client;
use ariadne_core::{AttentionReason, Role, SessionStatus};

use super::attention::reason_label;
use super::{ProfileNames, confirm};
use crate::output::{
    Column, Format, UNCAPPED, local_time, note, print_json, print_kv, print_table,
};
use crate::query::query_path;

/// Columns of `session ls`.
///
/// `context` is the one written by a human — a goal or task title runs as long
/// as it likes — so it is capped the way `task ls` caps its titles; what it is
/// cut to still says which work the row is about.
///
/// `attention` is next to `status` because the two are orthogonal: an agent
/// blocked on a permission prompt is still `running`, and the status alone
/// says nothing about it. It is `-` for a healthy session, and its wording is
/// `ariadne attention`'s, which is the UI's.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("context", 40),
    ("role", UNCAPPED),
    ("agent", UNCAPPED),
    ("status", UNCAPPED),
    ("attention", UNCAPPED),
    ("tmux", 32),
    ("internal id", 36),
];

#[derive(Subcommand)]
pub enum SessionCommand {
    /// List live agent sessions (docker-style; --all includes history)
    Ls {
        /// Filter by task id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        task: Option<String>,
        /// Filter by goal id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        goal: Option<String>,
        /// Filter by status, at the daemon: names one status instead of the
        /// live/finished split --all makes, and so replaces it — a status is
        /// listed whether or not it is a live one
        #[arg(long, value_enum)]
        status: Option<SessionStatus>,
        /// Filter by role, once the rows are here: `GET /v1/sessions` takes
        /// no role, so this narrows what it answered — as the UI's own role
        /// filter does. Composes with the rest: it never widens the list
        #[arg(long, value_enum)]
        role: Option<Role>,
        /// Include finished sessions (exited/failed), not just live ones;
        /// nothing to add once --status names one
        #[arg(short, long)]
        all: bool,
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
    },
    /// Show a session
    Inspect {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
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
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
    },
    /// Kill a session's tmux process
    Kill {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
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
            status,
            role,
            all,
            no_trunc,
        } => {
            let filtered = goal.is_some() || task.is_some() || status.is_some() || role.is_some();
            let query = SessionListQuery {
                goal,
                task,
                status,
                attention: None,
            };
            let sessions: Vec<SessionDto> = client
                .get_json(&query_path("/v1/sessions", &query)?)
                .await?;
            let sessions = visible(sessions, all, status, role);
            match format {
                Format::Json => print_json(&sessions)?,
                Format::Table => {
                    let context = SessionContext::fetch_for(client, &sessions).await;
                    print_table(
                        LS,
                        &sessions
                            .iter()
                            .map(|s| {
                                vec![
                                    s.id.clone(),
                                    context.label(s),
                                    s.role.as_str().into(),
                                    s.agent_kind.as_str().into(),
                                    s.status.as_str().into(),
                                    attention_label(s.attention_reason),
                                    s.tmux_session.clone(),
                                    s.internal_session_id.clone().unwrap_or_else(|| "-".into()),
                                ]
                            })
                            .collect::<Vec<_>>(),
                        no_trunc,
                    );
                    if sessions.is_empty() {
                        // A named status already says which sessions were
                        // asked for, so --all has nothing left to offer.
                        note(match (filtered, all || status.is_some()) {
                            (true, true) => "no sessions match that filter",
                            (true, false) => {
                                "no live sessions match that filter — finished ones are behind --all"
                            }
                            (false, true) => "no sessions yet",
                            (false, false) => "no live sessions — finished ones are behind --all",
                        });
                    }
                }
            }
        }
        SessionCommand::Inspect { id } => {
            let s: SessionDto = client.get_json(&format!("/v1/sessions/{id}")).await?;
            match format {
                Format::Json => print_json(&s)?,
                Format::Table => {
                    let profiles = ProfileNames::fetch(client).await;
                    print_kv(&[
                        ("id", s.id),
                        ("goal", s.goal_id),
                        ("task", s.task_id.unwrap_or_else(|| "-".into())),
                        ("role", s.role.as_str().into()),
                        ("profile", profiles.label(&s.profile_id)),
                        ("agent", s.agent_kind.as_str().into()),
                        // Recorded at launch, so it is what this session runs
                        // on even if the profile has moved on since.
                        ("model", s.model.unwrap_or_else(|| "default".into())),
                        ("status", s.status.as_str().into()),
                        ("attention", attention_label(s.attention_reason)),
                        (
                            "attention since",
                            s.attention_since.as_deref().map_or("-".into(), local_time),
                        ),
                        ("tmux", s.tmux_session),
                        ("worktree", s.worktree_path.unwrap_or_else(|| "-".into())),
                        (
                            "round",
                            s.review_round.map_or("-".into(), |r| r.to_string()),
                        ),
                        (
                            "internal id",
                            s.internal_session_id.unwrap_or_else(|| "-".into()),
                        ),
                        (
                            "activity",
                            s.last_activity_at.as_deref().map_or("-".into(), local_time),
                        ),
                        ("created", local_time(&s.created_at)),
                        (
                            "ended",
                            s.ended_at.as_deref().map_or("-".into(), local_time),
                        ),
                    ]);
                }
            }
        }
        SessionCommand::Logs { id } => {
            let logs: ariadne_api::sessions::SessionLogsResponse =
                client.get_json(&format!("/v1/sessions/{id}/logs")).await?;
            match format {
                Format::Json => print_json(&logs)?,
                Format::Table => print!("{}", logs.logs),
            }
        }
        SessionCommand::Resume { id } => {
            // The daemon answers with this same session either way: relaunched
            // when it really resumed it, or untouched when its pane turned out
            // to be alive already. What the row said before the call is what
            // tells a relaunch from a session that never needed one.
            let before: SessionDto = client.get_json(&format!("/v1/sessions/{id}")).await?;
            let s: SessionDto = client
                .post_empty(&format!("/v1/sessions/{id}/resume"))
                .await?;
            let resumed = !before.status.is_live() && s.status.is_live();
            match format {
                Format::Json => print_json(&serde_json::json!({
                    "resumed": resumed,
                    "session": s,
                }))?,
                Format::Table => {
                    if resumed {
                        println!("session {} resumed ({})", s.id, s.tmux_session);
                    } else {
                        println!(
                            "session {} already has a running agent ({}); nothing to resume",
                            s.id, s.tmux_session
                        );
                    }
                }
            }
        }
        SessionCommand::Kill { id, yes } => {
            let s: SessionDto = client.get_json(&format!("/v1/sessions/{id}")).await?;
            confirm(&kill_question(&s), yes)?;
            let s: SessionDto = client
                .post_empty(&format!("/v1/sessions/{id}/kill"))
                .await?;
            match format {
                Format::Json => print_json(&s)?,
                Format::Table => println!("session {} is now {}", s.id, s.status.as_str()),
            }
        }
    }
    Ok(())
}

/// Which of the sessions the daemon answered with `session ls` shows.
///
/// The default is docker's: live sessions, history behind --all. A named
/// status is that same choice made precisely, so it takes over — `--status
/// exited` that then dropped every row for not being live would answer
/// nothing at all.
///
/// The role is narrower than either and applies on top of both: `GET
/// /v1/sessions` takes no role, so — like the UI's own role filter — it is
/// applied to the answer rather than asked for.
fn visible(
    sessions: Vec<SessionDto>,
    all: bool,
    status: Option<SessionStatus>,
    role: Option<Role>,
) -> Vec<SessionDto> {
    sessions
        .into_iter()
        .filter(|s| all || status.is_some() || s.status.is_live())
        .filter(|s| role.is_none_or(|r| s.role == r))
        .collect()
}

/// The goal and task titles behind the sessions of a table, for its context
/// column: which piece of work each agent was run for, which the ids on the
/// row cannot say.
///
/// One list call each for the whole table, the way [`ProfileNames`] resolves
/// profile names. A title is a courtesy: a daemon that will not answer these
/// leaves the ids in place rather than failing the `ls` that asked for them.
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

    use ariadne_core::{AgentKind, Role};

    fn context() -> SessionContext {
        SessionContext {
            goals: HashMap::from([("01GOAL".to_string(), "Ship the board".to_string())]),
            tasks: HashMap::from([("01TASK".to_string(), "Wire the screen".to_string())]),
        }
    }

    fn session(goal: &str, task: Option<&str>) -> SessionDto {
        SessionDto {
            id: "01SESS".into(),
            goal_id: goal.into(),
            task_id: task.map(Into::into),
            role: Role::Engineer,
            profile_id: "01PROF".into(),
            agent_kind: AgentKind::ClaudeCode,
            model: None,
            internal_session_id: None,
            tmux_session: "ariadne-01SESS".into(),
            worktree_path: None,
            review_round: None,
            status: SessionStatus::Running,
            attention_reason: None,
            attention_since: None,
            last_activity_at: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            ended_at: None,
        }
    }

    #[test]
    fn a_session_on_a_task_is_named_by_the_task() {
        assert_eq!(
            context().label(&session("01GOAL", Some("01TASK"))),
            "Wire the screen"
        );
    }

    /// The planner runs for the goal itself, and the row says so rather than
    /// leaving a goal title where every other row carries a task.
    #[test]
    fn a_planner_session_is_named_by_its_goal() {
        assert_eq!(
            context().label(&session("01GOAL", None)),
            "goal: Ship the board"
        );
    }

    /// Titles are a courtesy — a daemon that would not answer the lists, or a
    /// task created since they were read, still leaves a usable row.
    #[test]
    fn an_unknown_id_stands_in_for_its_title() {
        let empty = SessionContext::default();
        assert_eq!(empty.label(&session("01GOAL", Some("01OTHER"))), "01OTHER");
        assert_eq!(empty.label(&session("01GOAL", None)), "goal: 01GOAL");
    }

    /// One session per role and per liveness, as `session ls` receives them
    /// from the daemon: the planner is running, the engineer has exited.
    fn listed() -> Vec<SessionDto> {
        let mut planner = session("01GOAL", None);
        planner.id = "01PLAN".into();
        planner.role = Role::Planner;
        let mut engineer = session("01GOAL", Some("01TASK"));
        engineer.id = "01ENG".into();
        engineer.status = SessionStatus::Exited;
        vec![planner, engineer]
    }

    fn ids(sessions: Vec<SessionDto>) -> Vec<String> {
        sessions.into_iter().map(|s| s.id).collect()
    }

    /// The default view is unchanged: live sessions, and history only once
    /// --all or a named --status asks for it.
    #[test]
    fn the_default_view_is_the_live_one() {
        assert_eq!(ids(visible(listed(), false, None, None)), ["01PLAN"]);
        assert_eq!(
            ids(visible(listed(), true, None, None)),
            ["01PLAN", "01ENG"]
        );
        assert_eq!(
            ids(visible(listed(), false, Some(SessionStatus::Exited), None)),
            ["01PLAN", "01ENG"],
            "a named status is the daemon's to answer, not ours to second-guess"
        );
    }

    /// The role narrows whatever the rest of the flags settled on, and never
    /// widens it: a finished engineer stays behind --all even when --role
    /// names engineers.
    #[test]
    fn a_role_narrows_the_view_it_is_used_with() {
        assert_eq!(
            ids(visible(listed(), false, None, Some(Role::Planner))),
            ["01PLAN"]
        );
        assert_eq!(
            ids(visible(listed(), false, None, Some(Role::Engineer))),
            [] as [String; 0],
            "the only engineer here has exited"
        );
        assert_eq!(
            ids(visible(listed(), true, None, Some(Role::Engineer))),
            ["01ENG"]
        );
        assert_eq!(
            ids(visible(listed(), false, None, Some(Role::Reviewer))),
            [] as [String; 0]
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
