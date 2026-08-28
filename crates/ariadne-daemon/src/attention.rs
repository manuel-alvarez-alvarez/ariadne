//! Whether a session is still the agent the work is waiting on.
//!
//! Attention on a session means "a human must act", and that is only true
//! while the work the session was started for is still its own to do. Both
//! detectors ask the same question: the sweep that flags a vanished pane, and
//! the event ingestion that flags a permission prompt. A reviewer that has
//! voted, an engineer whose task is under review and a planner whose goal has
//! left planning are all agents nobody is waiting on, whatever their pane
//! puts on the screen.

use ariadne_core::{GoalStatus, Role, TaskStatus};
use ariadne_store::{AgentSession, Store, Task};

/// Whether the work this session was started for is still going.
///
/// A question about the role and not only about the status: an engineer whose
/// task sits under review costs nobody anything — the reviewers are the ones
/// working, and the engineer is woken by id when they answer — and a reviewer
/// that has already voted is done however long the round runs on.
pub async fn work_is_active(store: &Store, session: &AgentSession) -> bool {
    match session.role() {
        // The goal being planned is the planner's whole job, and finalizing
        // the plan is what ends it.
        Role::Planner => matches!(
            store.get_goal(&session.goal_id).await.map(|g| g.status()),
            Ok(GoalStatus::Planning)
        ),
        // Every status the engineer is working in or about to be woken for;
        // `pending` has no engineer yet and `under_review` is not its turn.
        // `approved` is: landing the change is the engineer's last job.
        Role::Engineer => match task_of(store, session).await {
            Some(task) => matches!(
                task.status(),
                TaskStatus::Ready
                    | TaskStatus::InProgress
                    | TaskStatus::ChangesRequested
                    | TaskStatus::Approved
            ),
            None => false,
        },
        // A reviewer is only owed to a round it has not voted in.
        Role::Reviewer => match task_of(store, session).await {
            Some(task) if task.status() == TaskStatus::UnderReview => store
                .list_reviews(&task.id, Some(task.review_round))
                .await
                .is_ok_and(|reviews| {
                    !reviews
                        .iter()
                        .any(|r| r.reviewer_profile_id == session.profile_id)
                }),
            _ => false,
        },
    }
}

async fn task_of(store: &Store, session: &AgentSession) -> Option<Task> {
    let task_id = session.task_id.as_deref()?;
    store.get_task(task_id).await.ok()
}
