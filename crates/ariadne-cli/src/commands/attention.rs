//! `ariadne attention` — everything that needs a human, grouped by goal.
//!
//! The CLI's half of the UI's attention strip, composed client-side from the
//! same three lists (`ui/src/features/goals/attention.ts`): every goal, every
//! task, every session. The inclusion rules mirror the UI's exactly, so both
//! surfaces agree on what — and how much — is stuck, and the reasons are the
//! labels of `SESSION_ATTENTION_META` in
//! `ui/src/features/sessions/session-display.tsx`, lowercased. The grouping by
//! goal is the CLI's own: the strip lists rows flat.
//!
//! What is *not* here is anything an agent is waiting on: a task whose review
//! asked for changes, and a session that died with no work owed to it. The
//! daemon decides when either of those wants a person and says so in
//! `attention_reason`; deriving a row from a bare status here is what made
//! this list disagree with it.

mod board;

use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;

use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::tasks::TaskDto;
use ariadne_client::{Client, SseEvent};
use ariadne_core::{AttentionReason, TaskStatus};

use super::follow;

use crate::output::table::{check_columns, heading as heading_style, quiet_lines, render_groups};
use crate::output::{Format, View, note, print_json, view};
use board::{Attention, Group, ROWS, group, heading, rows, task_titles};

/// Why a row is on the list — the task reasons and the session reasons in one
/// vocabulary, since one table lists both. `failed` is a task's alone: a
/// session is on the list for the flag the daemon raised, never for its own
/// death.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    Failed,
    Stalled,
    WaitingPermission,
    WaitingInput,
    WaitingUser,
    AgentError,
    Disconnected,
}

impl Reason {
    /// The table spelling; JSON keeps the wire spelling via serde.
    fn label(self) -> &'static str {
        match self {
            Reason::Failed => "failed",
            Reason::Stalled => "stalled",
            Reason::WaitingPermission => "waiting for permission",
            Reason::WaitingInput => "waiting for input",
            Reason::WaitingUser => "waiting for you",
            Reason::AgentError => "agent error",
            Reason::Disconnected => "disconnected",
        }
    }
}

/// Whether this task wants the user, and what for. Kept identical to
/// `taskAttentionReason` in the UI.
///
/// `changes_requested` is deliberately not one of them: the reviewer has
/// spoken and the daemon resumes the author itself, so that task waits on an
/// agent. A resume that does not happen shows up as the session's own
/// `disconnected` or `stalled` flag. And `stalled` is checked last because it
/// is a flag on top of a status — the task's column mirrors any of its
/// sessions carrying `stalled` and comes down when that session's does — so a
/// task that also failed reads as failed.
fn task_reason(task: &TaskDto) -> Option<Reason> {
    match task.status {
        TaskStatus::Failed => Some(Reason::Failed),
        _ if task.stalled => Some(Reason::Stalled),
        _ => None,
    }
}

impl From<AttentionReason> for Reason {
    fn from(reason: AttentionReason) -> Self {
        match reason {
            AttentionReason::WaitingPermission => Reason::WaitingPermission,
            AttentionReason::WaitingInput => Reason::WaitingInput,
            AttentionReason::WaitingUser => Reason::WaitingUser,
            AttentionReason::AgentError => Reason::AgentError,
            AttentionReason::Disconnected => Reason::Disconnected,
            AttentionReason::Stalled => Reason::Stalled,
        }
    }
}

/// How a session's `attention_reason` is spelled outside this command —
/// `session ls` and `session inspect` show the same words, and the words are
/// this list's, so they are taken from here rather than written twice.
pub fn reason_label(reason: AttentionReason) -> &'static str {
    Reason::from(reason).label()
}

/// Whether this session wants the user, and what for. The stored reason is the
/// whole rule, as in the UI's `sessionAttention`.
///
/// A dead session raises no reason of its own on purpose: the daemon flags the
/// agent it still owes work to and leaves the rest alone, so a reviewer that
/// exited after voting is finished, not stuck — and reading `status` here
/// would put it back on the list the daemon kept it off.
fn session_reason(session: &SessionDto) -> Option<Reason> {
    session.attention_reason.map(Into::into)
}

/// When this session's row last moved: when its reason was raised, else the
/// death that put it here — and `created_at` only for a session the daemon has
/// not stamped an end on yet. The UI's rows age by the same three.
fn session_at(session: &SessionDto) -> &str {
    session
        .attention_since
        .as_deref()
        .or(session.ended_at.as_deref())
        .unwrap_or(&session.created_at)
}

