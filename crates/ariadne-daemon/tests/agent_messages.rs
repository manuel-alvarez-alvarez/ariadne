//! What one agent says to another, and how it gets there.
//!
//! The channel is one table and one transport: a message is written by the
//! agent that sent it and typed into the recipient's pane by the daemon, so it
//! arrives as a turn rather than as something anybody has to go and look for.
//!
//! What is worth pinning is the addressing — every message has exactly one
//! recipient, and an answer goes back to whoever asked without naming them —
//! and the delivery, which is the whole reason the channel is worth having.

mod common;

use axum::http::StatusCode;

use ariadne_api::error::ErrorBody;
use ariadne_api::messages::MessageDto;
use ariadne_core::{Actor, MessageKind, Seat, SessionStatus, TaskStatus};
use ariadne_daemon::scheduler::{self, SchedEvent};

use common::{Cast, TIMEOUT, as_session, eventually, get, harness};

fn messages_uri(cast: &Cast) -> String {
    format!("/v1/tasks/{}/messages", cast.task.id)
}

/// One message body, as an agent sends it.
fn ask(to_actor: &str, to_agent_id: Option<&str>, body: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "question",
        "to_actor": to_actor,
        "to_agent_id": to_agent_id,
        "body": body,
    })
}

/// A reviewer asks the author something mid-round, and the author answers it.
/// Neither of them left the task to do it, and the round is where it was.
#[tokio::test]
async fn a_reviewer_asks_the_author_and_the_author_answers_it() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let reviewer = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Reviewer,
            &cast.reviewer.id,
        )
        .await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.advance(&cast.task, TaskStatus::UnderReview).await;

    let asked: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                ask(
                    "author",
                    Some(&cast.author.id),
                    "Why is the retry unbounded?",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(asked.kind, MessageKind::Question);
    assert_eq!(asked.from_actor, Actor::Reviewer);
    assert_eq!(
        asked.from_agent_id.as_deref(),
        Some(cast.reviewer.id.as_str())
    );
    assert_eq!(asked.to_agent_id.as_deref(), Some(cast.author.id.as_str()));
    assert_eq!(asked.delivered_at, None, "nothing has typed it yet");

    // The answer names the message and nothing else: where it goes is who
    // asked.
    let answered: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &author.id,
                serde_json::json!({
                    "kind": "answer",
                    "to_actor": "orchestrator",
                    "in_reply_to": asked.id,
                    "body": "Because the caller retries too.",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(answered.to_actor, Actor::Reviewer, "back to whoever asked");
    assert_eq!(
        answered.to_agent_id.as_deref(),
        Some(cast.reviewer.id.as_str())
    );
    assert_eq!(answered.in_reply_to.as_deref(), Some(asked.id.as_str()));

    // And the round is untouched: a question is not a vote.
    assert_eq!(
        h.store
            .round_verdicts(&cast.task.id, cast.task.review_round + 1)
            .await
            .unwrap()
            .len(),
        0
    );
}

/// The transport: the daemon types the message into the recipient's pane and
/// stamps it delivered, so the agent reads it as a turn.
#[tokio::test]
async fn a_message_is_typed_into_the_pane_it_was_sent_to() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.pane_exists(&author);
    h.set_status(&author, SessionStatus::Idle).await;
    let reviewer = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Reviewer,
            &cast.reviewer.id,
        )
        .await;
    h.advance(&cast.task, TaskStatus::UnderReview).await;

    let asked: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                ask(
                    "author",
                    Some(&cast.author.id),
                    "Why is the retry unbounded?",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();

    eventually(TIMEOUT, "the message to reach the pane", async || {
        h.pasted(&author).contains("Why is the retry unbounded?")
    })
    .await;
    let pasted = h.pasted(&author);
    assert!(
        pasted.contains(&asked.id),
        "the id an answer names is not in what was typed: {pasted}"
    );
    // Named by the skills it works with: an agent has no name of its own.
    assert!(pasted.contains("reviewer (code-review)"), "{pasted}");

    eventually(TIMEOUT, "the message to be stamped delivered", async || {
        h.store.get_message(&asked.id).await.unwrap().is_delivered()
    })
    .await;
}

/// A message to somebody the task does not staff is refused, and nothing is
/// written: the channel carries what the agents said, not what they meant to.
#[tokio::test]
async fn a_message_to_an_agent_the_task_does_not_staff_is_refused() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let reviewer = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Reviewer,
            &cast.reviewer.id,
        )
        .await;

    let envelope: ErrorBody = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                ask("author", Some("01NOBODY"), "anyone there?"),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        envelope.error.message.contains("not staffed on task"),
        "{}",
        envelope.error.message
    );

    let listed: Vec<MessageDto> = h.json(get(&messages_uri(&cast)), StatusCode::OK).await;
    assert!(listed.is_empty(), "{listed:?}");
}

