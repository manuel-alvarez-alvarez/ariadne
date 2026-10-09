//! Whether a session is still the agent the work is waiting on.
//!
//! Attention on a session means "a human must act", and that is only true
//! while the work the session was started for is still its own to do. Both
//! detectors ask the same question: the sweep that flags a vanished agent,
//! and the event ingestion that flags a permission request. A reviewer that
//! has voted and an author whose task is under review are agents nobody is
//! waiting on, whatever they ask.

use ariadne_core::{GoalStatus, Seat, TaskStatus};
use ariadne_store::{AgentSession, Store, Task};

/// Whether the work this session was started for is still going.
///
/// A question about the seat and not only about the status: an author whose
/// task sits under review costs nobody anything — the reviewers are the ones
/// working, and the author is woken by id when they answer — and a reviewer
/// that has already voted is done however long the round runs on.
pub async fn work_is_active(store: &Store, session: &AgentSession) -> bool {
    match session.seat() {
        None => session.status().is_live(),
        Some(Seat::Agent) => {
            let Some(task) = task_of(store, session).await else {
                return false;
            };
            let Some(id) = &session.task_agent_id else {
                return false;
            };
            task.status() == TaskStatus::InProgress
                && task.step.is_some()
                && store
                    .get_task_agent(id)
                    .await
                    .is_ok_and(|a| a.step == task.step)
        }
        // The goal is the orchestrator's whole job, and it holds that job
        // for the whole goal: the plan is a hand-off, not an ending. It is
        // the agent the user talks to about work already running, and the
        // one the daemon tells when a task needs a decision, so an agent of
        // its own that vanishes under a goal still going is news.
        Some(Seat::Orchestrator) => matches!(
            store
                .get_goal(session.goal_id.as_deref().unwrap_or_default())
                .await
                .map(|g| g.status()),
            Ok(GoalStatus::Planning | GoalStatus::Active)
        ),
        // Every status the author is working in or about to be woken for;
        // `pending` has no author yet and `under_review` is not its turn.
        // `approved` is: landing the change is the author's last job.
        // A pull request session works for its request while Ariadne does
        // (026): what it raises — the request ready to merge — is owed until
        // a human merged or closed it, which takes the request's row away.
        Some(Seat::Author | Seat::Reviewer) if session.pull_request_id.is_some() => store
            .get_pull_request(session.pull_request_id.as_deref().unwrap_or_default())
            .await
            .is_ok(),
        Some(Seat::Author) => match task_of(store, session).await {
            Some(task) => matches!(
                task.status(),
                TaskStatus::Ready
                    | TaskStatus::InProgress
                    | TaskStatus::ChangesRequested
                    | TaskStatus::Approved
            ),
            None => false,
        },
        // A reviewer is only owed to a review it has not voted on.
        Some(Seat::Reviewer) => match task_of(store, session).await {
            Some(task) if task.status() == TaskStatus::UnderReview => {
                store.open_verdicts(&task.id).await.is_ok_and(|verdicts| {
                    !verdicts
                        .iter()
                        .any(|m| m.from_agent_id.as_ref() == session.task_agent_id.as_ref())
                })
            }
            _ => false,
        },
    }
}

async fn task_of(store: &Store, session: &AgentSession) -> Option<Task> {
    let task_id = session.task_id.as_deref()?;
    store.get_task(task_id).await.ok()
}
