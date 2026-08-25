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

use anyhow::Result;
use serde::Serialize;

use ariadne_api::goals::GoalDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::tasks::TaskDto;
use ariadne_client::Client;
use ariadne_core::{AttentionReason, TaskStatus};

use board::{ROWS, group, heading, rows, task_titles};
use crate::output::{Format, note, print_json, print_table};

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
/// spoken and the daemon resumes the engineer itself, so that task waits on an
/// agent. A resume that does not happen shows up as the session's own
/// `disconnected` or `stalled` flag. And `stalled` is checked last because it
/// is a flag on top of a status, so a task that also failed reads as failed.
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

pub async fn run(client: &Client, no_trunc: bool, format: Format) -> Result<()> {
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
    match format {
        Format::Json => print_json(&attention)?,
        Format::Table => {
            for (i, group) in attention.goals.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                println!("{}", heading(group));
                print_table(ROWS, &rows(group, &titles, chrono::Utc::now()), no_trunc);
            }
            if attention.goals.is_empty() {
                note("nothing needs attention");
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
    /// review asked for changes or whose engineer is landing it. Inventing a
    /// reason there from the bare status is exactly the disagreement with the
    /// UI this list exists not to have.
    #[test]
    fn a_task_is_reported_for_the_reason_the_ui_would_give() {
        let reason = |status, stalled| task_reason(&task("01T", "01G", status, stalled));
        assert_eq!(reason(TaskStatus::Failed, false), Some(Reason::Failed));
        assert_eq!(reason(TaskStatus::Failed, true), Some(Reason::Failed));
        assert_eq!(reason(TaskStatus::InProgress, true), Some(Reason::Stalled));
        assert_eq!(reason(TaskStatus::InProgress, false), None);
        assert_eq!(reason(TaskStatus::Merged, false), None);

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

}