/// The events that can put a row on this list or take one off it: a task's
/// status or stall flag, a session's attention flag, and a goal appearing or
/// going. Nothing else redraws it — an agent event moves the daemon, and it is
/// the daemon's own write that arrives here as a session update.
fn relevant(frame: &SseEvent) -> bool {
    matches!(
        frame.event.as_str(),
        "goal_created"
            | "goal_updated"
            | "goal_deleted"
            | "task_created"
            | "task_updated"
            | "session_created"
            | "session_updated"
    )
}

/// The heading a goal's table stands under, in the one style every heading
/// is printed in — the same seat it plays in `ariadne doctor`, and the same
/// look the column header under it has.
fn heading_line(group: &Group, color: bool) -> String {
    heading_style(&heading(group), color)
}

/// The whole board as one screen: a heading over each goal's table, one blank
/// line between two goals, and one fit across the lot.
///
/// The fit is the point of rendering it in one go: a table fitted per goal
/// puts the same column in a different place under every heading, which is
/// unreadable on a screen a reader scans down.
fn board(
    attention: &Attention,
    titles: &HashMap<String, String>,
    now: chrono::DateTime<chrono::Utc>,
    view: &View,
) -> Result<String> {
    let groups: Vec<Vec<Vec<String>>> = attention
        .goals
        .iter()
        .map(|group| rows(group, titles, now))
        .collect();
    let borrowed: Vec<&[Vec<String>]> = groups.iter().map(Vec::as_slice).collect();
    let tables = render_groups(ROWS, &borrowed, view)?;
    Ok(attention
        .goals
        .iter()
        .zip(tables)
        .map(|(group, table)| format!("{}\n{table}", heading_line(group, view.color)))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// Whether this run prints a table, which is what makes a `--columns` worth
/// refusing.
///
/// JSON has no columns at all, and `-q` prints the first cell of every row
/// whatever `--columns` names — so neither reads the flag, and neither may
/// refuse it. Every other `-q` listing goes through [`super::super::output::print_list`],
/// which skips the table and never looks at `--columns`; refusing here alone
/// would fail one command where the rest of the CLI does not.
fn prints_a_table(format: Format, view: &View) -> bool {
    matches!(format, Format::Table) && !view.quiet
}

pub async fn run(client: &Client, watch: bool, format: Format) -> Result<()> {
    // Before the first request, let alone the first table: a `--columns` this
    // board does not have is one error, not one per goal — and on a `--watch`
    // it is an error rather than a cleared screen saying it forever.
    if prints_a_table(format, view()) {
        check_columns(ROWS, view())?;
    }
    if !watch {
        return render(client, format).await;
    }
    follow::watch(client, "/v1/events/stream", relevant, async || {
        render(client, format).await
    })
    .await
}

/// The list as it stands, read afresh: a redraw is the current answer, never
/// the last one patched up from the events that woke it.
async fn render(client: &Client, format: Format) -> Result<()> {
    let goals: Vec<GoalDto> = client.get_json("/v1/goals").await?;
    let tasks: Vec<TaskDto> = client.get_json("/v1/tasks").await?;
    // Unfiltered, and narrowed by `session_reason` below rather than by the
    // daemon's `attention` filter: filtering here is what keeps the rule — "the
    // daemon raised a reason for it" — in one place with the UI, which reads
    // the same unfiltered list.
    let sessions: Vec<SessionDto> = client.get_json("/v1/sessions").await?;

    // Every task, not only the ones on the list: a session's row is named by
    // the task it was run for, which is usually a task that is doing fine.
    let titles = task_titles(&tasks);
    let attention = group(goals, tasks, sessions);
    let now = chrono::Utc::now();
    match format {
        Format::Json => print_json(&attention)?,
        // `-q` is the same promise here as in every `ls`: the ids, one per
        // line, so what is stuck can be piped into whatever unsticks it. The
        // goal headings are for eyes and go with the table.
        Format::Table if view().quiet => {
            let rows: Vec<Vec<String>> = attention
                .goals
                .iter()
                .flat_map(|group| rows(group, &titles, now))
                .collect();
            if !rows.is_empty() {
                println!("{}", quiet_lines(&rows));
            }
        }
        Format::Table if attention.goals.is_empty() => note("nothing needs attention"),
        Format::Table => println!("{}", board(&attention, &titles, now, view())?),
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use ariadne_core::SessionStatus;

    use crate::commands::fixtures::{self, NOW};
    use crate::output::style;

    /// A failed session the daemon raised nothing for — which is nobody's
    /// business. `flagged` and `dead` are the ones that are on the list.
    pub(crate) fn session(id: &str, goal_id: &str, task_id: Option<&str>) -> SessionDto {
        SessionDto {
            status: SessionStatus::Failed,
            ..fixtures::session(id, goal_id, task_id)
        }
    }

    /// A live session the daemon has flagged: still running, and on the list
    /// because of the flag — the only way onto it.
    pub(crate) fn flagged(id: &str, goal_id: &str, reason: AttentionReason) -> SessionDto {
        SessionDto {
            attention_reason: Some(reason),
            attention_since: Some("2026-08-18T11:00:00Z".into()),
            ..fixtures::session(id, goal_id, Some("01T9"))
        }
    }

    /// A dead session the daemon still owes work to: flagged, so on the list,
    /// and aged by its death since no `attention_since` was stamped.
    pub(crate) fn dead(id: &str, goal_id: &str, task_id: Option<&str>) -> SessionDto {
        SessionDto {
            attention_reason: Some(AttentionReason::Disconnected),
            ..session(id, goal_id, task_id)
        }
    }

    pub(crate) fn task(id: &str, goal_id: &str, status: TaskStatus, stalled: bool) -> TaskDto {
        TaskDto {
            status,
            stalled,
            ..fixtures::task(id, goal_id)
        }
    }

    pub(crate) use fixtures::goal;

    /// The reasons the UI reports for a task, in its precedence: a stalled
    /// task that also failed is failed, and a healthy task is nobody's
    /// business — including a task on its way out under its own power, whose
    /// review asked for changes or whose author is landing it. Inventing a
    /// reason there from the bare status is exactly the disagreement with the
    /// UI this list exists not to have.
    #[test]
    fn a_task_is_reported_for_the_reason_the_ui_would_give() {
        let reason = |status, stalled| task_reason(&task("01T", "01G", status, stalled));
        assert_eq!(reason(TaskStatus::Failed, false), Some(Reason::Failed));
        assert_eq!(reason(TaskStatus::Failed, true), Some(Reason::Failed));
        assert_eq!(reason(TaskStatus::InProgress, true), Some(Reason::Stalled));
        assert_eq!(reason(TaskStatus::InProgress, false), None);
        assert_eq!(reason(TaskStatus::Finished, false), None);

        // Waiting on an agent, not on a person — but a stall on top of either
        // is still a stall.
        for status in [TaskStatus::ChangesRequested, TaskStatus::Approved] {
            assert_eq!(reason(status, false), None, "{}", status.as_str());
            assert_eq!(reason(status, true), Some(Reason::Stalled));
        }
        let published = TaskDto {
            pr_url: Some("https://github.com/owner/repo/pull/12".into()),
            ..task("01T", "01G", TaskStatus::Approved, false)
        };
        assert_eq!(task_reason(&published), None);
    }

    /// The reasons the UI reports for a session: the daemon's flag, and
    /// nothing else — an agent nothing is owed to is nobody's business,
    /// whether it is working or long dead.
    #[test]
    fn a_session_is_reported_for_the_reason_the_ui_would_give() {
        for (flag, expected) in [
            (
                AttentionReason::WaitingPermission,
                Reason::WaitingPermission,
            ),
            (AttentionReason::WaitingInput, Reason::WaitingInput),
            (AttentionReason::WaitingUser, Reason::WaitingUser),
            (AttentionReason::AgentError, Reason::AgentError),
            (AttentionReason::Disconnected, Reason::Disconnected),
            (AttentionReason::Stalled, Reason::Stalled),
        ] {
            assert_eq!(
                session_reason(&flagged("01S", "01GA", flag)),
                Some(expected),
                "{}",
                flag.as_str()
            );
        }

        // Dead with nothing owed to it — the daemon deliberately raises no
        // flag for a reviewer that exited after voting — so it is not here.
        assert_eq!(session_reason(&session("01S", "01GA", None)), None);
        // Dead with work still on it: the daemon's flag puts it on the list.
        assert_eq!(
            session_reason(&dead("01S", "01GA", Some("01T9"))),
            Some(Reason::Disconnected)
        );
        // A flag survives the death that followed it.
        let died_after = SessionDto {
            status: SessionStatus::Failed,
            ..flagged("01S", "01GA", AttentionReason::AgentError)
        };
        assert_eq!(session_reason(&died_after), Some(Reason::AgentError));

        for status in [
            SessionStatus::Starting,
            SessionStatus::Running,
            SessionStatus::Idle,
            SessionStatus::Exited,
        ] {
            let healthy = SessionDto {
                status,
                ..session("01S", "01GA", None)
            };
            assert_eq!(session_reason(&healthy), None, "{}", status.as_str());
        }
    }

    /// The three stamps the UI ages a session row by, in its order.
    #[test]
    fn a_session_row_is_aged_by_when_its_reason_was_raised() {
        let waiting = flagged("01S", "01GA", AttentionReason::WaitingPermission);
        assert_eq!(session_at(&waiting), "2026-08-18T11:00:00Z");

        let died = SessionDto {
            ended_at: Some("2026-08-18T12:00:00Z".into()),
            ..dead("01S", "01GA", None)
        };
        assert_eq!(session_at(&died), "2026-08-18T12:00:00Z");

        // Failed, but the daemon never stamped an end on it.
        assert_eq!(session_at(&dead("01S", "01GA", None)), NOW);
    }

    /// A goal heading is printed in the one heading style — bold and
    /// uppercase — which is the style of the column header under it.
    #[test]
    fn the_goal_heading_is_printed_in_the_one_heading_style() {
        let g = Group {
            goal_id: "01GA".into(),
            goal: Some(goal("01GA", "Ship the board")),
            tasks: Vec::new(),
            sessions: Vec::new(),
        };
        assert_eq!(heading_line(&g, false), "SHIP THE BOARD (01GA)");
        assert_eq!(
            heading_line(&g, true),
            style::paint(true, style::HEADING, "SHIP THE BOARD (01GA)")
        );
    }

    /// A board of two goals: each goal keeps its own heading and its own
    /// table, and the tables are fitted together — so the column header lands
    /// in the same place under every heading, whatever a goal's rows hold.
    #[test]
    fn the_columns_align_across_every_goal_of_the_board() {
        let tasks = vec![
            task("01T1", "01GA", TaskStatus::Failed, false),
            TaskDto {
                title: "A title that runs on and on and would set this column much wider".into(),
                ..task("01T2", "01GB", TaskStatus::Failed, false)
            },
        ];
        let titles = task_titles(&tasks);
        let attention = group(
            vec![goal("01GA", "Older goal"), goal("01GB", "Newer goal")],
            tasks,
            Vec::new(),
        );
        let screen = board(&attention, &titles, chrono::Utc::now(), &View::plain()).expect("board");

        let lines: Vec<&str> = screen.lines().collect();
        assert_eq!(lines[0], "NEWER GOAL (01GB)");
        let header = lines[1];
        assert!(header.starts_with("ID"), "{screen}");
        // The blank line between two goals, then the second heading and the
        // same header row under it, byte for byte.
        assert_eq!(lines[3], "");
        assert_eq!(lines[4], "OLDER GOAL (01GA)");
        assert_eq!(lines[5], header, "{screen}");
    }

    /// A `--columns` this board does not have is one error, raised before
    /// anything is printed — the same check `--watch` runs before it opens
    /// the stream.
    #[test]
    fn a_bad_columns_flag_is_refused_before_any_table() {
        let bad = View {
            columns: vec!["colour".into()],
            ..View::plain()
        };
        let err = check_columns(ROWS, &bad).expect_err("no such column");
        assert!(err.to_string().contains("no column \"colour\""), "{err}");
        check_columns(ROWS, &View::plain()).expect("no --columns names nothing wrong");
    }

    /// Only the run that prints a table refuses a `--columns`: `-q` prints
    /// the first cell of every row whatever the flag names, and JSON has no
    /// columns at all. This is what every other `-q` listing does, so one
    /// command does not fail on a flag the rest of the CLI ignores.
    #[test]
    fn a_columns_flag_is_refused_only_where_a_table_is_printed() {
        let quiet = View {
            quiet: true,
            ..View::plain()
        };
        assert!(prints_a_table(Format::Table, &View::plain()));
        assert!(!prints_a_table(Format::Table, &quiet));
        assert!(!prints_a_table(Format::Json, &View::plain()));
        assert!(!prints_a_table(Format::Json, &quiet));
    }

    /// `-q` is the ids of every goal's rows, one per line: the same
    /// `quiet_lines` every other listing pipes through.
    #[test]
    fn quiet_output_is_the_ids_of_every_group() {
        let tasks = vec![
            task("01T1", "01GA", TaskStatus::Failed, false),
            task("01T2", "01GB", TaskStatus::Failed, false),
        ];
        let titles = task_titles(&tasks);
        let now = chrono::Utc::now();
        let attention = group(
            vec![goal("01GA", "Older goal"), goal("01GB", "Newer goal")],
            tasks,
            vec![dead("01S1", "01GA", Some("01T1"))],
        );
        let all: Vec<Vec<String>> = attention
            .goals
            .iter()
            .flat_map(|group| rows(group, &titles, now))
            .collect();
        assert_eq!(quiet_lines(&all), "01T2\n01T1\n01S1");
    }
}
