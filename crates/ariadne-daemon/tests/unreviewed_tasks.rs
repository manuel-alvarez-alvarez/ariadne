//! A task with nothing to review.
//!
//! Most work is worth a second pair of eyes, and a task is staffed with a
//! reviewer for that reason. Some work is not: a release, a dependency bump
//! the suite already judged, a report already filed. Such a task is staffed
//! with an author alone, and what makes it a task rather than a special case
//! is that it walks the same states — it is simply approved as soon as its
//! author asks, because there is nobody to ask.
//!
//! The goal's `required_approvals` is a ceiling over the plan, so it is what
//! the task's own reviewers can give that decides: as many as the goal asks
//! for, never more than there are reviewers to give them.

mod common;

use ariadne_core::{ReviewVerdict, TaskStatus};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_store::NewReview;

use common::{TIMEOUT, eventually, harness};

/// A task staffed with an author and no reviewer is approved the moment it
/// asks for review, and lands from there like any other.
#[tokio::test]
async fn a_task_with_no_reviewer_is_approved_as_soon_as_its_author_asks() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let task = h.task_on(&goal, &repo, "Cut the release", 0, None).await;
    h.activate(&goal).await;
    h.advance(&task, TaskStatus::UnderReview).await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();

    eventually(TIMEOUT, "the task to be approved", || async {
        h.store.get_task(&task.id).await.unwrap().status() == TaskStatus::Approved
    })
    .await;

    // And no review was invented to get it there: an approval nobody gave is
    // not one the history should carry.
    assert!(
        h.store
            .list_reviews(&task.id, None)
            .await
            .unwrap()
            .is_empty(),
        "a task with no reviewer collected a review"
    );
}

/// A goal that asks for two approvals, on a task with one reviewer, is
/// approved by that one: the goal's number is a ceiling, not a demand the
/// staffing cannot meet.
#[tokio::test]
async fn a_task_needs_no_more_approvals_than_it_has_reviewers_to_give() {
    let h = harness().await;
    let repo = h.repository(&h.at("repo")).await;
    let goal = h.goal_on(&repo, None).await;
    let task = h.task_on(&goal, &repo, "Wire it up", 1, None).await;
    let reviewer = h
        .store
        .list_task_reviewers(&task.id)
        .await
        .unwrap()
        .remove(0);
    h.activate(&goal).await;
    h.advance(&task, TaskStatus::UnderReview).await;
    // The round the request opened, not the one the task was created on:
    // asking for review is what starts a round.
    let under_review = h.store.get_task(&task.id).await.unwrap();
    h.store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: under_review.review_round,
            reviewer_agent_id: reviewer.id.clone(),
            session_id: None,
            verdict: ReviewVerdict::Approve,
            body: None,
        })
        .await
        .unwrap();

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();

    eventually(TIMEOUT, "the task to be approved", || async {
        h.store.get_task(&task.id).await.unwrap().status() == TaskStatus::Approved
    })
    .await;
}
