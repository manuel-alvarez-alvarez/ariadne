//! What reconciliation may conclude when `tmux` cannot be run at all.
//!
//! Nothing, is the answer. A daemon that cannot spawn a process has learned
//! neither that a pane is gone nor that it is there, and both of the decisions
//! reconciliation makes from that — retire the session, or start a second
//! agent beside it — are worse than waiting for the next tick. The liveness
//! sweep already leaves such rows alone; this pins the other half, since a
//! preserved row plus a "no live sessions" reading is exactly how one task
//! ends up with two engineers.
//!
//! The same goes for the watchdog over an agent that has reported nothing:
//! what it does at the first threshold turns on what the pane is drawing, and
//! a pane that cannot be read says nothing about that either.

mod common;

use std::time::Duration;

use ariadne_core::{Actor, Role, SessionStatus, TaskStatus};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_store::AgentSession;

use common::{Cast, Harness, harness};

/// Everything one of these tests works on: an active goal with a task on it,
/// an engineer already sitting in a pane, and a daemon whose `tmux` binary is
/// not there — so every question about that pane comes back unanswered rather
/// than answered "no", which is what a machine briefly out of process slots
/// looks like from here.
async fn world() -> (Harness, Cast, AgentSession) {
    let h = harness().tmux(common::Tmux::Missing).await;
    let cast = h.active_cast().await;
    let session = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Role::Engineer,
            &cast.engineer.id,
        )
        .await;
    (h, cast, session)
}

/// Put a working `tmux` where the unrunnable one was, with every pane it is
/// asked about there.
fn tmux_comes_back(h: &Harness) {
    h.tmux_returns();
    h.every_pane_exists();
}

#[tokio::test]
async fn reconciliation_with_tmux_unavailable_neither_spawns_nor_fails_the_task() {
    let (h, cast, session) = world().await;
    h.store
        .transition_task(&cast.task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();

    // More reconciliations than the spawn-retry budget allows for, so a task
    // failed by repeated attempts would have failed by the end of them.
    // No sleep inhibition: a test has no business touching power management.
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    for _ in 0..6 {
        sched
            .send(SchedEvent::TaskChanged(cast.task.id.clone()))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;
    }

    let sessions = h.sessions_of(&cast.task.id).await;
    assert_eq!(
        sessions.len(),
        1,
        "the session that may still be running keeps the task to itself: {sessions:#?}"
    );
    assert_eq!(sessions[0].id, session.id);
    assert!(
        sessions[0].status().is_live(),
        "a session is not retired because tmux could not be run: {:?}",
        sessions[0].status()
    );

    assert_ne!(
        h.status(&cast.task.id).await,
        TaskStatus::Failed,
        "an unreachable tmux is not the task's fault, and must not spend its retry budget"
    );
}

/// A pane nobody can read is not a pane with nothing in it. The watchdog's
/// first threshold has passed, and what it does about a silent agent depends
/// on what its composer is holding — which is exactly the question an
/// unrunnable tmux answers neither way. So nothing is typed and nothing is
/// spent: the moment tmux answers again, the composer decides, and here it is
/// still holding the instruction that was never submitted.
#[tokio::test]
async fn a_silent_agent_whose_pane_cannot_be_read_is_left_for_the_next_pass() {
    let (h, cast, session) = world().await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    // Running and silent for longer than the nudge threshold, which the store
    // only ever stamps "now": the columns the one clock is read from are moved
    // back by hand.
    h.set_status(&session, SessionStatus::Running).await;
    h.launched_ago(&session, 360).await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(
        h.keystrokes(&session),
        0,
        "nothing was typed at a pane nobody could read"
    );
    assert_eq!(
        h.attention(&session).await,
        None,
        "and the user is not told about a silence that was never confirmed"
    );

    // tmux comes back, and the pane it could not answer for is still holding
    // the instruction the launch put there — as the engineer's resume template
    // words it.
    h.composer_keeps(r#"Pick "task" up again on"#);
    tmux_comes_back(&h);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        sched
            .send(SchedEvent::TaskChanged(cast.task.id.clone()))
            .unwrap();
        if h.enters(&session) > 0 {
            assert_eq!(
                h.keystrokes(&session),
                1,
                "the nudge the outage never spent is one Enter, not a paste"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for the composer to be submitted once tmux answered"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
