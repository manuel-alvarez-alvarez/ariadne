//! What a task waiting on its dependencies does when one of them ends.
//!
//! A `pending` task is waiting for every dependency to merge. Two of the
//! statuses a dependency can end in are not that — `failed` and `cancelled` —
//! and neither of them is ever going to become one: the wait behind such a
//! dependency is over before it started, so the task ends too, saying which
//! dependency stopped it, and the user retries it once that one has landed.
//!
//! The scheduler is started after the seeding rather than with the harness,
//! so that the pass a test asks for is the first one over the state it just
//! wrote — the exception is the goal-cancellation test, which cancels over
//! HTTP and wants the handler's own event to reach a scheduler the router
//! knows about.

mod common;

use std::ops::Deref;

use ariadne_core::{Actor, Role, TaskStatus};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_store::{Goal, NewTask, ReviewerSlot, Task};

use common::{Harness, TIMEOUT, eventually, harness, post};

/// An active goal with two tasks on it: one the other waits for.
struct World {
    h: Harness,
    goal: Goal,
    /// The dependency: the task the second one is waiting on.
    first: Task,
    /// The dependent: `pending` until the first one merges.
    second: Task,
}

impl Deref for World {
    type Target = Harness;
    fn deref(&self) -> &Harness {
        &self.h
    }
}

impl World {
    async fn new() -> World {
        World::on(harness().await).await
    }

    async fn on(h: Harness) -> World {
        let planner = h.profile("planner", Role::Planner).await;
        let engineer = h.profile("engineer", Role::Engineer).await;
        let reviewer = h.profile("reviewer", Role::Reviewer).await;
        let (goal, repo) = h.goal(&planner).await;
        let first = h
            .task_on(&goal, &repo, "Build the engine", &engineer, &[&reviewer])
            .await;
        let second = h
            .store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo.id.clone(),
                title: "Drive what the engine built".into(),
                description: "do things".into(),
                engineer_profile_id: engineer.id.clone(),
                pin: None,
                reviewers: vec![ReviewerSlot::of(&reviewer.id)],
                depends_on: vec![first.id.clone()],
            })
            .await
            .unwrap();
        let goal = h.activate(&goal).await;
        World {
            h,
            goal,
            first,
            second,
        }
    }

    /// The scheduler, started over everything seeded so far.
    fn scheduler(&self) -> tokio::sync::mpsc::UnboundedSender<SchedEvent> {
        scheduler::start(self.store.clone(), self.launcher.clone(), false)
    }

    /// End the dependency where the daemon's own spawn budget or the user
    /// would end it: a task in progress that fails, or one that is cancelled.
    async fn end_first(&self, status: TaskStatus) {
        let (actor, reason) = match status {
            TaskStatus::Cancelled => (Actor::User, "cancelled by user"),
            _ => (Actor::Daemon, "the agent could not be started"),
        };
        self.advance(&self.first, TaskStatus::InProgress).await;
        self.store
            .transition_task(&self.first.id, status, actor, Some(reason), None)
            .await
            .unwrap();
    }

    /// What the second task's thread says, as a single body naming the first
    /// task — the notice the daemon writes for an ending.
    async fn notice(&self) -> String {
        self.thread(&self.second)
            .await
            .into_iter()
            .find(|body| body.contains(&self.first.title))
            .unwrap_or_else(|| panic!("no notice naming \"{}\"", self.first.title))
    }
}

/// A dependency that failed ends the task waiting on it, and the thread says
/// which dependency it was.
#[tokio::test]
async fn a_failed_dependency_fails_the_task_waiting_on_it() {
    let w = World::new().await;
    w.end_first(TaskStatus::Failed).await;

    let sched = w.scheduler();
    sched
        .send(SchedEvent::TaskChanged(w.second.id.clone()))
        .unwrap();

    eventually(TIMEOUT, "the waiting task to fail", async || {
        w.status(&w.second.id).await == TaskStatus::Failed
    })
    .await;
    let notice = w.notice().await;
    assert!(
        notice.contains(&format!("dependency \"{}\"", w.first.title)) && notice.contains("failed"),
        "the notice does not say the dependency failed: {notice}"
    );
    assert!(
        notice.contains(&w.first.id[w.first.id.len() - 8..]),
        "the notice does not name the dependency's id: {notice}"
    );
    assert_eq!(
        w.store
            .list_task_transitions(&w.second.id)
            .await
            .unwrap()
            .last()
            .and_then(|t| t.reason.clone())
            .as_deref()
            .map(|r| r.starts_with(&format!("dependency \"{}\"", w.first.title))),
        Some(true),
        "the transition itself carries the reason"
    );
}

