//! Whether a session is still the agent the work is waiting on.
//!
//! Attention on a session means "a human must act", and that is only true
//! while the work the session was started for is still its own to do. Both
//! detectors ask the same question: the sweep that flags a vanished agent,
//! and the event ingestion that flags a permission request. The agent of a
//! column the task has moved past is an agent nobody is waiting on, whatever
//! it asks.

pub(crate) mod agent_requests;
pub(crate) mod pull_requests;
pub(crate) mod recovery;

use ariadne_api::attention::AttentionListDto;
use ariadne_core::{GoalStatus, Seat, TaskStatus};
use ariadne_store::{AgentSession, Store, Task};

use crate::launcher::Launcher;

/// The whole "Needs attention" list `GET /v1/attention` answers: every item
/// every registered producer currently finds.
///
/// Registering a producer is adding a call here and extending it, the one
/// extension point this route gives a later task: [`recovery`] is the
/// first complete one; [`agent_requests`] and [`pull_requests`] are
/// registered already but answer empty until a later task gives them their
/// eligibility rules (009, 026, 029). A producer whose read fails costs
/// the list its `complete` flag rather than its other producers' items, so
/// a partial read never answers as if nothing were wrong.
pub async fn collect(store: &Store, launcher: &Launcher) -> AttentionListDto {
    let mut items = Vec::new();
    let mut complete = true;
    match recovery::items(store, launcher).await {
        Ok(found) => items.extend(found),
        Err(error) => {
            tracing::warn!(%error, "the recovery attention producer could not read its evidence");
            complete = false;
        }
    }
    match agent_requests::items(store).await {
        Ok(found) => items.extend(found),
        Err(error) => {
            tracing::warn!(%error, "the agent-request attention producer could not read its evidence");
            complete = false;
        }
    }
    match pull_requests::items(store).await {
        Ok(found) => items.extend(found),
        Err(error) => {
            tracing::warn!(%error, "the pull-request attention producer could not read its evidence");
            complete = false;
        }
    }
    AttentionListDto { items, complete }
}

/// Whether the work this session was started for is still going.
///
/// A question about the seat and not only about the status: the agent of a
/// column the task is not in costs nobody anything — the current column's
/// agent is the one working, and the others sit idle until the task comes
/// back to them.
pub async fn work_is_active(store: &Store, session: &AgentSession) -> bool {
    match session.seat() {
        None => session.status().is_live(),
        // Only the current column's agent is owed anything.
        Some(Seat::Agent) => {
            let Some(task) = task_of(store, session).await else {
                return false;
            };
            let Some(id) = &session.task_agent_id else {
                return false;
            };
            task.status() == TaskStatus::InProgress
                && store
                    .get_task_agent(id)
                    .await
                    .is_ok_and(|a| Some(a.step.as_str()) == task.step.as_deref())
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
        // A pull request session works for its request while Ariadne does
        // (029): what it raises is owed until a human merged or closed the
        // request, which takes the request's row away.
        Some(Seat::Reviewer) => match &session.pull_request_id {
            Some(pull_request_id) => store.get_pull_request(pull_request_id).await.is_ok(),
            None => false,
        },
    }
}

async fn task_of(store: &Store, session: &AgentSession) -> Option<Task> {
    let task_id = session.task_id.as_deref()?;
    store.get_task(task_id).await.ok()
}
