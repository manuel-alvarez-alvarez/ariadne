//! What the daemon says to the user, written where it happened.
//!
//! A notice is a message like any other: it goes into the thread it belongs
//! to, it names the user as its recipient where the user is who it is for,
//! and it is the delivery path — never the writer — that puts it on the
//! attention strip (`Scheduler::raise_for_user`). So every one of these
//! returns the message it wrote, for the caller to hand to the scheduler by
//! whichever route it has.
//!
//! What is here is the endings: the moments a task or a goal stops being
//! anybody's to work on, and the one where a goal never got started at all.
//! Everything up to then has an agent behind it that the user can watch; an
//! ending has none — the sessions are killed with it — and until this a task
//! that exhausted its spawn budget died in a log line.

use ariadne_core::{AuthorRole, TaskStatus};
use ariadne_store::{Goal, Message, NewMessage, Recipient, Result, Store, Task};

/// A transition's reason as a sentence quotes it, or the stand-in for one
/// that carried none.
fn reason_or_unsaid(reason: Option<&str>) -> &str {
    reason
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .unwrap_or("no reason was given")
}

/// Tell the user a task has ended, and what ended it.
///
/// Written for the three endings and no other status, so the caller can hand
/// it every transition it makes: `merged` names the commit the change landed
/// as, `failed` and `cancelled` the reason the transition carried. `None`
/// means this was not an ending and nothing was written.
pub async fn task_ended(
    store: &Store,
    task: &Task,
    reason: Option<&str>,
) -> Result<Option<Message>> {
    let title = &task.title;
    let body = match task.status() {
        TaskStatus::Merged => match &task.merge_commit {
            Some(commit) => format!("\"{title}\" is merged, as {commit}."),
            None => format!("\"{title}\" is merged."),
        },
        TaskStatus::Failed => format!(
            "\"{title}\" failed: {}.\n\nIts branch and worktree are kept, so what was \
             written on it is still there — retry the task once you have dealt with \
             what stopped it.",
            reason_or_unsaid(reason)
        ),
        TaskStatus::Cancelled => format!(
            "\"{title}\" was cancelled: {}.\n\nIts branch and worktree are kept, so what \
             was written on it is still there.",
            reason_or_unsaid(reason)
        ),
        _ => return Ok(None),
    };
    let message = store
        .create_message(NewMessage {
            goal_id: task.goal_id.clone(),
            task_id: Some(task.id.clone()),
            author_role: AuthorRole::System,
            author_session_id: None,
            recipient: Some(Recipient::User),
            body,
        })
        .await?;
    Ok(Some(message))
}

/// And tell the goal's own thread that there is nothing left of it.
///
/// Addressed to nobody: the planner that would read it is being killed in the
/// same breath, and the user reads the goal itself. It is the thread's last
/// line rather than a notice anybody is woken for.
pub async fn goal_completed(store: &Store, goal: &Goal) -> Result<Message> {
    store
        .create_message(NewMessage {
            goal_id: goal.id.clone(),
            task_id: None,
            author_role: AuthorRole::System,
            author_session_id: None,
            recipient: None,
            body: format!(
                "\"{}\" is complete: every task of it is merged or cancelled.",
                goal.title
            ),
        })
        .await
}

/// And tell it that its planner will not be started again.
///
/// Addressed to nobody, like the goal's own ending: there is no planner to
/// read it — that is what it says — and the session the last attempt left
/// behind already carries the flag the user acts on. What this adds is the
/// why, in the one place a goal that never got planned has to say it.
pub async fn planner_gave_up(store: &Store, goal: &Goal, attempts: u32) -> Result<Message> {
    store
        .create_message(NewMessage {
            goal_id: goal.id.clone(),
            task_id: None,
            author_role: AuthorRole::System,
            author_session_id: None,
            recipient: None,
            body: format!(
                "The planner for \"{}\" could not be started, and {attempts} attempts is \
                 all it gets: nothing more will be tried, and its last session is flagged \
                 for you. Deal with what stopped it — the agent CLI the goal runs on, the \
                 model it was given — and resume that session to have another go.",
                goal.title
            ),
        })
        .await
}
