//! What becomes of the words a planner ends its turn on.
//!
//! A planner that answers in plain text — no `post_message`, no
//! `AskUserQuestion` — used to leave no trace anywhere the user looks: the
//! goal thread stayed empty, nothing went up on the attention strip, and the
//! question was only ever found by opening the pane. The turn-boundary event
//! carries the text, so the daemon writes it into the thread as the message
//! the planner would have posted itself, and it goes to the user from there
//! the way every other message addressed to them does.
//!
//! A guess, and the tests are as much about where it is *not* made: a goal
//! past planning, another role, an event carrying no words, words the planner
//! already posted in the turn that is ending. A real scheduler behind the
//! router throughout, because the flag is the delivery path's to raise and not
//! the ingestion's — asserting on it with nothing running would be asserting
//! on nothing.
//!
//! Every turn here is a real one: a `user_prompt_submit` opens it and the
//! `stop` closes it, because what "already posted" means is bounded to the
//! turn and a test that never started one would never find that boundary.

mod common;

use std::time::Duration;

use axum::http::StatusCode;

use ariadne_api::messages::MessageDto;
use ariadne_core::{AttentionReason, AuthorRole, Role, TaskStatus};
use ariadne_store::{AgentSession, Goal, Recipient};

use common::{Cast, Harness, as_session, eventually, harness, post_json};

/// How long a test waits for the scheduler to come round to a posted message.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The question a planner is left holding, in every test here.
const QUESTION: &str = "Which of the two do you want?";

/// A Claude Code `Stop` payload ending the turn on `said`, trimmed to the
/// fields this path reads. `said` is a JSON value rather than a string so a
/// test can send the null codex puts there for a turn nobody spoke on.
fn stop_saying(said: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "session_id": "5cf3f43d-6d22-42eb-8e44-8213bee346cd",
        "cwd": "/tmp/wt",
        "hook_event_name": "Stop",
        "stop_hook_active": false,
        "last_assistant_message": said,
    })
}

/// What opens a turn: the prompt the agent was handed, reported by the same
/// hook that reports its end.
fn turn_starts() -> serde_json::Value {
    serde_json::json!({
        "session_id": "5cf3f43d-6d22-42eb-8e44-8213bee346cd",
        "cwd": "/tmp/wt",
        "hook_event_name": "UserPromptSubmit",
        "prompt": "Get on with the plan.",
    })
}

/// A goal still in planning, its planner at a live pane and one turn into its
/// work, behind a real scheduler: the shape a planner's question is asked in.
async fn planning() -> (Harness, Cast, AgentSession) {
    let h = harness().scheduler().await;
    let cast = h.cast().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    h.pane_exists(&planner);
    h.ingest(&planner, "user_prompt_submit", turn_starts())
        .await;
    (h, cast, planner)
}

/// Post into the goal's thread, as `sender` or (None) as the user.
async fn post_to_goal(
    h: &Harness,
    goal: &Goal,
    body: &str,
    to: Option<&str>,
    sender: Option<&AgentSession>,
) -> MessageDto {
    let uri = format!("/v1/goals/{}/messages", goal.id);
    let payload = serde_json::json!({ "body": body, "to": to });
    match sender {
        Some(session) => {
            h.json(as_session(&uri, &session.id, payload), StatusCode::CREATED)
                .await
        }
        None => h.json(post_json(&uri, payload), StatusCode::CREATED).await,
    }
}

