//! What clients hear when a task branch moves.
//!
//! Nothing in the store changes when an engineer commits, so the daemon
//! watches the branch ref itself and publishes `task_branch_updated` off that
//! watch. `git` is real here: the commits are made in the engineer's worktree
//! by the test, exactly where the agent would have made them. `tmux` is the
//! stub, so the engineer is a row and a spawn plan rather than a pane.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ariadne_api::stream::DomainEvent;
use ariadne_core::{Actor, TaskStatus};
use ariadne_daemon::bus::BusEvent;
use ariadne_store::Task;
use tokio::sync::broadcast::Receiver;

use common::{Harness, Sse, TIMEOUT, eventually, get, harness, next_sse, parse_sse, sh};

/// How long one commit is given to come back over the stream: a filesystem
/// notification, a debounce and a couple of `git` processes, on a machine
/// running the rest of the suite beside it.
const PATIENCE: Duration = Duration::from_secs(20);

/// How long the stream is watched for an event that must not be there.
const SILENCE: Duration = Duration::from_secs(2);

/// A task whose engineer has been spawned: a real repository, a worktree
/// checked out on the task branch, and the daemon following it.
///
/// The harness is the caller's, so that the tests about what the scheduler
/// does can run one.
async fn at_work(h: &Harness) -> (Task, PathBuf, PathBuf) {
    let repo = h.git_repo("repo");
    let cast = h.active_cast().await;
    h.launcher.spawn_engineer(&cast.task.id).await.unwrap();
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let worktree = task.worktree_path.clone().expect("the engineer has a worktree");
    (task, repo, PathBuf::from(worktree))
}

/// A commit in the engineer's worktree, and the sha it landed as.
fn commit(worktree: &Path, what: &str) -> String {
    sh(
        worktree,
        &format!(
            "echo {what} > {what}.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm {what} && git rev-parse HEAD"
        ),
    )
}

/// The next event on the stream that says something about the daemon's state.
///
/// The heartbeats are skipped: one opens every connection and another follows
/// every 15 idle seconds, and neither is about the branch being watched here.
async fn next_event(
    body: &mut axum::body::Body,
    within: Duration,
) -> Option<(String, serde_json::Value)> {
    let deadline = Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let Sse::Message(message) = next_sse(body, left).await else {
            return None;
        };
        let (kind, payload) = parse_sse(&message);
        if kind != "heartbeat" {
            return Some((kind, payload));
        }
    }
}

/// The head of the next `task_branch_updated` on the stream, or a failed test.
async fn next_head(body: &mut axum::body::Body, task: &Task) -> String {
    let Some((kind, payload)) = next_event(body, PATIENCE).await else {
        panic!("no task_branch_updated within the patience");
    };
    assert_eq!(kind, "task_branch_updated");
    assert_eq!(payload["task_id"], task.id);
    assert_eq!(payload["goal_id"], task.goal_id);
    assert_eq!(payload["branch"], task.branch);
    payload["head"].as_str().unwrap().to_string()
}

/// Assert the stream has nothing to say for a while.
async fn stays_quiet(body: &mut axum::body::Body, why: &str) {
    if let Some((kind, _)) = next_event(body, SILENCE).await {
        panic!("{why}: the stream sent a {kind}");
    }
}

/// The whole of what a client following one task hears: a commit says the
/// diff moved, and says it once — however many times git wrote in `.git` to
/// make it.
///
/// The `git pack-refs` in the middle is the case a watch on the ref *file*
/// would not survive: the loose ref is gone afterwards, folded into
/// `packed-refs`, and the next commit writes it back.
#[tokio::test]
async fn a_commit_on_the_task_branch_reaches_the_stream() {
    let h = harness().await;
    let (task, repo, worktree) = at_work(&h).await;
    let mut body = h
        .stream(get(&format!("/v1/events/stream?task={}", task.id)))
        .await;

    let first = commit(&worktree, "one");
    assert_eq!(next_head(&mut body, &task).await, first);

    let second = commit(&worktree, "two");
    assert_eq!(next_head(&mut body, &task).await, second);
    stays_quiet(&mut body, "one commit must be one event").await;

    // The ref is packed away and the sha has not moved: nothing to say.
    sh(&repo, "git pack-refs --all");
    stays_quiet(&mut body, "packing the refs is not a change to the branch").await;

    let third = commit(&worktree, "three");
    assert_eq!(next_head(&mut body, &task).await, third);
}

