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

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde::Serialize;

use ariadne_api::attention::{AttentionCause, AttentionListDto, AttentionSubjectKind};
use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::{SessionEntryDto, SessionKind, SessionPageDto, SessionPageQuery};
use ariadne_api::tasks::TaskDto;
use ariadne_client::{Client, SseEvent};
use ariadne_core::{AttentionReason, TaskStatus};

use super::{follow, query_path};

use crate::output::table::{check_columns, heading as heading_style, quiet_lines, render_groups};
use crate::output::{Format, View, age, empty_state, note, print_json, view};
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
    /// A pull request session's `waiting_user` (026): every approval and
    /// check of the request reads green, and the merge is the user's.
    ReadyToMerge,
    /// A reviewer pull request session's `waiting_user` (029): its review
    /// is posted, and the approval is the user's to give.
    ReviewPosted,
    AgentError,
    Disconnected,
    Exhausted,
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
            Reason::ReadyToMerge => "ready to merge",
            Reason::ReviewPosted => "review posted, approve yourself",
            Reason::AgentError => "agent error",
            Reason::Disconnected => "disconnected",
            Reason::Exhausted => "exhausted",
        }
    }
}

/// Whether this task wants the user, and what for. Kept identical to
/// `taskAttentionReason` in the UI.
///
/// A task sent back to an earlier column is deliberately not one of them: the
/// review column has spoken and the daemon briefs the develop agent itself,
/// so that task waits on an agent. A resume that does not happen shows up as the session's own
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
            AttentionReason::Exhausted => Reason::Exhausted,
        }
    }
}

/// How a session's `attention_reason` is spelled outside this command —
/// `session ls` and `session inspect` show the same words, and the words are
/// this list's, so they are taken from here rather than written twice.
pub(crate) fn reason_label(reason: AttentionReason) -> &'static str {
    Reason::from(reason).label()
}

/// Whether this session wants the user, and what for. The stored reason is
/// the whole rule, as in the UI's `sessionAttention` — except `exhausted`,
/// which this board no longer raises a row for on its own: automatic model
/// switching may still clear the bare flag on the very next tick, so
/// whether it is still worth a person's time is `GET /v1/attention`'s own
/// call, not a flag read here. A `quota` item carries the session's own
/// subject already (`recovery_items_section`), so folding it into this
/// board's rows would say the same thing twice.
fn session_reason(session: &SessionEntryDto) -> Option<Reason> {
    match session.attention_reason {
        Some(AttentionReason::Exhausted) => None,
        Some(AttentionReason::WaitingUser) if session.pull_request_id.is_some() => {
            match session.seat {
                Some(ariadne_core::Seat::Reviewer) => Some(Reason::ReviewPosted),
                _ => Some(Reason::ReadyToMerge),
            }
        }
        reason => reason.map(Into::into),
    }
}

/// Every task id a recovery item's `affected` list already names: the
/// board's own `task_reason` would say the same failure a second time,
/// generic where the recovery item is specific, so this board leaves those
/// tasks to the recovery section entirely.
fn recovery_affected_task_ids(recovery: &AttentionListDto) -> HashSet<String> {
    recovery
        .items
        .iter()
        .flat_map(|item| {
            item.affected
                .iter()
                .filter(|subject| subject.kind == AttentionSubjectKind::Task)
                .map(|subject| subject.id.clone())
        })
        .collect()
}

/// Every item `GET /v1/attention` currently finds, printed as its own
/// section below the per-goal board: cause, summary, the one action that
/// clears it, how long it has waited, and what it affects. None of these
/// belongs to one goal the way a task or a loose session's row does, so
/// they stand apart from the board rather than inside one of its groups.
fn recovery_items_section(
    recovery: &AttentionListDto,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    if recovery.items.is_empty() {
        return None;
    }
    let mut lines = vec!["RECOVERY".to_string()];
    for item in &recovery.items {
        let affected = item
            .affected
            .iter()
            .map(|subject| subject.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!(
            "- [{}] {} ({} old)",
            cause_label(item.cause),
            item.summary,
            age(&item.since, now)
        ));
        lines.push(format!("  action: {}", item.required_action));
        lines.push(format!("  affects: {affected}"));
    }
    Some(lines.join("\n"))
}