/// The everyday case, and the whole point of it: the question the planner
/// ended on is in the thread the user reads, as the planner's own message
/// addressed to them, and the strip says somebody is waiting.
#[tokio::test]
async fn a_planners_last_words_reach_the_goal_thread_and_the_user() {
    let (h, cast, planner) = planning().await;

    h.ingest(&planner, "stop", stop_saying(QUESTION.into()))
        .await;

    let thread = h.goal_thread(&cast.goal).await;
    let [message] = thread.as_slice() else {
        panic!("one message in the goal thread, got {thread:?}");
    };
    assert_eq!(message.body, QUESTION);
    assert_eq!(
        message.author_role,
        AuthorRole::Planner.as_str(),
        "the planner's own words, not a notice of the daemon's"
    );
    assert_eq!(
        message.author_session_id.as_deref(),
        Some(planner.id.as_str()),
        "traced back to the session the user answers in"
    );
    assert_eq!(
        message.recipient(),
        Some(Recipient::User),
        "and addressed to them, which is what puts it on the strip"
    );

    eventually(
        TIMEOUT,
        "the planner to be raised for the user",
        async || h.attention(&planner).await == Some(AttentionReason::WaitingUser),
    )
    .await;
    assert_eq!(
        h.keystrokes(&planner),
        0,
        "nothing is typed back at the planner for what it said itself"
    );
}

/// The other half of "waiting for you", through this path: the user answering
/// in the goal thread takes it down, the way it does for a message the
/// planner posted itself.
#[tokio::test]
async fn the_user_answering_in_the_thread_takes_the_flag_down() {
    let (h, cast, planner) = planning().await;

    h.ingest(&planner, "stop", stop_saying(QUESTION.into()))
        .await;
    eventually(
        TIMEOUT,
        "the planner to be raised for the user",
        async || h.attention(&planner).await == Some(AttentionReason::WaitingUser),
    )
    .await;

    post_to_goal(&h, &cast.goal, "The first one.", None, None).await;

    assert_eq!(
        h.attention(&planner).await,
        None,
        "the user has answered in the thread the question was asked in"
    );
}

/// A planner that has finalized its plan is nobody's to wake: the goal has
/// left planning, so whatever it says at the end of a turn is said to a
/// conversation that is over.
#[tokio::test]
async fn a_planner_of_a_goal_past_planning_says_nothing_to_anybody() {
    let (h, cast, planner) = planning().await;
    h.activate(&cast.goal).await;

    h.ingest(&planner, "stop", stop_saying(QUESTION.into()))
        .await;

    assert!(
        h.goal_thread(&cast.goal).await.is_empty(),
        "nothing was written into a thread nobody is waiting in"
    );
    assert_eq!(h.attention(&planner).await, None, "and nothing went up");
}

/// An engineer that ends a turn is waiting for the daemon's nudge, not for a
/// person — the same reason `idle_prompt` raises nothing. Flagging it for the
/// user would take it out of the watchdog that is about to nudge it.
#[tokio::test]
async fn an_engineer_ending_a_turn_is_waiting_on_nobody() {
    let h = harness().scheduler().await;
    let cast = h.active_cast().await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let engineer = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Role::Engineer,
            &cast.engineer.id,
        )
        .await;
    h.pane_exists(&engineer);

    h.ingest(&engineer, "stop", stop_saying(QUESTION.into()))
        .await;

    assert!(
        h.goal_thread(&cast.goal).await.is_empty(),
        "the goal's thread is the planner's, and nothing was written into it"
    );
    assert!(
        h.thread(&cast.task).await.is_empty(),
        "nor into the task's own"
    );
    assert_eq!(h.attention(&engineer).await, None, "and nothing went up");
}

/// A turn that ended on no words of anybody's. The field is absent on some
/// events, null on a codex turn that spoke through tool calls alone, and a
/// planner that ended on whitespace has said nothing worth showing.
#[tokio::test]
async fn a_turn_that_ended_on_nothing_is_written_nowhere() {
    let (h, cast, planner) = planning().await;

    h.ingest(
        &planner,
        "stop",
        serde_json::json!({"hook_event_name": "Stop"}),
    )
    .await;
    for said in [
        serde_json::Value::Null,
        "".into(),
        "   \n\t  ".into(),
        serde_json::json!(42),
    ] {
        h.ingest(&planner, "stop", stop_saying(said)).await;
    }

    assert!(
        h.goal_thread(&cast.goal).await.is_empty(),
        "a turn with nothing to relay writes nothing"
    );
    assert_eq!(h.attention(&planner).await, None, "and raises nothing");
}

