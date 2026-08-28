//! What happens to a message that names somebody.
//!
//! Posting one writes it to the thread; this is the other half — the addressee
//! being told. An agent with a pane is nudged with the message, one whose
//! session ended is resumed with it as its instruction, and an addressee with
//! no session at all keeps its message in the thread, where its next briefing
//! sends it to read. A message for the human wakes nobody: it goes up the
//! attention path instead, on the session of the agent that wrote it.
//!
//! The other half is what happens when the delivery does not work: a tmux
//! that will not take the keystrokes, an agent that cannot be resumed and an
//! addressee with no session to type into are tried again, and once the
//! passes are gone somebody is told — the addressee on its own session, or
//! the author on theirs when the addressee has no session at all. Nothing is
//! ever quietly struck off.
//!
//! The whole path is exercised, from the HTTP handler both agents and the CLI
//! post through to the keystrokes that come out the other end: the router is
//! wired to a real scheduler, and `tmux` is the stub whose sessions are the
//! ones a test lists as alive and which writes down every `send-keys` it is
//! handed — including the hexadecimal paste bodies, which is how "this agent
//! was told what the message said" is asserted.

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};

use ariadne_api::SESSION_HEADER;
use ariadne_api::messages::MessageDto;
use ariadne_core::{Actor, AttentionReason, Role, TaskStatus};
use ariadne_daemon::notify;
use ariadne_store::{AgentSession, Goal, SessionFilter, Task};

use common::{Cast, Harness, eventually, harness};

/// How long a test waits for a delivery to come out of the scheduler. A
/// confirmed paste sleeps a second inside `send_submitted` before the pane is
/// read back, so this is not as generous as it looks.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The same, for the one test that waits on a reconciliation tick rather than
/// on an event: nothing re-posts a message in production, so a retry is the
/// tick's to make and this is how long a tick can take to come round.
const TICK_TIMEOUT: Duration = Duration::from_secs(40);

/// How many passes one message is worth before the user is told it never
/// arrived, mirroring the scheduler's `DELIVERY_ATTEMPTS`.
const DELIVERY_ATTEMPTS: usize = 3;


/// An active goal with one task on it, in progress, behind a real scheduler:
/// the shape every test here starts from.
async fn seeded() -> (Harness, Cast) {
    let h = harness().scheduler().await;
    let cast = h.active_cast().await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    (h, Cast { task, ..cast })
}

/// Post into a task's conversation, as `sender` or (None) as the user.
async fn post_to_task(
    h: &Harness,
    task: &Task,
    body: &str,
    to: Option<&str>,
    sender: Option<&AgentSession>,
) -> MessageDto {
    post(h, &format!("/v1/tasks/{}/messages", task.id), body, to, sender).await
}

/// Post into a goal's planning thread.
async fn post_to_goal(
    h: &Harness,
    goal: &Goal,
    body: &str,
    to: Option<&str>,
    sender: Option<&AgentSession>,
) -> MessageDto {
    post(h, &format!("/v1/goals/{}/messages", goal.id), body, to, sender).await
}

async fn post(
    h: &Harness,
    path: &str,
    body: &str,
    to: Option<&str>,
    sender: Option<&AgentSession>,
) -> MessageDto {
    let payload = serde_json::json!({ "body": body, "to": to });
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(session) = sender {
        request = request.header(SESSION_HEADER, &session.id);
    }
    h.json(
        request.body(Body::from(payload.to_string())).unwrap(),
        StatusCode::CREATED,
    )
    .await
}

/// The argv of the last launch of `session_id`, where a resumed agent's
/// instruction rides, or `None` while the launch has yet to write its plan:
/// the session is marked live the moment the resume starts, a while before the
/// spawn plan reaches the disk, so this is something to wait for rather than
/// to read once.
fn resume_argv(h: &Harness, session_id: &str) -> Option<String> {
    h.spawn_plan(session_id).map(|plan| plan.argv.join(" "))
}

