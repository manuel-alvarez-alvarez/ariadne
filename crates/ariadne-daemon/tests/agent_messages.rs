//! What one agent says to another, and how it gets there.
//!
//! The channel is one table and one transport: a message is written by the
//! agent that sent it and handed to the recipient as a prompt by the daemon, so it
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

use common::{Cast, TIMEOUT, as_session, eventually, get, harness, test_pin};

fn messages_uri(cast: &Cast) -> String {
    format!("/v1/tasks/{}/messages", cast.task.id)
}

/// One message body, as an agent sends it.
fn message(to_actor: &str, to_agent_id: Option<&str>, body: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "message",
        "to_actor": to_actor,
        "to_agent_id": to_agent_id,
        "body": body,
    })
}

/// A reviewer writes to the author mid-review, and the author writes back
/// what it needs. Neither of them left the task to do it, and the review is
/// where it was.
///
/// Both are the same thing: one message naming the agent it is for. Nothing
/// threads and nothing is a reply — each one reaches its agent as a turn, so
/// a channel that invites one back spends two turns saying nothing.
#[tokio::test]
async fn agents_write_to_each_other_without_leaving_the_task() {
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

    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message(
                    "author",
                    Some(&cast.author.id),
                    "The retry loop has no bound and the caller has one.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(sent.kind, MessageKind::Message);
    assert_eq!(sent.from_actor, Actor::Reviewer);
    assert_eq!(
        sent.from_agent_id.as_deref(),
        Some(cast.reviewer.id.as_str())
    );
    assert_eq!(sent.to_agent_id.as_deref(), Some(cast.author.id.as_str()));
    assert_eq!(sent.delivered_at, None, "nothing has typed it yet");

    let answered: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &author.id,
                message(
                    "reviewer",
                    Some(&cast.reviewer.id),
                    "The caller retries too, so the inner one stays.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(answered.kind, MessageKind::Message);
    assert_eq!(answered.to_actor, Actor::Reviewer, "back to whoever wrote");
    assert_eq!(
        answered.to_agent_id.as_deref(),
        Some(cast.reviewer.id.as_str())
    );

    // And the review is untouched: a question is not a vote.
    assert_eq!(h.store.open_verdicts(&cast.task.id).await.unwrap().len(), 0);
}

/// The transport: the daemon hands the message to the recipient's agent as a
/// prompt and stamps it delivered, so the agent reads it as a turn.
#[tokio::test]
async fn a_message_is_handed_to_the_agent_it_was_sent_to() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.agent_runs(&author).await;
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

    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message(
                    "author",
                    Some(&cast.author.id),
                    "The retry loop has no bound and the caller has one.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();

    eventually(TIMEOUT, "the message to reach the agent", async || {
        h.prompted(&author)
            .contains("The retry loop has no bound and the caller has one.")
    })
    .await;
    let pasted = h.prompted(&author);
    assert!(
        !pasted.contains(&sent.id),
        "the agent is told an id there is nothing to answer on: {pasted}"
    );
    // Named by the skills it works with: an agent has no name of its own.
    assert!(pasted.contains("reviewer (code-review)"), "{pasted}");

    eventually(TIMEOUT, "the message to be stamped delivered", async || {
        h.store.get_message(&sent.id).await.unwrap().is_delivered()
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
                message("author", Some("01NOBODY"), "anyone there?"),
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
async fn a_verdict_outside_a_review_is_refused() {
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
async fn an_agent_writes_to_the_orchestrator_and_it_reaches_its_agent() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let orchestrator = h.orchestrator_session(&cast.goal).await;
    h.agent_runs(&orchestrator).await;
    h.set_status(&orchestrator, SessionStatus::Idle).await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;

    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &author.id,
                message(
                    "orchestrator",
                    None,
                    "The task names no CLI, and the spec it cites has one.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(sent.to_actor, Actor::Orchestrator);
    assert_eq!(sent.to_agent_id, None);

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();

    eventually(
        TIMEOUT,
        "the message to reach the orchestrator",
        async || {
            h.prompted(&orchestrator)
                .contains("The task names no CLI, and the spec it cites has one.")
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
                    ariadne_store::NewTaskAgent::new(Seat::Reviewer, ["code-review"], test_pin()),
                    ariadne_store::NewTaskAgent::new(
                        Seat::Reviewer,
                        ["security-review"],
                        test_pin(),
                    ),
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

/// Every agent of a task stays up until the task is over. A reviewer that has
/// voted is not done with it — the author may have something to ask, and an
/// agent that was killed can be asked nothing — so the round it closed leaves
/// it idle at its prompt rather than ending it.
#[tokio::test]
async fn a_reviewer_that_voted_is_left_where_it_is() {
    let h = harness().scheduler().await;
    let cast = h.active_cast().await;
    // The author is there and resumable: an approved task briefs it to land
    // the change, and a task that cannot find one fails and takes every
    // session of it down, reviewer included.
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.make_resumable(&cast.task, &author).await;
    h.agent_runs(&author).await;
    h.set_status(&author, SessionStatus::Idle).await;

    let reviewer = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Reviewer,
            &cast.reviewer.id,
        )
        .await;
    h.agent_runs(&reviewer).await;
    h.set_status(&reviewer, SessionStatus::Idle).await;
    h.advance(&cast.task, TaskStatus::UnderReview).await;
    h.verdict_from(&cast.task, &reviewer, MessageKind::Approve, "Looks fine.")
        .await;

    h.notify(&cast.task.id);
    eventually(
        TIMEOUT,
        "the reviewer's verdict to close the round",
        async || h.status(&cast.task.id).await == TaskStatus::Approved,
    )
    .await;

    // Several passes past the verdict, and the reviewer is still where it was.
    for _ in 0..3 {
        h.notify(&cast.task.id);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(
        h.agent_is_running(&reviewer),
        "the reviewer was killed once its round closed"
    );
    assert_eq!(
        h.session_status(&reviewer).await,
        SessionStatus::Idle,
        "and it sits idle, ready for anything the author asks it"
    );
}

/// A reviewer votes once on each review it is asked for.
///
/// This used to be a unique index over the round a verdict carried. A review
/// is bounded by its own request now — a row rather than a column — so the
/// rule is read where a verdict is written, and it has to hold in both
/// directions: a second verdict on the open review is refused, and the same
/// reviewer votes again as soon as the author asks again.
#[tokio::test]
async fn only_one_verdict_per_reviewer_per_review_is_taken() {
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
    h.advance(&cast.task, TaskStatus::UnderReview).await;
    let verdict = |kind: &str, body: &str| {
        as_session(
            &messages_uri(&cast),
            &reviewer.id,
            serde_json::json!({
                "kind": kind,
                "to_actor": "author",
                "to_agent_id": cast.author.id,
                "body": body,
            }),
        )
    };

    let first: MessageDto = h
        .json(verdict("approve", "looks right"), StatusCode::CREATED)
        .await;
    assert_eq!(first.kind, MessageKind::Approve);

    let envelope: ErrorBody = h
        .json(
            verdict("request_changes", "on second thoughts"),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        envelope.error.message.contains("already given its verdict"),
        "{}",
        envelope.error.message
    );

    // Asked again, and the same reviewer has a verdict to give again.
    h.store
        .send_message(ariadne_store::NewMessage {
            goal_id: cast.goal.id.clone(),
            task_id: Some(cast.task.id.clone()),
            kind: MessageKind::ReviewRequest,
            from_actor: Actor::Author,
            from_agent_id: Some(cast.author.id.clone()),
            from_session: None,
            to_actor: Actor::Reviewer,
            to_agent_id: Some(cast.reviewer.id.clone()),
            body: "revised".into(),
        })
        .await
        .unwrap();

    let again: MessageDto = h
        .json(verdict("approve", "fixed now"), StatusCode::CREATED)
        .await;
    assert_eq!(again.kind, MessageKind::Approve);
    assert_eq!(
        h.store.open_verdicts(&cast.task.id).await.unwrap().len(),
        1,
        "and the verdict before the request is not counted in this review"
    );
}
