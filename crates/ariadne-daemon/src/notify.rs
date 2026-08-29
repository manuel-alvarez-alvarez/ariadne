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
//!
//! And one thing that is not the daemon's own: the words a planner ended its
//! turn on ([`planner_ended_its_turn`]). Composed by nobody here — the daemon
//! only relays what the agent already said, into the thread the planner would
//! have posted it to itself — but written on the user's behalf all the same,
//! and travelling the same way once it is written.

use ariadne_core::{AuthorRole, GoalStatus, Role, TaskStatus};
use ariadne_store::{AgentSession, Goal, Message, NewMessage, Recipient, Result, Store, Task};

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

/// The words a planner ended its turn on, put into its goal's thread as a
/// message for the user.
///
/// The one thing here the daemon did not compose. A planner that ends its
/// turn on a plain-text question — no `post_message`, no `AskUserQuestion` —
/// leaves no trace anywhere the user looks: the thread stays empty and
/// nothing goes up on the attention strip, so the question is only ever found
/// by opening the pane. The text is in the daemon's hands either way (the
/// idle event carries it, see
/// [`crate::http::classify::last_assistant_message`]), so it is written as
/// the message the planner would have posted itself — the same shape a
/// `post_message` to "user" produces — and the delivery path raises it from
/// there, the way it raises every other message addressed to the user.
///
/// What makes the guess safe is the status. A planner whose turn ends while
/// its goal is still in planning is by construction waiting for the user: it
/// did not call `finalize_plan`, so it either asked something or is showing
/// them the plan. `None` for anything else, and nothing is written:
///
/// - another role — an engineer or a reviewer that ends a turn is waiting for
///   the daemon's nudge, not for a person, and flagging it for the user takes
///   it out of the watchdog that would have nudged it;
/// - a goal past planning, where a planner that has finalized is nobody's to
///   wake;
/// - text that is empty or nothing but whitespace;
/// - text the planner posted itself during this same turn, which is what
///   keeps a `post_message` to "user" followed by the same words at the end
///   of the turn from arriving in the thread twice.
///
/// `turn_began_at` is when the turn that is ending began — the timestamp of
/// the session's last turn-start event ([`crate::http::classify::TURN_STARTS`]).
/// It is what keeps that last exception to one turn: a planner that asked
/// something, was answered, and asks the very same thing again turns later is
/// asking it afresh, and suppressing it would leave the user waiting on a
/// question nothing shows. `None` — a session with no turn-start recorded at
/// all — deduplicates nothing, since a line said twice in the thread costs
/// the user a glance and a question swallowed costs them the goal.
pub async fn planner_ended_its_turn(
    store: &Store,
    session: &AgentSession,
    text: &str,
    turn_began_at: Option<&str>,
) -> Result<Option<Message>> {
    let body = text.trim();
    if session.role() != Role::Planner || body.is_empty() {
        return Ok(None);
    }
    if store.get_goal(&session.goal_id).await?.status() != GoalStatus::Planning {
        return Ok(None);
    }
    if let Some(began) = turn_began_at
        && store
            .last_goal_message_from(&session.goal_id, &session.id)
            .await?
            .is_some_and(|last| last.body.trim() == body && last.created_at.as_str() >= began)
    {
        return Ok(None);
    }
    let message = store
        .create_message(NewMessage {
            goal_id: session.goal_id.clone(),
            task_id: None,
            author_role: AuthorRole::Planner,
            author_session_id: Some(session.id.clone()),
            recipient: Some(Recipient::User),
            body: body.to_string(),
        })
        .await?;
    Ok(Some(message))
}