/// The everyday case: an agent that is running is told what was said to it,
/// with the sender named and the message quoted, so it can act without going
/// to look the message up first.
#[tokio::test]
async fn an_addressed_agent_with_a_live_pane_is_nudged_with_the_message() {
    let (h, cast) = seeded().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&engineer);

    post_to_task(
        &h,
        &cast.task,
        "Skip the migration: the store already has the column.",
        Some("engineer"),
        Some(&planner),
    )
    .await;

    eventually(TIMEOUT, "the engineer to be nudged", async || {
        h.pasted(&engineer)
            .contains("Skip the migration: the store already has the column.")
    })
    .await;
    let pasted = h.pasted(&engineer);
    assert!(
        pasted.contains("New message from the planner in your task conversation"),
        "the sender and the thread are named: {pasted}"
    );
    assert!(
        pasted.contains("`list_messages`"),
        "and the rest of the conversation is one call away: {pasted}"
    );
}

/// An agent whose session ended is not woken by typing into a pane that is no
/// longer there: it is resumed, with the message as the instruction it comes
/// back to.
#[tokio::test]
async fn an_addressed_agent_whose_session_ended_is_resumed_with_the_message() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    let engineer = h.ended(&engineer).await;

    post_to_task(
        &h,
        &cast.task,
        "Rebase before you merge.",
        Some("engineer"),
        None,
    )
    .await;

    eventually(TIMEOUT, "the engineer to be resumed", async || {
        resume_argv(&h, &engineer.id).is_some()
    })
    .await;
    let argv = resume_argv(&h, &engineer.id).unwrap();
    assert!(
        argv.contains("--resume uuid-1234"),
        "the same conversation, not a fresh one: {argv}"
    );
    assert!(
        argv.contains("Rebase before you merge."),
        "and it comes back to the message: {argv}"
    );
    assert!(
        argv.contains("New message from the user in your task conversation"),
        "with its sender named: {argv}"
    );
}

/// A goal's planning thread addresses its planner, whose session is the goal's
/// own — the tasks under it have sessions too, and none of them is the one
/// meant here.
#[tokio::test]
async fn a_goal_thread_message_wakes_the_planner() {
    let (h, cast) = seeded().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&planner);
    h.pane_exists(&engineer);

    post_to_goal(
        &h,
        &cast.goal,
        "Split the last task in two.",
        Some("planner"),
        None,
    )
    .await;

    eventually(TIMEOUT, "the planner to be nudged", async || {
        h.pasted(&planner).contains("Split the last task in two.")
    })
    .await;
    let pasted = h.pasted(&planner);
    assert!(
        pasted.contains("the goal's planning thread"),
        "the thread it was said in is named: {pasted}"
    );
    assert_eq!(
        h.keystrokes(&engineer),
        0,
        "and the goal's tasks are not woken by it"
    );
}

/// An addressee with no session at all — a reviewer between rounds — is no
/// error and no message lost: it stays in the thread, which is where every
/// agent's briefing sends it to read, and nobody is started for it. What
/// happens if no session ever turns up for it is the next test's.
#[tokio::test]
async fn an_addressee_with_no_session_leaves_the_message_in_the_thread() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&engineer);

    post_to_task(
        &h,
        &cast.task,
        "Have a look at the error handling.",
        Some("reviewer"),
        None,
    )
    .await;
    // Posted after it and delivered: the queue is in order, so the reviewer's
    // message has been through the scheduler by the time this one lands.
    post_to_task(&h, &cast.task, "Carry on.", Some("engineer"), None)
        .await;

    eventually(TIMEOUT, "the engineer to be nudged", async || {
        h.pasted(&engineer).contains("Carry on.")
    })
    .await;
    assert!(
        h.store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .iter()
            .all(|s| s.profile_id != cast.reviewer.id),
        "no reviewer was started for a message"
    );
    assert!(
        h.thread(&cast.task)
            .await
            .contains(&"Have a look at the error handling.".to_string()),
        "and the message is where it was left"
    );
    assert!(
        !h.pasted(&engineer)
            .contains("Have a look at the error handling."),
        "certainly not typed at somebody else"
    );
}

