//! The board `ariadne attention` prints: one section per goal, its stuck
//! tasks first and then its stuck sessions, in the order the UI's strip lists
//! them.

use std::collections::HashMap;

use serde::Serialize;

use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::tasks::TaskDto;

use super::{Reason, session_at, session_reason, task_reason};
use crate::output::{Column, UNCAPPED};

/// Columns of one goal's section. Task rows leave `task` empty (their id is
/// the task); session rows name their role in `title` and the task they were
/// run for here — by its title, the way the UI's strip names it, since a bare
/// ULID says nothing about which work is blocked.
pub const ROWS: &[Column] = &[
    ("id", UNCAPPED),
    ("title", 48),
    ("reason", UNCAPPED),
    ("task", 40),
    ("age", UNCAPPED),
];

/// The `--format json` document: goals → items, ready for scripting.
#[derive(Serialize)]
pub struct Attention {
    /// Rows in total, the number the UI's sidebar badge shows.
    pub count: usize,
    pub goals: Vec<Group>,
}

/// Everything one goal has that needs attention. Never empty.
#[derive(Serialize)]
pub struct Group {
    pub goal_id: String,
    /// The goal itself, when the goals list has it — a task or session can
    /// outlive its goal falling out of the list.
    pub goal: Option<GoalDto>,
    pub tasks: Vec<AttentionTask>,
    /// Sessions of this goal that want the user, its planner's included.
    pub sessions: Vec<AttentionSession>,
}

#[derive(Serialize)]
pub struct AttentionTask {
    pub reason: Reason,
    pub task: TaskDto,
}

#[derive(Serialize)]
pub struct AttentionSession {
    pub reason: Reason,
    pub session: SessionDto,
}

