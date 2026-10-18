//! Where the seat text and the skill index reach an agent (006).
//!
//! The MCP server's instructions carry both for every seat. No adapter is
//! proven to keep those instructions in its real system prompt through a
//! compaction, so every agent gets the seat text and the skill index ahead
//! of the first turn of a launch, and again ahead of the first turn after a
//! model switch. Every other turn is its own text alone.
//!
//! The agents are the harness's stub, registered as its own `stub`, and
//! under `claude-acp` too: Claude takes the same path as every other agent.

use crate::common;

use axum::http::StatusCode;
use serde_json::json;

use ariadne_core::{SessionStatus, TaskStatus};
use ariadne_store::AgentSession;

use common::{Cast, Harness, TIMEOUT, eventually, harness, post_json};

/// Start the develop column's agent of a task pinned to `pin`, and hand it
/// the column's first briefing, the way the scheduler does. Answers the
/// session and the briefing.
async fn briefed_develop(h: &Harness, pin: &str) -> (AgentSession, String) {
    h.git_repo("repo");
    let cast = h.cast_pinned(pin).await;
    let goal = h.activate(&cast.goal).await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let cast = Cast { goal, task, ..cast };
    let session = h
        .launcher
        .start_step_agent(&cast.task, cast.develop())
        .await
        .unwrap();
    let steps = h.store.goal_steps(&cast.goal.id).await.unwrap();
    let step = steps.iter().find(|s| s.id == "develop").unwrap();
    let briefing = h
        .launcher
        .step_first_briefing(&cast.task, step, "")
        .await
        .unwrap();
    h.launcher
        .acp
        .send_prompt(&session.id, briefing.clone())
        .unwrap();
    (session, briefing)
}

/// Hand `session` the prompt `text`, and answer what its agent was sent,
/// once that turn is over.
async fn turn(h: &Harness, session: &AgentSession, text: &str) -> String {
    let before = h.prompts_to(session).len();
    h.launcher
        .acp
        .send_prompt(&session.id, text.to_string())
        .unwrap();
    eventually(TIMEOUT, "the turn to reach the agent and end", || async {
        h.prompts_to(session).len() > before
            && h.session_status(session).await == SessionStatus::Idle
    })
    .await;
    h.prompts_to(session).remove(before)
}

/// Wait until the first turn of `session` is over, and answer what it sent.
async fn first_turn(h: &Harness, session: &AgentSession) -> String {
    eventually(TIMEOUT, "the first turn to end", || async {
        !h.prompts_to(session).is_empty() && h.session_status(session).await == SessionStatus::Idle
    })
    .await;
    h.prompts_to(session).remove(0)
}

/// Claude takes the same path as every other agent: its first turn carries
/// the seat brief, and the nudge after it reaches the agent word for word.
#[tokio::test]
async fn a_claude_agent_gets_the_seat_brief_first_and_each_later_turn_word_for_word() {
    let h = harness().second_agent_as("claude-acp").await;
    let (session, briefing) = briefed_develop(&h, "claude-acp:a").await;
    let brief = h.launch_file(&session.id).unwrap().system_prompt;
    assert!(brief.contains("Your skills."), "{brief}");

    assert_eq!(
        first_turn(&h, &session).await,
        format!("{brief}\n\n{briefing}")
    );
    let nudge = "Continue the task at Develop.";
    assert_eq!(turn(&h, &session, nudge).await, nudge);
}

/// The first turn of a launch, and the first turn after a model switch,
/// carry the seat text and the skill index ahead of their own text. Every
/// other turn is its own text alone.
#[tokio::test]
async fn an_agent_gets_the_seat_brief_on_the_first_turn_and_after_a_switch() {
    let h = harness().await;
    let (session, briefing) = briefed_develop(&h, "stub:a").await;
    let brief = h.launch_file(&session.id).unwrap().system_prompt;
    assert!(brief.contains("Your skills."), "{brief}");

    assert_eq!(
        first_turn(&h, &session).await,
        format!("{brief}\n\n{briefing}")
    );
    assert_eq!(turn(&h, &session, "Go on.").await, "Go on.");

    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/switch", session.id),
            json!({ "model": "stub:b" }),
        ))
        .await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        turn(&h, &session, "After the switch.").await,
        format!("{brief}\n\nAfter the switch.")
    );
    assert_eq!(turn(&h, &session, "Later.").await, "Later.");
}