/// A message for the human is not an agent's to answer: nobody is woken, and
/// it goes up on the session of the agent that asked — which is the pane the
/// user replies in.
#[tokio::test]
async fn a_message_for_the_user_raises_its_author_and_wakes_no_agent() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    h.pane_exists(&engineer);
    h.pane_exists(&planner);

    post_to_task(
        &h,
        &cast.task,
        "Which database should this write to?",
        Some("user"),
        Some(&engineer),
    )
    .await;

    eventually(TIMEOUT, "the author to be raised for the user", async || {
        h.attention(&engineer).await == Some(AttentionReason::WaitingUser)
    })
    .await;
    assert_eq!(
        h.keystrokes(&engineer),
        0,
        "the author is not nudged with its own question"
    );
    assert_eq!(h.keystrokes(&planner), 0, "and no other agent is either");
}

/// Walk `task` to an ending and hand the notice the daemon writes for it to
/// the scheduler — which is `Scheduler::announce_ending`, and the HTTP
/// transition handler, spelled out.
async fn ends_as(h: &Harness, task: &Task, to: TaskStatus, actor: Actor, reason: Option<&str>) {
    if to == TaskStatus::Merged {
        for (status, by) in [
            (TaskStatus::UnderReview, Actor::Engineer),
            (TaskStatus::Approved, Actor::Daemon),
        ] {
            h.store
                .transition_task(&task.id, status, by, None, None)
                .await
                .unwrap();
        }
    }
    let commit = (to == TaskStatus::Merged).then_some("abc1234");
    let ended = h
        .store
        .transition_task(&task.id, to, actor, reason, commit)
        .await
        .unwrap();
    let notice = notify::task_ended(&h.store, &ended, reason)
        .await
        .unwrap()
        .expect("an ending is written to the thread");
    h.notify_message(&notice.id);
}

/// The daemon's own ending notice, put through the scheduler with an engineer
/// still live behind it: the notice keeps the user as its recipient, and
/// nothing goes up on the session, because nobody is waiting on the agent of
/// a task that is over.
///
/// The flag is asserted never to have appeared rather than to be gone by now:
/// the stale sweep took it down up to fifteen seconds later, which is a
/// "Waiting for you" that flashes on every ending. A message posted behind the
/// notice is what says the scheduler has been through it — the queue is one,
/// and in order.
async fn an_ending_raises_nothing(to: TaskStatus, actor: Actor, reason: Option<&str>) {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    h.pane_exists(&engineer);

    ends_as(&h, &cast.task, to, actor, reason).await;
    post_to_task(&h, &cast.task, "And that is that.", Some("engineer"), Some(&planner)).await;

    eventually(TIMEOUT, "the message queued behind the notice", async || {
        h.pasted(&engineer).contains("And that is that.")
    })
    .await;
    assert_eq!(
        h.attention(&engineer).await,
        None,
        "the ending put nothing on the engineer's session"
    );
    assert_eq!(
        h.user_messages(&cast.task).await.len(),
        1,
        "and the notice is still the user's, so the thread still shows it as theirs"
    );
}

#[tokio::test]
async fn a_merged_task_tells_the_user_without_flagging_its_engineer() {
    an_ending_raises_nothing(TaskStatus::Merged, Actor::Engineer, None).await;
}

#[tokio::test]
async fn a_failed_task_tells_the_user_without_flagging_its_engineer() {
    an_ending_raises_nothing(TaskStatus::Failed, Actor::Daemon, Some("the build never passed")).await;
}

#[tokio::test]
async fn a_cancelled_task_tells_the_user_without_flagging_its_engineer() {
    an_ending_raises_nothing(TaskStatus::Cancelled, Actor::User, Some("we do not need it")).await;
}