/// And so does one that was cancelled — said in those words, since nothing
/// failed.
#[tokio::test]
async fn a_cancelled_dependency_fails_the_task_waiting_on_it() {
    let w = World::new().await;
    w.end_first(TaskStatus::Cancelled).await;

    let sched = w.scheduler();
    sched
        .send(SchedEvent::TaskChanged(w.second.id.clone()))
        .unwrap();

    eventually(TIMEOUT, "the waiting task to fail", async || {
        w.status(&w.second.id).await == TaskStatus::Failed
    })
    .await;
    let notice = w.notice().await;
    assert!(
        notice.contains(&format!("dependency \"{}\"", w.first.title))
            && notice.contains("was cancelled"),
        "the notice does not say the dependency was cancelled: {notice}"
    );
}

/// The failure is the user's to undo: with the dependency retried and merged,
/// retrying the task behind it starts it rather than failing it again.
#[tokio::test]
async fn a_task_retried_after_its_dependency_landed_is_not_failed_again() {
    let w = World::new().await;
    w.end_first(TaskStatus::Failed).await;

    let sched = w.scheduler();
    sched
        .send(SchedEvent::TaskChanged(w.second.id.clone()))
        .unwrap();
    eventually(TIMEOUT, "the waiting task to fail", async || {
        w.status(&w.second.id).await == TaskStatus::Failed
    })
    .await;

    // The dependency retried and taken all the way to merged. The scheduler
    // is running over the same task, so a move it has already made is not a
    // failure of the walk: only the merge at the end of it is this test's.
    w.store
        .transition_task(&w.first.id, TaskStatus::Ready, Actor::User, None, None)
        .await
        .unwrap();
    for (status, actor) in [
        (TaskStatus::InProgress, Actor::Daemon),
        (TaskStatus::UnderReview, Actor::Engineer),
        (TaskStatus::Approved, Actor::Daemon),
    ] {
        let _ = w
            .store
            .transition_task(&w.first.id, status, actor, None, None)
            .await;
    }
    w.store
        .transition_task(
            &w.first.id,
            TaskStatus::Merged,
            Actor::Engineer,
            None,
            Some("abc123"),
        )
        .await
        .unwrap();

    // And now the retry of the task that was waiting: nothing is blocking it
    // any more, so it starts.
    w.store
        .transition_task(
            &w.second.id,
            TaskStatus::Ready,
            Actor::User,
            Some("retried by user"),
            None,
        )
        .await
        .unwrap();
    sched
        .send(SchedEvent::TaskChanged(w.second.id.clone()))
        .unwrap();
    eventually(TIMEOUT, "the retried task to start", async || {
        matches!(
            w.status(&w.second.id).await,
            TaskStatus::Ready | TaskStatus::InProgress
        )
    })
    .await;
}

/// A goal being cancelled cancels every task on it, dependents included: the
/// rule above never sees them, because a pass over a goal that is no longer
/// active does nothing at all.
#[tokio::test]
async fn cancelling_the_goal_leaves_every_task_cancelled_and_none_failed() {
    let w = World::on(harness().scheduler().await).await;

    let (status, body) = w
        .send(post(&format!("/v1/goals/{}/cancel", w.goal.id)))
        .await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

    eventually(TIMEOUT, "every task to be cancelled", async || {
        w.status(&w.first.id).await == TaskStatus::Cancelled
            && w.status(&w.second.id).await == TaskStatus::Cancelled
    })
    .await;
}