/// `-q`'s rows: every goal's own, and one per recovery item — a
/// configuration-only blocker with no goal of its own would otherwise
/// print nothing under `-q` while the table still showed it.
fn quiet_rows(
    attention: &Attention,
    titles: &HashMap<String, String>,
    now: chrono::DateTime<chrono::Utc>,
    recovery: &AttentionListDto,
) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = attention
        .goals
        .iter()
        .flat_map(|group| board::rows(group, titles, now))
        .collect();
    rows.extend(recovery.items.iter().map(|item| vec![item.id.clone()]));
    rows
}

/// The table spelling of a recovery cause.
fn cause_label(cause: AttentionCause) -> &'static str {
    match cause {
        AttentionCause::Access => "access",
        AttentionCause::Quota => "quota",
        AttentionCause::Configuration => "configuration",
        AttentionCause::Resource => "resource",
        AttentionCause::Unknown => "unknown",
    }
}

/// When this session's row last moved: when its reason was raised, else the
/// death that put it here — and `created_at` only for a session the daemon has
/// not stamped an end on yet. The UI's rows age by the same three.
fn session_at(session: &SessionEntryDto) -> &str {
    session
        .attention_since
        .as_deref()
        .or(session.ended_at.as_deref())
        .or(session.created_at.as_deref())
        .unwrap_or_default()
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
            // A repository's forge fetch error is a configuration recovery
            // item's own evidence, and its removal or a goal's is what
            // takes a stale one down (`GET /v1/attention`).
            | "repository_updated"
            | "repository_deleted"
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

pub(crate) async fn run(client: &Client, watch: bool, format: Format) -> Result<()> {
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

/// Every Ariadne-kind session, over however many pages `GET /v1/sessions`
/// paged them into — an outside conversation carries no `attention_reason`
/// and is no business of this board, so the daemon is asked to leave it out
/// before the first page is even fetched.
///
/// Unfiltered otherwise, and narrowed by [`session_reason`] below rather than
/// by the daemon's `attention` filter: filtering here is what keeps the rule
/// — "the daemon raised a reason for it" — in one place with the UI, which
/// reads the same unfiltered list. `all` is set so a session that ended, or
/// last moved, more than 7 days ago is still seen: the daemon flags it for
/// exactly that, outstanding work owed to a dead agent, however old.
async fn ariadne_sessions(client: &Client) -> Result<Vec<SessionEntryDto>> {
    let query = SessionPageQuery {
        kind: Some(SessionKind::Ariadne),
        all: Some(true),
        ..SessionPageQuery::default()
    };
    let mut page: SessionPageDto = client
        .get_json(&query_path("/v1/sessions", &query)?)
        .await?;
    let mut sessions = page.sessions;
    while let Some(cursor) = page.next_cursor {
        page = client
            .get_json(&query_path(
                "/v1/sessions",
                &SessionPageQuery {
                    cursor: Some(cursor),
                    ..query.clone()
                },
            )?)
            .await?;
        sessions.extend(page.sessions);
    }
    Ok(sessions)
}

/// The list as it stands, read afresh: a redraw is the current answer, never
/// the last one patched up from the events that woke it.
async fn render(client: &Client, format: Format) -> Result<()> {
    let goals: Vec<GoalDto> = client.get_json("/v1/goals").await?;
    let tasks: Vec<TaskDto> = client.get_json("/v1/tasks").await?;
    let sessions = ariadne_sessions(client).await?;
    let recovery: AttentionListDto = client.get_json("/v1/attention").await?;
    let recovery_tasks = recovery_affected_task_ids(&recovery);
    let now = chrono::Utc::now();
    let recovery_section = recovery_items_section(&recovery, now);

    // Every task, not only the ones on the list: a session's row is named by
    // the task it was run for, which is usually a task that is doing fine.
    let titles = task_titles(&tasks);
    let attention = group(goals, tasks, sessions, &recovery_tasks);
    // A producer that could not read its evidence costs the list its own
    // items, not an all-clear: an empty board under an incomplete read is
    // unknown, never "nothing needs attention" (009).
    let incomplete = !recovery.complete;
    match format {
        Format::Json => print_json(&serde_json::json!({
            "goals": &attention.goals,
            "count": attention.count + recovery.items.len(),
            "recovery": &recovery.items,
            "complete": recovery.complete,
        }))?,
        // `-q` is the same promise here as in every `ls`: the ids, one per
        // line, so what is stuck can be piped into whatever unsticks it. The
        // goal headings are for eyes and go with the table.
        Format::Table if view().quiet => {
            let rows = quiet_rows(&attention, &titles, now, &recovery);
            if !rows.is_empty() {
                println!("{}", quiet_lines(&rows));
            }
        }
        Format::Table if attention.goals.is_empty() && recovery_section.is_none() => {
            if incomplete {
                note("Some of what needs attention could not be read; showing what is known.");
            } else {
                note(&empty_state("Nothing needs attention.", None));
            }
        }
        Format::Table => {
            if !attention.goals.is_empty() {
                println!("{}", board(&attention, &titles, now, view())?);
            }
            if let Some(section) = recovery_section {
                if !attention.goals.is_empty() {
                    println!();
                }
                println!("{section}");
            }
            if incomplete {
                println!();
                note("Some of what needs attention could not be read; showing what is known.");
            }
        }
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
    pub(crate) fn session(id: &str, goal_id: &str, task_id: Option<&str>) -> SessionEntryDto {
        SessionEntryDto {
            kind: SessionKind::Ariadne,
            id: id.into(),
            agent_id: "stub".into(),
            title: None,
            goal_id: Some(goal_id.into()),
            task_id: task_id.map(Into::into),
            seat: Some(match task_id {
                Some(_) => ariadne_core::Seat::Agent,
                None => ariadne_core::Seat::Orchestrator,
            }),
            task_agent_id: Some("01DEVELOP".into()),
            model: Some("stub:test-model".into()),
            effort: None,
            internal_session_id: None,
            working_directory: None,
            status: Some(SessionStatus::Failed),
            attention_reason: None,
            attention_since: None,
            last_activity_at: None,
            usage: Some(Default::default()),
            context_used: None,
            context_size: None,
            created_at: Some(NOW.into()),
            ended_at: None,
            pull_request_id: None,
        }
    }

    /// A live session the daemon has flagged: still running, and on the list
    /// because of the flag — the only way onto it.
    pub(crate) fn flagged(id: &str, goal_id: &str, reason: AttentionReason) -> SessionEntryDto {
        SessionEntryDto {
            attention_reason: Some(reason),
            attention_since: Some("2026-08-18T11:00:00Z".into()),
            ..session(id, goal_id, Some("01T9"))
        }
    }

    /// A dead session the daemon still owes work to: flagged, so on the list,
    /// and aged by its death since no `attention_since` was stamped.
    pub(crate) fn dead(id: &str, goal_id: &str, task_id: Option<&str>) -> SessionEntryDto {
        SessionEntryDto {
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

        // A task on its `pr` column waits on the forge, not on a person.
        let published = TaskDto {
            pr_url: Some("https://github.com/owner/repo/pull/12".into()),
            ..task("01T", "01G", TaskStatus::InProgress, false)
        };
        assert_eq!(task_reason(&published), None);
    }

    /// The reasons the UI reports for a session: the daemon's flag, and
    /// nothing else — an agent nothing is owed to is nobody's business,
    /// whether it is working or long dead. `exhausted` is never one of
    /// them: this board leaves it to `GET /v1/attention`'s own `quota`
    /// item entirely (`recovery_items_section`).
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
        assert_eq!(
            session_reason(&flagged("01S", "01GA", AttentionReason::Exhausted)),
            None
        );

        // A pull request session waiting on the user has a request that is
        // the user's to merge.
        let ready = SessionEntryDto {
            goal_id: None,
            task_id: None,
            pull_request_id: Some("01PR".into()),
            ..flagged("01S", "01GA", AttentionReason::WaitingUser)
        };
        assert_eq!(session_reason(&ready), Some(Reason::ReadyToMerge));
        assert_eq!(Reason::ReadyToMerge.label(), "ready to merge");
        // A reviewer session waiting on the user has posted its review, and
        // the approval is the user's to give (029).
        let reviewed = SessionEntryDto {
            seat: Some(ariadne_core::Seat::Reviewer),
            ..ready
        };
        assert_eq!(session_reason(&reviewed), Some(Reason::ReviewPosted));
        assert_eq!(
            Reason::ReviewPosted.label(),
            "review posted, approve yourself"
        );

        // Dead with nothing owed to it — the daemon deliberately raises no
        // flag for a reviewer that exited after voting — so it is not here.
        assert_eq!(session_reason(&session("01S", "01GA", None)), None);
        // Dead with work still on it: the daemon's flag puts it on the list.
        assert_eq!(
            session_reason(&dead("01S", "01GA", Some("01T9"))),
            Some(Reason::Disconnected)
        );
        // A flag survives the death that followed it.
        let died_after = SessionEntryDto {
            status: Some(SessionStatus::Failed),
            ..flagged("01S", "01GA", AttentionReason::AgentError)
        };
        assert_eq!(session_reason(&died_after), Some(Reason::AgentError));

        for status in [
            SessionStatus::Starting,
            SessionStatus::Running,
            SessionStatus::Idle,
            SessionStatus::Exited,
        ] {
            let healthy = SessionEntryDto {
                status: Some(status),
                ..session("01S", "01GA", None)
            };
            assert_eq!(session_reason(&healthy), None, "{}", status.as_str());
        }
    }

    fn attention_item(
        cause: AttentionCause,
        affected: Vec<(AttentionSubjectKind, &str)>,
    ) -> ariadne_api::attention::AttentionItemDto {
        use ariadne_api::attention::{AttentionProducer, AttentionSubjectDto, AttentionTarget};
        ariadne_api::attention::AttentionItemDto {
            id: "01I".into(),
            producer: AttentionProducer::Recovery,
            cause,
            summary: "a blocker".into(),
            required_action: "clear it".into(),
            since: NOW.into(),
            affected: affected
                .into_iter()
                .map(|(kind, id)| AttentionSubjectDto {
                    kind,
                    id: id.into(),
                    label: id.into(),
                })
                .collect(),
            target: AttentionTarget::Settings {
                section: "forge".into(),
            },
        }
    }

    /// Every task id any recovery item names, whatever its cause — the
    /// board's own `task_reason` leaves every one of them to the recovery
    /// section rather than repeating a blanker version of the same row.
    #[test]
    fn recovery_affected_task_ids_names_every_cause() {
        let recovery = AttentionListDto {
            items: vec![
                attention_item(
                    AttentionCause::Resource,
                    vec![(AttentionSubjectKind::Task, "01T1")],
                ),
                attention_item(
                    AttentionCause::Unknown,
                    vec![(AttentionSubjectKind::Task, "01T2")],
                ),
                attention_item(
                    AttentionCause::Quota,
                    vec![(AttentionSubjectKind::Session, "01S")],
                ),
            ],
            complete: true,
        };
        assert_eq!(
            recovery_affected_task_ids(&recovery),
            HashSet::from(["01T1".to_string(), "01T2".to_string()])
        );
    }

    /// Every item `GET /v1/attention` finds is its own section, whatever
    /// its cause — a `quota` item included, since this board no longer
    /// derives a session row from the bare `exhausted` flag at all — and
    /// each line names the cause, how long it has waited, the action that
    /// clears it, and what it affects.
    #[test]
    fn every_recovery_item_prints_in_its_own_section() {
        let now = NOW.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
        let recovery = AttentionListDto {
            items: vec![attention_item(
                AttentionCause::Quota,
                vec![(AttentionSubjectKind::Session, "01S")],
            )],
            complete: true,
        };
        let section = recovery_items_section(&recovery, now).expect("a quota item has a section");
        assert!(section.contains("[quota]"), "{section}");
        assert!(section.contains("a blocker"), "{section}");
        assert!(section.contains("clear it"), "{section}");
        assert!(section.contains("01S"), "{section}");

        assert_eq!(
            recovery_items_section(
                &AttentionListDto {
                    items: Vec::new(),
                    complete: true
                },
                now
            ),
            None
        );
    }

    /// The three stamps the UI ages a session row by, in its order.
    #[test]
    fn a_session_row_is_aged_by_when_its_reason_was_raised() {
        let waiting = flagged("01S", "01GA", AttentionReason::WaitingPermission);
        assert_eq!(session_at(&waiting), "2026-08-18T11:00:00Z");

        let died = SessionEntryDto {
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
            &HashSet::new(),
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
            &HashSet::new(),
        );
        let all: Vec<Vec<String>> = attention
            .goals
            .iter()
            .flat_map(|group| rows(group, &titles, now))
            .collect();
        assert_eq!(quiet_lines(&all), "01T2\n01T1\n01S1");
    }

    /// `-q` is the ids of every recovery item too, not only the per-goal
    /// board's: a configuration-only blocker has no goal of its own, so it
    /// would otherwise print nothing under `-q` while the table still
    /// showed it.
    #[test]
    fn quiet_rows_also_names_every_recovery_item() {
        let attention = group(Vec::new(), Vec::new(), Vec::new(), &HashSet::new());
        let recovery = AttentionListDto {
            items: vec![attention_item(
                AttentionCause::Configuration,
                vec![(AttentionSubjectKind::Repository, "01R")],
            )],
            complete: true,
        };
        let rows = quiet_rows(&attention, &HashMap::new(), chrono::Utc::now(), &recovery);
        assert_eq!(quiet_lines(&rows), "01I");
    }

    /// The daemon answers `GET /v1/sessions` with a page object, not a bare
    /// array — decoding it as the latter is exactly the bug this command had.
    #[tokio::test]
    async fn the_board_renders_against_a_paged_sessions_response() {
        use axum::Router;
        use axum::routing::get;

        async fn goals() -> axum::Json<Vec<GoalDto>> {
            axum::Json(vec![goal("01GA", "Ship the board")])
        }
        async fn tasks() -> axum::Json<Vec<TaskDto>> {
            axum::Json(vec![task("01T1", "01GA", TaskStatus::Failed, false)])
        }
        async fn sessions() -> axum::Json<SessionPageDto> {
            axum::Json(SessionPageDto {
                sessions: vec![flagged("01S1", "01GA", AttentionReason::WaitingPermission)],
                next_cursor: None,
                total: 1,
                snapshot_at: NOW.into(),
            })
        }
        async fn attention() -> axum::Json<AttentionListDto> {
            axum::Json(AttentionListDto {
                items: Vec::new(),
                complete: true,
            })
        }

        let app = Router::new()
            .route("/v1/goals", get(goals))
            .route("/v1/tasks", get(tasks))
            .route("/v1/sessions", get(sessions))
            .route("/v1/attention", get(attention));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = Client::tcp(format!("http://{address}"));
        let rendered = render(&client, Format::Json).await;
        server.abort();

        rendered.expect("a page object decodes, where a bare array once failed");
    }

    /// A board with more sessions than fit in one page still sees every one
    /// of them: the Ariadne kind alone is asked for, and every `next_cursor`
    /// is followed until the page is the last.
    #[tokio::test]
    async fn every_page_of_ariadne_sessions_is_fetched() {
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};

        use axum::Router;
        use axum::extract::{RawQuery, State};
        use axum::routing::get;

        #[derive(Clone)]
        struct Api {
            pages: Arc<Mutex<VecDeque<SessionPageDto>>>,
            queries: Arc<Mutex<Vec<String>>>,
        }

        async fn listed(
            State(api): State<Api>,
            RawQuery(query): RawQuery,
        ) -> axum::Json<SessionPageDto> {
            api.queries.lock().unwrap().push(query.unwrap_or_default());
            axum::Json(api.pages.lock().unwrap().pop_front().expect("page"))
        }

        let queries = Arc::new(Mutex::new(Vec::new()));
        let api = Api {
            pages: Arc::new(Mutex::new(VecDeque::from(vec![
                SessionPageDto {
                    sessions: vec![flagged("01S1", "01GA", AttentionReason::WaitingPermission)],
                    next_cursor: Some("after-one".into()),
                    total: 2,
                    snapshot_at: NOW.into(),
                },
                SessionPageDto {
                    sessions: vec![flagged("01S2", "01GA", AttentionReason::WaitingInput)],
                    next_cursor: None,
                    total: 2,
                    snapshot_at: NOW.into(),
                },
            ]))),
            queries: queries.clone(),
        };
        let app = Router::new()
            .route("/v1/sessions", get(listed))
            .with_state(api);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = Client::tcp(format!("http://{address}"));
        let sessions = ariadne_sessions(&client).await.unwrap();
        server.abort();

        assert_eq!(
            sessions.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["01S1", "01S2"],
            "both pages reach the board"
        );
        let queries = queries.lock().unwrap();
        assert!(queries[0].contains("kind=ariadne"), "{queries:?}");
        assert!(queries[1].contains("cursor=after-one"), "{queries:?}");
    }
}