/// The other half of "waiting for you": the user answering in the thread takes
/// it down, whether or not they addressed anybody, and only in the thread they
/// wrote in.
#[tokio::test]
async fn a_message_from_the_user_takes_its_thread_off_waiting_for_them() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    h.pane_exists(&engineer);
    h.pane_exists(&planner);

    // A second task of the same goal, its own engineer waiting on the user too.
    let repo = h.store.list_goal_repositories(&cast.goal.id).await.unwrap()[0].clone();
    let other = h
        .task_on(&cast.goal, &repo, "Something else", &cast.engineer, &[&cast.reviewer])
        .await;
    let elsewhere = h
        .session(&cast.goal, Some(&other), Role::Engineer, &cast.engineer.id)
        .await;
    h.raise(&engineer, AttentionReason::WaitingUser).await;
    h.raise(&elsewhere, AttentionReason::WaitingUser).await;

    // An agent talking in the thread is not the user having answered.
    post_to_task(&h, &cast.task, "Still on it.", None, Some(&planner)).await;
    assert_eq!(
        h.attention(&engineer).await,
        Some(AttentionReason::WaitingUser),
        "another agent's message answers nothing"
    );

    post_to_task(&h, &cast.task, "The staging one, please.", None, None).await;
    assert_eq!(
        h.attention(&engineer).await,
        None,
        "the user has spoken in the thread the flag was raised in"
    );
    assert_eq!(
        h.attention(&elsewhere).await,
        Some(AttentionReason::WaitingUser),
        "and said nothing in any other"
    );
}

/// The goal's own thread reaches the planner working in it, and reaches into
/// none of its tasks.
#[tokio::test]
async fn a_message_from_the_user_in_the_goal_thread_takes_the_planner_off_waiting() {
    let (h, cast) = seeded().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&planner);
    h.raise(&planner, AttentionReason::WaitingUser).await;
    h.raise(&engineer, AttentionReason::WaitingUser).await;

    post_to_goal(&h, &cast.goal, "Yes, split it in two.", None, None).await;

    assert_eq!(h.attention(&planner).await, None);
    assert_eq!(
        h.attention(&engineer).await,
        Some(AttentionReason::WaitingUser),
        "a task's thread is not the goal's own"
    );
}

/// An agent sitting on a permission dialog is left alone: the Enter behind a
/// paste would answer the dialog, which is the one decision the daemon must
/// not make on the user's behalf.
#[tokio::test]
async fn an_agent_waiting_on_a_dialog_is_not_typed_into() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    let reviewer = h
        .session(&cast.goal, Some(&cast.task), Role::Reviewer, &cast.reviewer.id)
        .await;
    h.pane_exists(&engineer);
    h.pane_exists(&reviewer);
    h.raise(&engineer, AttentionReason::WaitingPermission).await;

    post_to_task(
        &h,
        &cast.task,
        "Use the other endpoint.",
        Some("engineer"),
        None,
    )
    .await;
    post_to_task(&h, &cast.task, "Start on round two.", Some("reviewer"), None)
        .await;

    eventually(TIMEOUT, "the reviewer to be nudged", async || {
        h.pasted(&reviewer).contains("Start on round two.")
    })
    .await;
    assert_eq!(
        h.keystrokes(&engineer),
        0,
        "nothing was typed at the agent holding a dialog"
    );
    assert_eq!(
        h.attention(&engineer).await,
        Some(AttentionReason::WaitingPermission),
        "and what it is waiting for is still what it says"
    );
}

/// The scheduler resweeps everything every tick, and a message it sees twice
/// must not be typed in twice — the agent would read the same thing said
/// again as something new.
#[tokio::test]
async fn a_message_is_delivered_once_however_often_the_scheduler_sees_it() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&engineer);

    let msg = post_to_task(&h, &cast.task, "Only once, please.", Some("engineer"), None)
        .await;
    // The resweep: the same message, offered to the scheduler again.
    for _ in 0..3 {
        h.notify_message(&msg.id);
    }
    // Behind all of them in the same queue, so its arrival means they are done.
    post_to_task(&h, &cast.task, "And that is all.", Some("engineer"), None)
        .await;

    eventually(TIMEOUT, "the second message to arrive", async || {
        h.pasted(&engineer).contains("And that is all.")
    })
    .await;
    assert_eq!(
        h.pasted(&engineer).matches("Only once, please.").count(),
        1,
        "the first message was typed in exactly once"
    );
}

/// A message addressed to the thread rather than to anyone in it behaves as
/// every message did before recipients existed: it is written down, and
/// nobody is woken for it.
#[tokio::test]
async fn an_unaddressed_message_wakes_nobody() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&engineer);

    post_to_task(&h, &cast.task, "Noting this for the record.", None, None)
        .await;
    post_to_task(&h, &cast.task, "Now this is for you.", Some("engineer"), None)
        .await;

    eventually(TIMEOUT, "the addressed message to arrive", async || {
        h.pasted(&engineer).contains("Now this is for you.")
    })
    .await;
    assert!(
        !h.pasted(&engineer).contains("Noting this for the record."),
        "the unaddressed one was nobody's to be woken for"
    );
    assert_eq!(
        h.attention(&engineer).await,
        None,
        "and it raised nothing for the user"
    );
}