/// Commit until the watch answers.
///
/// A watch arms on the runtime rather than on the caller's thread, so a commit
/// made the instant after it was asked for can be the one its baseline picks
/// up rather than the one it reports. Committing again is what tells "not
/// armed yet" from "armed and quiet".
async fn commit_until_seen(worktree: &Path, rx: &mut Receiver<BusEvent>) {
    for n in 0..40 {
        let head = commit(worktree, &format!("armed{n}"));
        let seen = tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                if let DomainEvent::TaskBranchUpdated(dto) =
                    rx.recv().await.expect("event bus closed").event
                {
                    return dto.head;
                }
            }
        })
        .await;
        if let Ok(seen) = seen {
            assert_eq!(seen, head, "the watch reported a head nobody committed");
            return;
        }
    }
    panic!("the branch watch never reported a commit");
}

/// The watches are the process's own, so a daemon that starts over tasks
/// already in flight has to take them up again. Dropping every watch and
/// running the startup sweep is what a restart amounts to from in here.
#[tokio::test]
async fn the_startup_sweep_follows_the_worktrees_it_finds() {
    let h = harness().await;
    let (task, _repo, worktree) = at_work(&h).await;
    h.launcher.branches.unwatch(&task.id);
    assert!(!h.launcher.branches.is_watching(&task.id));

    h.launcher.watch_task_branches().await.unwrap();
    assert!(h.launcher.branches.is_watching(&task.id));

    let mut rx = h.bus.subscribe();
    commit_until_seen(&worktree, &mut rx).await;
}

/// A task that failed is not followed either, though the cleanup never runs on
/// one and its worktree stays where it is: nobody is committing on its branch
/// until a user retries it, and the spawn that revives it takes the watch back
/// up.
#[tokio::test]
async fn a_failed_task_stops_being_followed() {
    let h = harness().scheduler().await;
    let (task, _repo, worktree) = at_work(&h).await;
    let mut rx = h.bus.subscribe();
    // Armed for certain, so the silence at the end means something.
    commit_until_seen(&worktree, &mut rx).await;

    h.store
        .transition_task(
            &task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the agent could not be started"),
            None,
        )
        .await
        .unwrap();
    h.notify(&task.id);
    eventually(TIMEOUT, "the failed task's branch watch to be dropped", async || {
        !h.launcher.branches.is_watching(&task.id)
    })
    .await;
    // The worktree is still there: a retry puts the engineer back in it.
    assert!(worktree.is_dir());
    h.launcher.watch_task_branches().await.unwrap();
    assert!(
        !h.launcher.branches.is_watching(&task.id),
        "a failed task must not be picked up by the sweep"
    );

    commit(&worktree, "after-the-failure");
    tokio::time::sleep(SILENCE).await;
    while let Ok(event) = rx.try_recv() {
        assert!(
            !matches!(event.event, DomainEvent::TaskBranchUpdated(_)),
            "a failed task's branch was still reported"
        );
    }
}

/// A task that is over is not followed: the cleanup that takes its worktree
/// away takes the watch with it, and the startup sweep does not put it back.
#[tokio::test]
async fn the_watch_goes_with_the_worktree() {
    let h = harness().await;
    let (task, _repo, _worktree) = at_work(&h).await;
    assert!(h.launcher.branches.is_watching(&task.id));

    h.store
        .transition_task(
            &task.id,
            TaskStatus::Cancelled,
            Actor::User,
            Some("no longer wanted"),
            None,
        )
        .await
        .unwrap();
    h.launcher
        .cleanup_task(&task.id, true, false)
        .await
        .unwrap();

    assert!(!h.launcher.branches.is_watching(&task.id));
    h.launcher.watch_task_branches().await.unwrap();
    assert!(
        !h.launcher.branches.is_watching(&task.id),
        "a finished task must not be picked up by the sweep"
    );
}
