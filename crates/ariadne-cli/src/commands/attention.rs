//! `ariadne attention` — everything that needs a human, grouped by goal.
//!
//! The CLI's version of the UI's attention strip, composed client-side from
//! the same three lists (`ui/src/features/goals/attention.ts`): every goal,
//! every task, and every session. The inclusion rules mirror the UI's exactly,
//! so both surfaces agree on what — and how much — is stuck, and the reasons
//! are the labels of `SESSION_ATTENTION_META` in
//! `ui/src/features/sessions/session-display.tsx`, lowercased. The grouping by
//! goal is the CLI's own: the strip lists rows flat.

use anyhow::Result;
use serde::Serialize;

use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::tasks::TaskDto;
use ariadne_client::Client;
use ariadne_core::{AttentionReason, SessionStatus, TaskStatus};

use crate::output::{Column, Format, UNCAPPED, note, print_json, print_table};

/// Columns of one goal's section. Task rows leave `task` empty (their id is
/// the task); session rows name their role in `title` and their task here.
const ROWS: &[Column] = &[
    ("id", UNCAPPED),
    ("title", 48),
    ("reason", UNCAPPED),
    ("task", UNCAPPED),
    ("age", UNCAPPED),
];

/// Why a row is on the list — the task reasons and the session reasons in one
/// vocabulary, since one table lists both.
///
/// For a task, `stalled` is checked last because it is a flag *on top of* a
/// status: the daemon sets it when an agent went idle without advancing the
/// task and clears it on the next transition, so a task that also failed is
/// reported as failed. `failed` is shared: a dead task and a dead agent are
/// the same word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    Failed,
    ChangesRequested,
    Stalled,
    WaitingPermission,
    WaitingInput,
    AgentError,
    Disconnected,
}

impl Reason {
    /// The table spelling; JSON keeps the wire spelling via serde.
    fn label(self) -> &'static str {
        match self {
            Reason::Failed => "failed",
            Reason::ChangesRequested => "changes requested",
            Reason::Stalled => "stalled",
            Reason::WaitingPermission => "waiting for permission",
            Reason::WaitingInput => "waiting for input",
            Reason::AgentError => "agent error",
            Reason::Disconnected => "disconnected",
        }
    }
}

/// Whether this task wants the user, and what for.
fn task_reason(task: &TaskDto) -> Option<Reason> {
    match task.status {
        TaskStatus::Failed => Some(Reason::Failed),
        TaskStatus::ChangesRequested => Some(Reason::ChangesRequested),
        _ if task.stalled => Some(Reason::Stalled),
        _ => None,
    }
}