/// The three lists as one document: goals first, newest first — the order the
/// UI shows them in — then any goal the goals list did not carry.
pub fn group(goals: Vec<GoalDto>, tasks: Vec<TaskDto>, sessions: Vec<SessionDto>) -> Attention {
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

    for task in tasks {
        if let Some(reason) = task_reason(&task) {
            let i = index_of(&mut groups, &task.goal_id);
            groups[i].tasks.push(AttentionTask { reason, task });
        }
    }
    for session in sessions {
        if let Some(reason) = session_reason(&session) {
            let i = index_of(&mut groups, &session.goal_id);
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
pub fn heading(group: &Group) -> String {
    match &group.goal {
        Some(goal) => format!("{} ({})", goal.title, short_id(&goal.id)),
        None => format!("Goal {}", short_id(&group.goal_id)),
    }
}

/// Task id → title, for naming the task a session was run for.
pub fn task_titles(tasks: &[TaskDto]) -> HashMap<String, String> {
    tasks
        .iter()
        .map(|task| (task.id.clone(), task.title.clone()))
        .collect()
}

/// One goal's rows: its tasks first, then its stuck sessions, as the UI lists
/// them. `titles` names the task each session was working on, falling back to
/// a short id — the row still has to say what the agent was doing.
pub fn rows(
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
        vec![
            s.id.clone(),
            format!("{} session", s.role.as_str()),
            item.reason.label().into(),
            // A planner belongs to no task, and the goal heading above is
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

/// Ids are 26-character ULIDs: unreadable in full, but the tail is enough to
/// tell two of them apart — the same shortening the UI uses. The full id
/// stays in the table rows.
fn short_id(id: &str) -> String {
    match id.char_indices().nth_back(7) {
        Some((i, _)) if id.len() > 10 => format!("…{}", &id[i..]),
        _ => id.to_string(),
    }
}

/// How long ago that was: `12s`, `4m`, `3h`, `2d` — floored, never rounded, so
/// 89 seconds is "1m" and not the "2m" rounding would jump to a second early.
/// Anything unparseable is passed through, as [`crate::output::local_time`].
fn age(rfc3339: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    match chrono::DateTime::parse_from_rfc3339(rfc3339) {
        Ok(t) => {
            let total = (now - t.with_timezone(&chrono::Utc)).num_seconds().max(0);
            match total {
                s if s < 60 => format!("{s}s"),
                s if s < 3600 => format!("{}m", s / 60),
                s if s < 86400 => format!("{}h", s / 3600),
                s => format!("{}d", s / 86400),
            }
        }
        Err(_) => rfc3339.to_string(),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_core::{AttentionReason, Role, TaskStatus};

    use crate::commands::attention::reason_label;
    use crate::commands::attention::tests::{dead, flagged, goal, session, task};

    #[test]
    fn groups_follow_the_goal_order_the_ui_shows() {
        let attention = group(
            vec![goal("01GA", "older"), goal("01GB", "newer")],
            vec![
                task("01T1", "01GA", TaskStatus::Failed, false),
                task("01T2", "01GB", TaskStatus::InProgress, true),
                task("01T3", "01GONE", TaskStatus::Failed, false),
                task("01T4", "01GA", TaskStatus::ChangesRequested, false),
            ],
            vec![
                dead("01S1", "01GB", None),
                session("01S2", "01GA", Some("01T4")),
            ],
        );
        let ids: Vec<&str> = attention.goals.iter().map(|g| g.goal_id.as_str()).collect();
        assert_eq!(ids, ["01GB", "01GA", "01GONE"]);
        // The count is what the UI's badge shows: three task rows and the one
        // flagged session, which is a planner's and lands in its goal's group.
        assert_eq!(attention.count, 4);
        assert_eq!(
            attention.goals[0].goal.as_ref().map(|g| g.title.as_str()),
            Some("newer")
        );
        assert_eq!(attention.goals[0].sessions[0].session.role, Role::Planner);
        assert!(attention.goals[2].goal.is_none());

        let quiet = group(vec![goal("01GA", "A")], Vec::new(), Vec::new());
        assert_eq!(quiet.count, 0);
        assert!(quiet.goals.is_empty());
    }

    /// Who is asking and what they were working on: a session row names its
    /// role and the task by its title — the two things the UI's strip row
    /// leads with, where a ULID named nothing at all. A task row is its own
    /// subject, so the title is the row and the id is beside it.
    #[test]
    fn a_row_names_what_it_is_about() {
        let tasks = vec![task("01T9", "01GA", TaskStatus::Failed, false)];
        let titles = task_titles(&tasks);
        let now = chrono::Utc::now();
        let rows_of = |sessions| {
            let attention = group(vec![goal("01GA", "A")], tasks.clone(), sessions);
            rows(&attention.goals[0], &titles, now)
        };

        let rows = rows_of(vec![dead("01S1", "01GA", Some("01T9"))]);
        assert_eq!(rows[0][..4], ["01T9", "task 01T9", "failed", "-"]);
        assert_eq!(rows[1][0], "01S1");
        assert_eq!(rows[1][1], "engineer session");
        assert_eq!(rows[1][2], "disconnected");
        assert_eq!(rows[1][3], "task 01T9");

        // A task the list no longer carries: named by its short id rather than
        // leaving the column empty. A planner belongs to no task at all.
        let rows = rows_of(vec![
            dead("01S1", "01GA", Some("01ARZ3NDEKTSV4RRFFQ69G5FAV")),
            dead("01S2", "01GA", None),
        ]);
        assert_eq!(rows[1][3], "…Q69G5FAV");
        assert_eq!(rows[2][3], "-");
    }

    /// The wording the UI's `SESSION_ATTENTION_META` labels lowercase to,
    /// which `session ls` and `session inspect` take from here too.
    #[test]
    fn a_flagged_session_row_spells_the_reason_the_ui_spells() {
        let flags = [
            (AttentionReason::WaitingPermission, "waiting for permission"),
            (AttentionReason::WaitingInput, "waiting for input"),
            (AttentionReason::WaitingUser, "waiting for you"),
            (AttentionReason::AgentError, "agent error"),
            (AttentionReason::Disconnected, "disconnected"),
            (AttentionReason::Stalled, "stalled"),
        ];
        let sessions = flags
            .iter()
            .enumerate()
            .map(|(i, (flag, _))| flagged(&format!("01S{i}"), "01GA", *flag))
            .collect();
        let g = &group(vec![goal("01GA", "A")], Vec::new(), sessions).goals[0];
        let rows = rows(g, &HashMap::new(), chrono::Utc::now());
        let labels: Vec<&str> = rows.iter().map(|row| row[2].as_str()).collect();
        assert_eq!(labels, flags.map(|(_, label)| label));
        for (flag, label) in flags {
            assert_eq!(reason_label(flag), label);
        }
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

    /// Floored at every unit, clamped at zero for a stamp from the future
    /// (clock skew), passed through when unparseable.
    #[test]
    fn an_age_is_compact_and_floored() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-18T12:00:00Z")
            .expect("parse")
            .with_timezone(&chrono::Utc);
        let at = |s: &str| age(s, now);
        assert_eq!(at("2026-08-18T11:59:49Z"), "11s");
        assert_eq!(at("2026-08-18T11:58:31Z"), "1m");
        assert_eq!(at("2026-08-18T09:00:01Z"), "2h");
        assert_eq!(at("2026-08-15T12:00:00Z"), "3d");
        assert_eq!(at("2026-08-18T12:00:05Z"), "0s");
        assert_eq!(at("not a time"), "not a time");
    }

    /// `--format json` is for scripts: wire spellings, full DTOs.
    #[test]
    fn the_json_document_uses_wire_spellings() {
        let attention = group(
            vec![goal("01GA", "A")],
            vec![task("01T1", "01GA", TaskStatus::InProgress, true)],
            vec![flagged("01S1", "01GA", AttentionReason::WaitingPermission)],
        );
        let doc = serde_json::to_value(&attention).expect("serialize");
        assert_eq!(doc["count"], 2);
        assert_eq!(doc["goals"][0]["goal_id"], "01GA");
        assert_eq!(doc["goals"][0]["goal"]["title"], "A");
        assert_eq!(doc["goals"][0]["tasks"][0]["reason"], "stalled");
        assert_eq!(doc["goals"][0]["tasks"][0]["task"]["id"], "01T1");
        assert_eq!(
            doc["goals"][0]["sessions"][0]["reason"],
            "waiting_permission"
        );
        assert_eq!(doc["goals"][0]["sessions"][0]["session"]["id"], "01S1");
    }
}
