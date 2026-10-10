//! The board `ariadne attention` prints: one section per goal, its stuck
//! tasks first and then its stuck sessions, in the order the UI's strip lists
//! them.

use std::collections::HashMap;

use serde::Serialize;

use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionEntryDto;
use ariadne_api::tasks::TaskDto;

use super::{Reason, session_at, session_reason, task_reason};
use crate::output::{Column, UNCAPPED, age, col, short_id};

/// Columns of one goal's section. Task rows leave `task` empty (their id is
/// the task); session rows name their seat in `title` and the task they were
/// run for here — by its title, the way the UI's strip names it, since a bare
/// ULID says nothing about which work is blocked.
///
/// The reason is why the row is here at all, so it stays with the id and the
/// title however narrow the terminal is; the task it belongs to is the one
/// thing the goal heading above already half answers.
pub(super) const ROWS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 48).title(),
    col("reason", UNCAPPED).attention(),
    col("task", 40).rank(1),
    col("age", UNCAPPED).rank(2),
];

/// The `--format json` document: goals → items, ready for scripting.
#[derive(Serialize)]
pub(super) struct Attention {
    /// Rows in total, the number the UI's sidebar badge shows.
    pub count: usize,
    pub goals: Vec<Group>,
}

/// Everything one goal has that needs attention. Never empty.
#[derive(Serialize)]
pub(super) struct Group {
    pub goal_id: String,
    /// The goal itself, when the goals list has it — a task or session can
    /// outlive its goal falling out of the list.
    pub goal: Option<GoalDto>,
    pub tasks: Vec<AttentionTask>,
    /// Sessions of this goal that want the user, its orchestrator's included.
    pub sessions: Vec<AttentionSession>,
}

#[derive(Serialize)]
pub(super) struct AttentionTask {
    pub reason: Reason,
    pub task: TaskDto,
}

#[derive(Serialize)]
pub(super) struct AttentionSession {
    pub reason: Reason,
    pub session: SessionEntryDto,
}

/// The three lists as one document: goals first, newest first — the order the
/// UI shows them in — then any goal the goals list did not carry.
pub(super) fn group(
    goals: Vec<GoalDto>,
    tasks: Vec<TaskDto>,
    sessions: Vec<SessionEntryDto>,
    recovery_trustworthy: bool,
) -> Attention {
    let mut goals = goals;
    goals.sort_by(|a, b| b.id.cmp(&a.id));

    let mut groups: Vec<Group> = Vec::new();
    let index_of = |groups: &mut Vec<Group>, goal_id: &str| -> usize {
        match groups.iter().position(|g| g.goal_id == goal_id) {
            Some(i) => i,
            None => {
                groups.push(Group {
                    goal_id: goal_id.to_string(),
                    goal: None,
                    tasks: Vec::new(),
                    sessions: Vec::new(),
                });
                groups.len() - 1
            }
        }
    };

    // Once the recovery read is complete, it is authoritative for every
    // failed task — including by staying silent while its orchestrator is
    // still the one answering it. Falling back to the bare status for a
    // task recovery merely left unnamed would undo that silence on the
    // spot, so the fallback is reserved for a read that could not be
    // trusted at all.
    if !recovery_trustworthy {
        for task in tasks {
            if let Some(reason) = task_reason(&task) {
                let i = index_of(&mut groups, &task.goal_id);
                groups[i].tasks.push(AttentionTask { reason, task });
            }
        }
    }
    for session in sessions {
        if let Some(reason) = session_reason(&session) {
            // A session's stall or disconnection is never this board's own
            // business, task-tied or not: while the agent is still being
            // nudged or relaunched, automatic recovery is still trying it,
            // and whichever recovery item eventually covers it — the
            // task's own, once it actually fails, or the taskless
            // orchestrator's or reviewer's own give-up item — says the
            // same thing with the cause and the action this bare flag
            // cannot.
            if matches!(reason, Reason::Stalled | Reason::Disconnected) {
                continue;
            }
            // A pull request session works for no goal: its requests are
            // one section of their own.
            let goal_id = match (&session.goal_id, &session.pull_request_id) {
                (Some(goal), _) => goal.as_str(),
                (None, Some(_)) => PULL_REQUESTS,
                (None, None) => "-",
            };
            let i = index_of(&mut groups, goal_id);
            groups[i]
                .sessions
                .push(AttentionSession { reason, session });
        }
    }

    let order: HashMap<&str, usize> = goals
        .iter()
        .enumerate()
        .map(|(i, g)| (g.id.as_str(), i))
        .collect();
    groups.sort_by_key(|g| order.get(g.goal_id.as_str()).copied().unwrap_or(usize::MAX));
    for group in &mut groups {
        group.goal = goals.iter().find(|g| g.id == group.goal_id).cloned();
    }

    Attention {
        count: groups
            .iter()
            .map(|g| g.tasks.len() + g.sessions.len())
            .sum(),
        goals: groups,
    }
}