impl From<AttentionReason> for Reason {
    fn from(reason: AttentionReason) -> Self {
        match reason {
            AttentionReason::WaitingPermission => Reason::WaitingPermission,
            AttentionReason::WaitingInput => Reason::WaitingInput,
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

/// Whether this session wants the user, and what for.
///
/// The reason wins over the status: a session that died *after* reporting an
/// error is on the list for the error, which is the half of "failed" that says
/// something. `status = failed` is the one case the daemon raises no reason
/// for — the agent died without saying why — and it has always been on this
/// list. Kept identical to `sessionAttention` in the UI.
fn session_reason(session: &SessionDto) -> Option<Reason> {
    match session.attention_reason {
        Some(reason) => Some(reason.into()),
        None if session.status == SessionStatus::Failed => Some(Reason::Failed),
        None => None,
    }
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

/// The `--format json` document: goals → items, ready for scripting.
#[derive(Serialize)]
struct Attention {
    /// Rows in total, the number the UI's sidebar badge shows.
    count: usize,
    goals: Vec<Group>,
}

/// Everything one goal has that needs attention. Never empty.
#[derive(Serialize)]
struct Group {
    goal_id: String,
    /// The goal itself, when the goals list has it — a task or session can
    /// outlive its goal falling out of the list.
    goal: Option<GoalDto>,
    tasks: Vec<AttentionTask>,
    /// Sessions of this goal that want the user, its planner's included.
    sessions: Vec<AttentionSession>,
}

#[derive(Serialize)]
struct AttentionTask {
    reason: Reason,
    task: TaskDto,
}

#[derive(Serialize)]
struct AttentionSession {
    reason: Reason,
    session: SessionDto,
}

pub async fn run(client: &Client, no_trunc: bool, format: Format) -> Result<()> {
    let goals: Vec<GoalDto> = client.get_json("/v1/goals").await?;
    let tasks: Vec<TaskDto> = client.get_json("/v1/tasks").await?;
    // Unfiltered, and narrowed by `session_reason` below: the rule is
    // "flagged for attention *or* dead", and `GET /v1/sessions` ANDs its
    // `attention` filter with its `status` one, so no single request spells
    // it. The UI reads the same unfiltered list for the same reason.
    let sessions: Vec<SessionDto> = client.get_json("/v1/sessions").await?;

    let attention = group(goals, tasks, sessions);
    match format {
        Format::Json => print_json(&attention)?,
        Format::Table => {
            for (i, group) in attention.goals.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                println!("{}", heading(group));
                print_table(ROWS, &rows(group, chrono::Utc::now()), no_trunc);
            }
            if attention.goals.is_empty() {
                note("nothing needs attention");
            }
        }
    }
    Ok(())
}

/// The three lists as one document: goals first, newest first — the order the
/// UI shows them in — then any goal the goals list did not carry.
fn group(goals: Vec<GoalDto>, tasks: Vec<TaskDto>, sessions: Vec<SessionDto>) -> Attention {
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

    let order: std::collections::HashMap<&str, usize> = goals
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
fn heading(group: &Group) -> String {
    match &group.goal {
        Some(goal) => format!("{} ({})", goal.title, short_id(&goal.id)),
        None => format!("Goal {}", short_id(&group.goal_id)),
    }
}

/// One goal's rows: its tasks first, then its stuck sessions — the order
/// the UI lists them in.
fn rows(group: &Group, now: chrono::DateTime<chrono::Utc>) -> Vec<Vec<String>> {
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
            s.task_id.clone().unwrap_or_else(|| "-".into()),
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

/// How long ago that was, compactly: `12s`, `4m`, `3h`, `2d` — floored, never
/// rounded, so 89 seconds is "1m" and not the "2m" a rounding step would jump
/// to a second early. Anything unparseable is passed through, like
/// [`crate::output::local_time`] does.
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

    use ariadne_core::{AgentKind, Role};

    fn goal(id: &str, title: &str) -> GoalDto {
        GoalDto {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            status: ariadne_core::GoalStatus::Active,
            max_tasks: None,
            required_approvals: 1,
            planner_profile_id: "01PROFILE".into(),
            repos: Vec::new(),
            created_at: "2026-08-18T10:00:00Z".into(),
            updated_at: "2026-08-18T10:00:00Z".into(),
        }
    }

    fn task(id: &str, goal_id: &str, status: TaskStatus, stalled: bool) -> TaskDto {
        TaskDto {
            id: id.into(),
            goal_id: goal_id.into(),
            repo_id: "01REPO".into(),
            title: format!("task {id}"),
            description: String::new(),
            status,
            engineer_profile_id: "01PROFILE".into(),
            reviewer_profile_ids: Vec::new(),
            depends_on: Vec::new(),
            branch: "ariadne/x".into(),
            worktree_path: None,
            review_round: 0,
            stalled,
            merge_commit: None,
            created_at: "2026-08-18T10:00:00Z".into(),
            updated_at: "2026-08-18T10:00:00Z".into(),
        }
    }

    /// A session that is on the list for the bare reason it always was: the
    /// agent died. `flagged` puts a reason on top of it.
    fn session(id: &str, goal_id: &str, task_id: Option<&str>) -> SessionDto {
        SessionDto {
            id: id.into(),
            goal_id: goal_id.into(),
            task_id: task_id.map(Into::into),
            role: match task_id {
                Some(_) => Role::Engineer,
                None => Role::Planner,
            },
            profile_id: "01PROFILE".into(),
            agent_kind: AgentKind::ClaudeCode,
            internal_session_id: None,
            tmux_session: "ariadne-x".into(),
            worktree_path: None,
            review_round: None,
            status: SessionStatus::Failed,
            attention_reason: None,
            attention_since: None,
            last_activity_at: None,
            created_at: "2026-08-18T10:00:00Z".into(),
            ended_at: None,
        }
    }

    /// A live session the daemon has flagged: still running, still on the list.
    fn flagged(id: &str, goal_id: &str, reason: AttentionReason) -> SessionDto {
        SessionDto {
            status: SessionStatus::Running,
            attention_reason: Some(reason),
            attention_since: Some("2026-08-18T11:00:00Z".into()),
            ..session(id, goal_id, Some("01T9"))
        }
    }

    /// The reasons the UI reports, in its precedence: a stalled task that
    /// also failed is failed, and a healthy task is nobody's business.
    #[test]
    fn a_task_is_reported_for_the_reason_the_ui_would_give() {
        let reason = |status, stalled| task_reason(&task("01T", "01G", status, stalled));
        assert_eq!(reason(TaskStatus::Failed, false), Some(Reason::Failed));
        assert_eq!(reason(TaskStatus::Failed, true), Some(Reason::Failed));
        assert_eq!(
            reason(TaskStatus::ChangesRequested, false),
            Some(Reason::ChangesRequested)
        );
        assert_eq!(reason(TaskStatus::InProgress, true), Some(Reason::Stalled));
        assert_eq!(reason(TaskStatus::InProgress, false), None);
        assert_eq!(reason(TaskStatus::Merged, false), None);
    }

    /// The reasons the UI reports for a session, in its precedence: the
    /// flag when there is one, the death when there is not, and a working
    /// agent is nobody's business.
    #[test]
    fn a_session_is_reported_for_the_reason_the_ui_would_give() {
        for (flag, expected) in [
            (
                AttentionReason::WaitingPermission,
                Reason::WaitingPermission,
            ),
            (AttentionReason::WaitingInput, Reason::WaitingInput),
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

        // Dead without a word about why: the one case with no flag on it.
        assert_eq!(
            session_reason(&session("01S", "01GA", None)),
            Some(Reason::Failed)
        );
        // A flag survives the death that followed it, and outranks it.
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
            ..session("01S", "01GA", None)
        };
        assert_eq!(session_at(&died), "2026-08-18T12:00:00Z");

        // Failed, but the daemon never stamped an end on it.
        assert_eq!(
            session_at(&session("01S", "01GA", None)),
            "2026-08-18T10:00:00Z"
        );
    }

    #[test]
    fn healthy_tasks_and_sessions_produce_no_groups_at_all() {
        let working = SessionDto {
            status: SessionStatus::Running,
            ..session("01S1", "01GA", Some("01T1"))
        };
        let attention = group(
            vec![goal("01GA", "A")],
            vec![task("01T1", "01GA", TaskStatus::InProgress, false)],
            vec![working],
        );
        assert_eq!(attention.count, 0);
        assert!(attention.goals.is_empty());
    }

    /// Goals come newest first — ids are ULIDs, so the biggest id is the
    /// newest goal — and a goal the goals list does not carry comes last,
    /// with the goal field empty rather than the row dropped.
    #[test]
    fn groups_follow_the_goal_order_the_ui_shows() {
        let attention = group(
            vec![goal("01GA", "older"), goal("01GB", "newer")],
            vec![
                task("01T1", "01GA", TaskStatus::Failed, false),
                task("01T2", "01GB", TaskStatus::ChangesRequested, false),
                task("01T3", "01GONE", TaskStatus::Failed, false),
            ],
            vec![session("01S1", "01GB", None)],
        );
        let ids: Vec<&str> = attention.goals.iter().map(|g| g.goal_id.as_str()).collect();
        assert_eq!(ids, ["01GB", "01GA", "01GONE"]);
        assert_eq!(attention.count, 4);
        assert_eq!(
            attention.goals[0].goal.as_ref().map(|g| g.title.as_str()),
            Some("newer")
        );
        assert!(attention.goals[2].goal.is_none());
    }

    /// The count is what the UI's badge shows: every task row and every
    /// stuck-session row, planner sessions included.
    #[test]
    fn a_planner_session_lands_in_its_goals_group() {
        let attention = group(
            vec![goal("01GA", "A")],
            Vec::new(),
            vec![session("01S1", "01GA", None)],
        );
        assert_eq!(attention.count, 1);
        assert_eq!(attention.goals[0].sessions[0].session.role, Role::Planner);
    }

    #[test]
    fn a_session_row_names_its_role_and_task() {
        let g = &group(
            vec![goal("01GA", "A")],
            Vec::new(),
            vec![session("01S1", "01GA", Some("01T9"))],
        )
        .goals[0];
        let now = chrono::Utc::now();
        let row = &rows(g, now)[0];
        assert_eq!(row[0], "01S1");
        assert_eq!(row[1], "engineer session");
        assert_eq!(row[2], "failed");
        assert_eq!(row[3], "01T9");
    }

    /// The wording the UI's `SESSION_ATTENTION_META` labels lowercase to.
    #[test]
    fn a_flagged_session_row_spells_the_reason_the_ui_spells() {
        let flags = [
            (AttentionReason::WaitingPermission, "waiting for permission"),
            (AttentionReason::WaitingInput, "waiting for input"),
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
        let rows = rows(g, chrono::Utc::now());
        let labels: Vec<&str> = rows.iter().map(|row| row[2].as_str()).collect();
        assert_eq!(labels, flags.map(|(_, label)| label));
    }

    #[test]
    fn the_heading_is_the_title_or_the_bare_short_id() {
        let with = Group {
            goal_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            goal: Some(goal("01ARZ3NDEKTSV4RRFFQ69G5FAV", "CLI vs App")),
            tasks: Vec::new(),
            sessions: Vec::new(),
        };
        assert_eq!(heading(&with), "CLI vs App (…Q69G5FAV)");
        let without = Group { goal: None, ..with };
        assert_eq!(heading(&without), "Goal …Q69G5FAV");
    }

    /// The same shortening the UI's `shortId` does: short ids stay whole.
    #[test]
    fn a_short_id_keeps_the_tail_that_tells_ids_apart() {
        assert_eq!(short_id("01ARZ3NDEKTSV4RRFFQ69G5FAV"), "…Q69G5FAV");
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
            vec![task("01T1", "01GA", TaskStatus::ChangesRequested, false)],
            Vec::new(),
        );
        let doc = serde_json::to_value(&attention).expect("serialize");
        assert_eq!(doc["count"], 1);
        assert_eq!(doc["goals"][0]["goal_id"], "01GA");
        assert_eq!(doc["goals"][0]["goal"]["title"], "A");
        assert_eq!(doc["goals"][0]["tasks"][0]["reason"], "changes_requested");
        assert_eq!(doc["goals"][0]["tasks"][0]["task"]["id"], "01T1");

        let attention = group(
            vec![goal("01GA", "A")],
            Vec::new(),
            vec![flagged("01S1", "01GA", AttentionReason::WaitingPermission)],
        );
        let doc = serde_json::to_value(&attention).expect("serialize");
        assert_eq!(
            doc["goals"][0]["sessions"][0]["reason"],
            "waiting_permission"
        );
        assert_eq!(doc["goals"][0]["sessions"][0]["session"]["id"], "01S1");
    }
}