/// The planner takes part in every task thread, and its session is the goal's
/// own — the task it is being written to has no session of the planner's in
/// it, and looking for one there is how a message addressed to the planner
/// used to wake nobody at all.
#[tokio::test]
async fn a_task_thread_message_addressed_to_the_planner_wakes_it() {
    let (h, cast) = seeded().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&planner);
    h.pane_exists(&engineer);

    post_to_task(
        &h,
        &cast.task,
        "This task needs a second repository.",
        Some("planner"),
        Some(&engineer),
    )
    .await;

    eventually(TIMEOUT, "the planner to be nudged", async || {
        h.pasted(&planner)
            .contains("This task needs a second repository.")
    })
    .await;
    let pasted = h.pasted(&planner);
    assert!(
        pasted.contains("New message from the engineer in your task conversation"),
        "the sender and the thread are named: {pasted}"
    );
    assert_eq!(
        h.keystrokes(&engineer),
        0,
        "and the agent that wrote it is not woken with its own message"
    );
}

/// A tmux that would not take the keystrokes has said nothing about whether
/// the agent is there to hear them: the message is not struck off for it. The
/// reconciliation tick tries again — nothing re-posts a message in production,
/// so this is the only thing that would — and the agent gets it once, whole.
#[tokio::test]
async fn a_delivery_tmux_refused_is_tried_again_on_a_later_tick() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&engineer);
    h.keystrokes_refused(true);

    post_to_task(
        &h,
        &cast.task,
        "The store already has that column.",
        Some("engineer"),
        None,
    )
    .await;
    eventually(TIMEOUT, "the delivery to be turned away", async || !h.refused_panes().is_empty()).await;
    assert_eq!(
        h.pasted(&engineer),
        "",
        "nothing reached the pane on the pass that failed"
    );

    h.keystrokes_refused(false);

    eventually(TICK_TIMEOUT, "the tick to try again", async || {
        h.pasted(&engineer)
            .contains("The store already has that column.")
    })
    .await;
    assert_eq!(
        h.pasted(&engineer)
            .matches("The store already has that column.")
            .count(),
        1,
        "and it arrives once, not once per attempt"
    );
    assert_eq!(
        h.attention(&engineer).await,
        None,
        "a delivery that got there raises nothing"
    );
}

/// The passes are not endless. An agent whose pane cannot be reached at all
/// ends as a flag on its own session: whatever it was told, it never heard it,
/// and only a person can do anything about that.
#[tokio::test]
async fn a_delivery_that_never_gets_through_raises_the_addressee() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&engineer);
    h.keystrokes_refused(true);

    let message = post_to_task(
        &h,
        &cast.task,
            "Rebase before you merge.",
            Some("engineer"),
            None,
        )
        .await;

    // The passes the tick would make, asked for without waiting a quarter of
    // a minute for each: a message already in flight or already given up on
    // is nobody's to deliver again, so the extra offers cost nothing.
    eventually(TIMEOUT, "the engineer to be raised", async || {
        h.notify_message(&message.id);
        h.attention(&engineer).await == Some(AttentionReason::Stalled)
    })
    .await;
    assert!(
        h.refused_panes().len() >= DELIVERY_ATTEMPTS,
        "it was tried every pass it was worth, not given up on the first: {}",
        h.refused_panes().len()
    );
    assert!(
        h.thread(&cast.task)
            .await
            .contains(&"Rebase before you merge.".to_string()),
        "and the message is still in the thread for whoever comes to look"
    );
}