/// A planner that posted its question itself and then ended the turn on it —
/// which is what Claude Code does with the text of a `post_message` call — is
/// not made to say it twice.
#[tokio::test]
async fn words_the_planner_already_posted_are_not_written_again() {
    let (h, cast, planner) = planning().await;

    post_to_goal(&h, &cast.goal, QUESTION, Some("user"), Some(&planner)).await;
    eventually(
        TIMEOUT,
        "the planner to be raised for the user",
        async || h.attention(&planner).await == Some(AttentionReason::WaitingUser),
    )
    .await;

    // The same words, trailing newline and all, at the end of the turn.
    h.ingest(
        &planner,
        "stop",
        stop_saying(format!("{QUESTION}\n").into()),
    )
    .await;

    let thread = h.goal_thread(&cast.goal).await;
    assert_eq!(
        thread.len(),
        1,
        "the planner said it once and the thread shows it once, got {thread:?}"
    );

    // What it goes on to say is another matter, and is written.
    h.ingest(&planner, "stop", stop_saying("Or a third option?".into()))
        .await;
    assert_eq!(
        h.goal_thread(&cast.goal)
            .await
            .iter()
            .map(|m| m.body.as_str())
            .collect::<Vec<_>>(),
        [QUESTION, "Or a third option?"],
    );
}

/// The same words a turn later are not the same words. A planner that asked
/// something, was answered, and ends a later turn on the very same sentence is
/// asking it afresh — suppressing that would leave the user waiting on a
/// question nothing in the thread shows.
#[tokio::test]
async fn words_the_planner_posted_in_an_earlier_turn_are_written_again() {
    let (h, cast, planner) = planning().await;

    // Turn one: asked with a `post_message`, and the turn ends on the same
    // words, which are not written a second time.
    post_to_goal(&h, &cast.goal, QUESTION, Some("user"), Some(&planner)).await;
    h.ingest(&planner, "stop", stop_saying(QUESTION.into()))
        .await;
    assert_eq!(
        h.goal_thread(&cast.goal).await.len(),
        1,
        "the planner said it once in this turn"
    );

    // The user answers, which starts the planner on a turn of its own.
    post_to_goal(&h, &cast.goal, "Neither — think again.", None, None).await;
    h.ingest(&planner, "user_prompt_submit", turn_starts())
        .await;

    // Turn two ends on the same sentence, and this time it is news.
    h.ingest(&planner, "stop", stop_saying(QUESTION.into()))
        .await;

    assert_eq!(
        h.goal_thread(&cast.goal)
            .await
            .iter()
            .map(|m| m.body.as_str())
            .collect::<Vec<_>>(),
        [QUESTION, "Neither — think again.", QUESTION],
        "the question asked again is in the thread again"
    );
    eventually(
        TIMEOUT,
        "the planner to be raised for the user again",
        async || h.attention(&planner).await == Some(AttentionReason::WaitingUser),
    )
    .await;
}

/// And where there is no turn to be bounded by — a session that has reported
/// no start at all — nothing is suppressed. A line said twice in the thread
/// costs the user a glance; a question swallowed costs them the goal.
#[tokio::test]
async fn a_session_with_no_turn_recorded_suppresses_nothing() {
    let h = harness().scheduler().await;
    let cast = h.cast().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner.id)
        .await;
    h.pane_exists(&planner);

    post_to_goal(&h, &cast.goal, QUESTION, Some("user"), Some(&planner)).await;
    h.ingest(&planner, "stop", stop_saying(QUESTION.into()))
        .await;

    assert_eq!(
        h.goal_thread(&cast.goal).await.len(),
        2,
        "with no turn to hold it to, the words are written rather than dropped"
    );
}