/// One goal's section title: its title and short id, or just the short id
/// when the goals list no longer carries it.
pub(super) fn heading(group: &Group) -> String {
    match &group.goal {
        Some(goal) => format!("{} ({})", goal.title, short_id(&goal.id)),
        None if group.goal_id == PULL_REQUESTS => "Pull requests".into(),
        None => format!("Goal {}", short_id(&group.goal_id)),
    }
}

/// The section the sessions of pull requests are listed under.
pub(super) const PULL_REQUESTS: &str = "pull-requests";

/// Task id → title, for naming the task a session was run for.
pub(super) fn task_titles(tasks: &[TaskDto]) -> HashMap<String, String> {
    tasks
        .iter()
        .map(|task| (task.id.clone(), task.title.clone()))
        .collect()
}

/// One goal's rows: its tasks first, then its stuck sessions, as the UI lists
/// them. `titles` names the task each session was working on, falling back to
/// a short id — the row still has to say what the agent was doing.
pub(super) fn rows(
    group: &Group,
    titles: &HashMap<String, String>,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = group
        .tasks
        .iter()
        .map(|item| {
            vec![
                item.task.id.clone(),
                item.task.title.clone(),
                item.reason.label().into(),
                "-".into(),
                age(&item.task.updated_at, now),
            ]
        })
        .collect();
    rows.extend(group.sessions.iter().map(|item| {
        let s = &item.session;
        let title = match (&s.pull_request_id, &s.title) {
            (Some(_), Some(title)) => format!("pull request {title}"),
            _ => format!("{} session", s.seat.map_or("-", |seat| seat.as_str())),
        };
        vec![
            s.id.clone(),
            title,
            item.reason.label().into(),
            // An orchestrator belongs to no task, and the goal heading above is
            // already what it is about.
            s.task_id
                .as_deref()
                .map(|id| titles.get(id).cloned().unwrap_or_else(|| short_id(id)))
                .unwrap_or_else(|| "-".into()),
            age(session_at(s), now),
        ]
    }));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_core::{AttentionReason, Seat, TaskStatus};

    use crate::commands::attention::reason_label;
    use crate::commands::attention::tests::{flagged, goal, session, task};

    /// A pull request session ready to merge is listed under the pull
    /// requests, by the request's title.
    #[test]
    fn a_pull_request_ready_to_merge_is_listed_by_its_title() {
        let session = SessionEntryDto {
            goal_id: None,
            task_id: None,
            pull_request_id: Some("01PR".into()),
            title: Some("Fix widgets".into()),
            ..flagged("01PRS", "01GOAL", AttentionReason::WaitingUser)
        };
        let attention = group(Vec::new(), Vec::new(), vec![session], false);
        assert_eq!(heading(&attention.goals[0]), "Pull requests");
        let rows = rows(&attention.goals[0], &HashMap::new(), chrono::Utc::now());
        assert_eq!(rows[0][0], "01PRS");
        assert_eq!(rows[0][1], "pull request Fix widgets");
        assert_eq!(rows[0][2], "ready to merge");
    }

    #[test]
    fn a_loose_session_keeps_its_attention_row() {
        let session = SessionEntryDto {
            goal_id: None,
            task_id: None,
            seat: None,
            ..flagged("01LOOSE", "01GOAL", AttentionReason::AgentError)
        };
        let attention = group(Vec::new(), Vec::new(), vec![session], false);
        assert_eq!(attention.count, 1);
        assert_eq!(attention.goals[0].goal_id, "-");
        let rows = rows(&attention.goals[0], &HashMap::new(), chrono::Utc::now());
        assert_eq!(rows[0][1], "- session");
        assert_eq!(rows[0][3], "-");
    }

    #[test]
    fn groups_follow_the_goal_order_the_ui_shows() {
        let attention = group(
            vec![goal("01GA", "older"), goal("01GB", "newer")],
            vec![
                task("01T1", "01GA", TaskStatus::Failed, false),
                task("01T2", "01GB", TaskStatus::InProgress, true),
                task("01T3", "01GONE", TaskStatus::Failed, false),
                task("01T4", "01GA", TaskStatus::Ready, false),
            ],
            vec![
                SessionEntryDto {
                    task_id: None,
                    seat: Some(Seat::Orchestrator),
                    ..flagged("01S1", "01GB", AttentionReason::AgentError)
                },
                session("01S2", "01GA", Some("01T4")),
            ],
            false,
        );
        let ids: Vec<&str> = attention.goals.iter().map(|g| g.goal_id.as_str()).collect();
        assert_eq!(ids, ["01GB", "01GA", "01GONE"]);
        // The count is what the UI's badge shows: two failed task rows and
        // the one flagged session, which is an orchestrator's and lands in
        // its goal's group. The stalled task reports nothing of its own —
        // automatic recovery is still trying it (see `task_reason`).
        assert_eq!(attention.count, 3);
        assert_eq!(
            attention.goals[0].goal.as_ref().map(|g| g.title.as_str()),
            Some("newer")
        );
        assert_eq!(
            attention.goals[0].sessions[0].session.seat,
            Some(Seat::Orchestrator)
        );
        assert!(attention.goals[2].goal.is_none());

        let quiet = group(vec![goal("01GA", "A")], Vec::new(), Vec::new(), false);
        assert_eq!(quiet.count, 0);
        assert!(quiet.goals.is_empty());
    }

    /// Who is asking and what they were working on: a session row names its
    /// seat and the task by its title — the two things the UI's strip row
    /// leads with, where a ULID named nothing at all. A task row is its own
    /// subject, so the title is the row and the id is beside it.
    #[test]
    fn a_row_names_what_it_is_about() {
        let tasks = vec![task("01T9", "01GA", TaskStatus::Failed, false)];
        let titles = task_titles(&tasks);
        let now = chrono::Utc::now();
        let rows_of = |sessions| {
            let attention = group(vec![goal("01GA", "A")], tasks.clone(), sessions, false);
            rows(&attention.goals[0], &titles, now)
        };

        // `agent_error`, not `disconnected`: a task-tied disconnection is
        // this board's business only once the task itself fails (see
        // `an_exhausted_session_raises_no_row_of_its_own_on_this_board`'s
        // sibling rule), where `agent_error` carries no such gate.
        let task_tied = flagged("01S1", "01GA", AttentionReason::AgentError);
        let rows = rows_of(vec![task_tied]);
        assert_eq!(rows[0][..4], ["01T9", "task 01T9", "failed", "-"]);
        assert_eq!(rows[1][0], "01S1");
        assert_eq!(rows[1][1], "agent session");
        assert_eq!(rows[1][2], "agent error");
        assert_eq!(rows[1][3], "task 01T9");

        // A task the list no longer carries: named by its short id rather than
        // leaving the column empty. An orchestrator belongs to no task at all.
        let rows = rows_of(vec![
            SessionEntryDto {
                task_id: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
                ..flagged("01S1", "01GA", AttentionReason::AgentError)
            },
            SessionEntryDto {
                task_id: None,
                seat: Some(Seat::Orchestrator),
                ..flagged("01S2", "01GA", AttentionReason::AgentError)
            },
        ]);
        assert_eq!(rows[1][3], "…Q69G5FAV");
        assert_eq!(rows[2][3], "-");
    }

    /// The wording the UI's `SESSION_ATTENTION_META` labels lowercase to,
    /// which `session ls` and `session inspect` take from here too —
    /// `exhausted` included, since `reason_label` still spells it for those
    /// two even though this board no longer raises a row for the bare flag
    /// (`an_exhausted_session_raises_no_row_of_its_own_on_this_board`).
    #[test]
    fn a_flagged_session_row_spells_the_reason_the_ui_spells() {
        let flags = [
            (AttentionReason::WaitingPermission, "waiting for permission"),
            (AttentionReason::WaitingInput, "waiting for input"),
            (AttentionReason::WaitingUser, "waiting for you"),
            (AttentionReason::AgentError, "agent error"),
        ];
        let sessions: Vec<_> = flags
            .iter()
            .enumerate()
            .map(|(i, (flag, _))| flagged(&format!("01S{i}"), "01GA", *flag))
            .collect();
        let g = &group(vec![goal("01GA", "A")], Vec::new(), sessions, false).goals[0];
        let rows = rows(g, &HashMap::new(), chrono::Utc::now());
        let labels: Vec<&str> = rows.iter().map(|row| row[2].as_str()).collect();
        assert_eq!(labels, flags.map(|(_, label)| label));
        for (flag, label) in flags {
            assert_eq!(reason_label(flag), label);
        }
        // `reason_label` still spells every reason correctly for
        // `session ls`/`session inspect`, even the ones this board never
        // raises a row of its own for.
        assert_eq!(reason_label(AttentionReason::Exhausted), "exhausted");
        assert_eq!(reason_label(AttentionReason::Disconnected), "disconnected");
        assert_eq!(reason_label(AttentionReason::Stalled), "stalled");
    }

    /// `exhausted` is never a row of its own on this board, however it is
    /// asked: only `GET /v1/attention`'s `quota` item, read in
    /// `recovery_items_section`, says whether it is still worth a person's
    /// time.
    #[test]
    fn an_exhausted_session_raises_no_row_of_its_own_on_this_board() {
        let session = flagged("01S", "01GA", AttentionReason::Exhausted);
        let attention = group(vec![goal("01GA", "A")], Vec::new(), vec![session], false);
        assert!(attention.goals.is_empty());
    }

    /// `disconnected` and `stalled` are never rows of their own either,
    /// task-tied or not: a taskless orchestrator's or reviewer's own
    /// give-up is `GET /v1/attention`'s own `unknown` item to raise, read
    /// in `recovery_items_section`, never derived from the bare flag here.
    #[test]
    fn a_taskless_disconnected_or_stalled_session_raises_no_row_of_its_own_on_this_board() {
        let sessions = vec![
            SessionEntryDto {
                task_id: None,
                seat: Some(Seat::Orchestrator),
                ..flagged("01S1", "01GA", AttentionReason::Disconnected)
            },
            SessionEntryDto {
                task_id: None,
                seat: Some(Seat::Orchestrator),
                ..flagged("01S2", "01GA", AttentionReason::Stalled)
            },
        ];
        let attention = group(vec![goal("01GA", "A")], Vec::new(), sessions, false);
        assert!(attention.goals.is_empty());
    }

    /// A complete recovery read is authoritative for every failed task, not
    /// only the ones its own items name: once `GET /v1/attention` answered
    /// in full, a failed task recovery is still working stays off this
    /// board entirely rather than falling back to the bare `failed` row —
    /// the exact bypass a membership check on recovery's own `affected`
    /// lists used to leave open (an empty, complete recovery read is
    /// indistinguishable from a task recovery had nothing to say about).
    /// Only once the read itself could not be trusted does the bare status
    /// stand in.
    #[test]
    fn a_failed_task_recovery_has_not_named_stays_off_the_board_once_recovery_is_trustworthy() {
        let tasks = vec![task("01T1", "01GA", TaskStatus::Failed, false)];

        let trusted = group(vec![goal("01GA", "A")], tasks.clone(), Vec::new(), true);
        assert!(trusted.goals.is_empty());

        let untrusted = group(vec![goal("01GA", "A")], tasks, Vec::new(), false);
        assert_eq!(untrusted.count, 1);
    }

    /// The heading is the goal's title and the same shortened id the UI's
    /// `shortId` produces — or the bare short id when the goals list no longer
    /// carries the goal. Ids short enough to read stay whole.
    #[test]
    fn the_heading_is_the_title_or_the_bare_short_id() {
        let with = Group {
            goal_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            goal: Some(goal("01ARZ3NDEKTSV4RRFFQ69G5FAV", "CLI vs App")),
            tasks: Vec::new(),
            sessions: Vec::new(),
        };
        assert_eq!(heading(&with), "CLI vs App (…Q69G5FAV)");
        assert_eq!(heading(&Group { goal: None, ..with }), "Goal …Q69G5FAV");
        assert_eq!(short_id("0123456789"), "0123456789");
    }

    /// `--format json` is for scripts: wire spellings, full DTOs.
    #[test]
    fn the_json_document_uses_wire_spellings() {
        let attention = group(
            vec![goal("01GA", "A")],
            vec![task("01T1", "01GA", TaskStatus::Failed, false)],
            vec![flagged("01S1", "01GA", AttentionReason::WaitingPermission)],
            false,
        );
        let doc = serde_json::to_value(&attention).expect("serialize");
        assert_eq!(doc["count"], 2);
        assert_eq!(doc["goals"][0]["goal_id"], "01GA");
        assert_eq!(doc["goals"][0]["goal"]["title"], "A");
        assert_eq!(doc["goals"][0]["tasks"][0]["reason"], "failed");
        assert_eq!(doc["goals"][0]["tasks"][0]["task"]["id"], "01T1");
        assert_eq!(
            doc["goals"][0]["sessions"][0]["reason"],
            "waiting_permission"
        );
        assert_eq!(doc["goals"][0]["sessions"][0]["session"]["id"], "01S1");
    }
}