/// An addressee whose session ended is resumed with the message — and when
/// that resume cannot happen (here: the worktree it would come back in is
/// gone), the message has reached nobody. The session says so, with the
/// reason its pane has: there is none.
#[tokio::test]
async fn an_addressee_that_cannot_be_resumed_is_raised_for_the_user() {
    let (h, cast) = seeded().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    let engineer = h.ended(&engineer).await;
    std::fs::remove_dir_all(engineer.worktree_path.as_ref().unwrap()).unwrap();

    let message = post_to_task(
        &h,
        &cast.task,
            "Have another look at the error handling.",
            Some("engineer"),
            None,
        )
        .await;

    eventually(TIMEOUT, "the engineer to be raised", async || {
        h.notify_message(&message.id);
        h.attention(&engineer).await == Some(AttentionReason::Disconnected)
    })
    .await;
    assert!(
        resume_argv(&h, &engineer.id).is_none(),
        "nothing was launched: there was nowhere to launch it"
    );
}

/// The last resort. An addressee that had a session and no longer has one
/// leaves nothing to flag — so the flag goes where the answer was going to be
/// read: on the session of whoever asked, as the user's to deal with.
#[tokio::test]
async fn a_message_whose_addressee_lost_its_session_raises_its_author() {
    let (h, cast) = seeded().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&engineer);
    h.pane_exists(&planner);
    h.keystrokes_refused(true);

    let message = post_to_task(
        &h,
        &cast.task,
            "Which database should this write to?",
            Some("engineer"),
            Some(&planner),
        )
        .await;
    eventually(TIMEOUT, "the delivery to be turned away", async || !h.refused_panes().is_empty()).await;
    // And now the addressee is gone, the way a deleted goal takes its
    // sessions with it, with the message still owed.
    h.forget_session(&engineer).await;

    eventually(TIMEOUT, "the author to be raised for the user", async || {
        h.notify_message(&message.id);
        h.attention(&planner).await == Some(AttentionReason::WaitingInput)
    })
    .await;
}

/// An addressee that never gets a session is the same story as one that lost
/// it. The message keeps its place in the thread, and the passes are not
/// endless: when they run out with nobody there to be woken, the agent that
/// asked is raised for the user, since it is the one waiting on an answer
/// that is not coming.
#[tokio::test]
async fn a_message_for_an_addressee_that_never_gets_a_session_raises_its_author() {
    let (h, cast) = seeded().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    h.pane_exists(&planner);

    // The reviewer has no session at all: this round has not started one.
    let message = post_to_task(
        &h,
        &cast.task,
            "Have a look at the error handling once you pick this up.",
            Some("reviewer"),
            Some(&planner),
        )
        .await;

    // The passes the tick would make, asked for without waiting a quarter of
    // a minute for each.
    eventually(TIMEOUT, "the author to be raised for the user", async || {
        h.notify_message(&message.id);
        h.attention(&planner).await == Some(AttentionReason::WaitingInput)
    })
    .await;
    assert!(
        h.store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .iter()
            .all(|s| s.profile_id != cast.reviewer.id),
        "no reviewer was started for a message"
    );
    assert_eq!(
        h.keystrokes(&planner),
        0,
        "and nothing was typed at the agent that wrote it"
    );
    assert!(
        h.thread(&cast.task)
            .await
            .contains(&"Have a look at the error handling once you pick this up.".to_string()),
        "the message is still there for whoever comes to read it"
    );
}

/// The goal-level fallback is the planner's alone. Profiles are reusable, so
/// another role can have a session with no task on it; a task thread's
/// message is not that conversation's, and is not typed into it.
#[tokio::test]
async fn a_task_message_is_not_typed_at_another_role_working_outside_the_task() {
    let (h, cast) = seeded().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    // An engineer session of the goal's rather than of the task's — not where
    // this task's engineer works, whatever profile it was started with.
    let elsewhere = h
        .session(&cast.goal, None, Role::Engineer, &cast.engineer.id)
        .await;
    h.pane_exists(&planner);
    h.pane_exists(&elsewhere);

    let message = post_to_task(
        &h,
        &cast.task,
            "Skip the migration.",
            Some("engineer"),
            Some(&planner),
        )
        .await;

    eventually(TIMEOUT, "the author to be raised for the user", async || {
        h.notify_message(&message.id);
        h.attention(&planner).await == Some(AttentionReason::WaitingInput)
    })
    .await;
    assert_eq!(
        h.keystrokes(&elsewhere),
        0,
        "the session outside the task was left to its own conversation"
    );
}