/// A verdict is a message, and only a reviewer of this task can give one:
/// the kind is what closes a round, so who may send it is checked.
#[tokio::test]
async fn only_a_reviewer_of_the_task_can_send_a_verdict() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.advance(&cast.task, TaskStatus::UnderReview).await;

    let (status, _) = h
        .send(as_session(
            &messages_uri(&cast),
            &author.id,
            serde_json::json!({
                "kind": "approve",
                "to_actor": "author",
                "to_agent_id": cast.author.id,
                "body": "I approve of myself.",
            }),
        ))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// And a verdict on a task nobody asked to have reviewed is refused too: a
/// round has to be open for one to close it.
#[tokio::test]
async fn a_verdict_outside_a_round_is_refused() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let reviewer = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Reviewer,
            &cast.reviewer.id,
        )
        .await;
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let envelope: ErrorBody = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                serde_json::json!({
                    "kind": "approve",
                    "to_actor": "author",
                    "to_agent_id": cast.author.id,
                    "body": "looks right",
                }),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        envelope.error.message.contains("only taken under_review"),
        "{}",
        envelope.error.message
    );
}

/// An agent can write to the orchestrator, which is what keeps it open to the
/// coding agents for the whole goal: it is addressed by what it is, since a
/// goal has one and it is staffed on no task.
#[tokio::test]
async fn an_agent_writes_to_the_orchestrator_and_it_reaches_its_pane() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let orchestrator = h.orchestrator_session(&cast.goal, "orc").await;
    h.pane_exists(&orchestrator);
    h.set_status(&orchestrator, SessionStatus::Idle).await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;

    let asked: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &author.id,
                ask("orchestrator", None, "Does this task cover the CLI too?"),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(asked.to_actor, Actor::Orchestrator);
    assert_eq!(asked.to_agent_id, None);

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();

    eventually(
        TIMEOUT,
        "the message to reach the orchestrator",
        async || {
            h.pasted(&orchestrator)
                .contains("Does this task cover the CLI too?")
        },
    )
    .await;
}

/// Asking for a review is the author writing to its reviewers, so the channel
/// carries it: one message each, with the summary the author asked with.
#[tokio::test]
async fn a_review_request_reaches_every_reviewer_as_a_message() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let second = h
        .store
        .update_task(
            &cast.task.id,
            ariadne_store::TaskUpdate {
                reviewers: Some(vec![
                    ariadne_store::NewTaskAgent::new(Seat::Reviewer, ["code-review"]),
                    ariadne_store::NewTaskAgent::new(Seat::Reviewer, ["security-review"]),
                ]),
                ..Default::default()
            },
        )
        .await
        .map(|_| ())
        .and(h.store.list_task_reviewers(&cast.task.id).await)
        .unwrap();
    assert_eq!(second.len(), 2);
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.advance(&cast.task, TaskStatus::InProgress).await;

    h.json::<serde_json::Value>(
        as_session(
            &format!("/v1/tasks/{}/transitions", cast.task.id),
            &author.id,
            serde_json::json!({"to": "under_review", "reason": "Renamed the flag and tested it."}),
        ),
        StatusCode::OK,
    )
    .await;

    let listed: Vec<MessageDto> = h.json(get(&messages_uri(&cast)), StatusCode::OK).await;
    let requests: Vec<&MessageDto> = listed
        .iter()
        .filter(|m| m.kind == MessageKind::ReviewRequest)
        .collect();
    assert_eq!(requests.len(), 2, "one per reviewer: {listed:?}");
    for reviewer in &second {
        assert!(
            requests
                .iter()
                .any(|m| m.to_agent_id.as_deref() == Some(reviewer.id.as_str())),
            "no request for {}: {requests:?}",
            reviewer.id
        );
    }
    assert!(
        requests
            .iter()
            .all(|m| m.body == "Renamed the flag and tested it."),
        "the summary the author asked with is what they were sent: {requests:?}"
    );
}
