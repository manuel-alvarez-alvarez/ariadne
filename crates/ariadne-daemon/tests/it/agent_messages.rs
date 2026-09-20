//! What one agent says to another, and how it gets there.
//!
//! The channel is one table and one transport: a message is written by the
//! agent that sent it and handed to the recipient as a prompt by the daemon, so it
//! arrives as a turn rather than as something anybody has to go and look for.
//!
//! What is worth pinning is the addressing — every message has exactly one
//! recipient, and an answer goes back to whoever asked without naming them —
//! and the delivery, which is the whole reason the channel is worth having.

use crate::common;

use axum::body::Body;
use axum::http::{Request, StatusCode};

use ariadne_api::SESSION_HEADER;
use ariadne_api::error::ErrorBody;
use ariadne_api::messages::MessageDto;
use ariadne_core::{Actor, MessageKind, Seat, SessionStatus, TaskStatus};
use ariadne_daemon::scheduler::{self, SchedEvent};

use common::{Cast, TIMEOUT, as_session, eventually, get, harness, sh, test_pin};

fn messages_uri(cast: &Cast) -> String {
    format!("/v1/tasks/{}/messages", cast.task.id)
}

/// A read an agent makes as itself, carrying the session header the daemon
/// identifies it by: the shape `read_messages` calls the channel with.
fn read_as(uri: &str, session_id: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(SESSION_HEADER, session_id)
        .body(Body::empty())
        .unwrap()
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

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
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

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
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
                    ariadne_store::NewTaskAgent::new(Seat::Reviewer, ["spec-review"], test_pin()),
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

/// A review request reaches its reviewer in the review briefing. That
/// briefing carries the author's summary and stamps the channel row, so no
/// bare message follows the reviewer's first turn.
#[tokio::test]
async fn a_review_request_reaches_a_reviewer_once_as_its_briefing() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.active_cast().await;
    h.notify(&cast.task.id);
    eventually(TIMEOUT, "the author to start", async || {
        h.status(&cast.task.id).await == TaskStatus::InProgress
            && h.running_session(&cast.task.id, Seat::Author)
                .await
                .is_some()
    })
    .await;
    let author = h
        .running_session(&cast.task.id, Seat::Author)
        .await
        .expect("a live author session");
    let summary = "Renamed the flag and tested it.";

    h.json::<serde_json::Value>(
        as_session(
            &format!("/v1/tasks/{}/transitions", cast.task.id),
            &author.id,
            serde_json::json!({"to": "under_review", "reason": summary}),
        ),
        StatusCode::OK,
    )
    .await;

    eventually(TIMEOUT, "the reviewer to start", async || {
        h.running_session(&cast.task.id, Seat::Reviewer)
            .await
            .is_some()
    })
    .await;
    let reviewer = h
        .running_session(&cast.task.id, Seat::Reviewer)
        .await
        .expect("a live reviewer session");
    let request = h
        .store
        .list_messages(ariadne_store::MessageFilter {
            task_id: Some(cast.task.id.clone()),
            to_agent_id: Some(cast.reviewer.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .find(|message| message.kind() == Some(MessageKind::ReviewRequest))
        .expect("the review request");
    eventually(TIMEOUT, "the briefing to stamp the request", async || {
        h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered()
    })
    .await;
    eventually(TIMEOUT, "the reviewer's first turn to end", async || {
        h.session_status(&reviewer).await == SessionStatus::Idle
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let prompts = h.prompts_to(&reviewer);
    assert_eq!(prompts.len(), 1, "a bare message followed: {prompts:#?}");
    assert_eq!(
        prompts[0].matches(summary).count(),
        1,
        "the briefing did not carry the summary once: {}",
        prompts[0]
    );
    assert!(
        !prompts[0].contains("Message from your author"),
        "the review request arrived as a bare message: {}",
        prompts[0]
    );
}

/// A reviewer that sent a task back stays live for the next review, so the
/// second request reaches it as soon as the author asks rather than waiting
/// for the quiet clock to notice it again.
#[tokio::test]
async fn a_live_reviewer_is_briefed_at_once_for_a_second_review() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.active_cast().await;
    h.notify(&cast.task.id);
    eventually(TIMEOUT, "the author to start", async || {
        h.status(&cast.task.id).await == TaskStatus::InProgress
            && h.running_session(&cast.task.id, Seat::Author)
                .await
                .is_some()
    })
    .await;
    let author = h
        .running_session(&cast.task.id, Seat::Author)
        .await
        .expect("a live author session");

    h.json::<serde_json::Value>(
        as_session(
            &format!("/v1/tasks/{}/transitions", cast.task.id),
            &author.id,
            serde_json::json!({"to": "under_review", "reason": "the first review"}),
        ),
        StatusCode::OK,
    )
    .await;
    eventually(TIMEOUT, "the reviewer to start", async || {
        h.running_session(&cast.task.id, Seat::Reviewer)
            .await
            .is_some()
    })
    .await;
    let reviewer = h
        .running_session(&cast.task.id, Seat::Reviewer)
        .await
        .expect("a live reviewer session");
    eventually(
        TIMEOUT,
        "the reviewer to finish its first turn",
        async || h.session_status(&reviewer).await == SessionStatus::Idle,
    )
    .await;

    h.json::<MessageDto>(
        as_session(
            &messages_uri(&cast),
            &reviewer.id,
            serde_json::json!({
                "kind": "request_changes",
                "to_actor": "author",
                "to_agent_id": cast.author.id,
                "body": "take another look at the bounds",
            }),
        ),
        StatusCode::CREATED,
    )
    .await;
    eventually(
        TIMEOUT,
        "the author to resume after the changes",
        async || h.status(&cast.task.id).await == TaskStatus::InProgress,
    )
    .await;

    let summary = "the revised review";
    h.json::<serde_json::Value>(
        as_session(
            &format!("/v1/tasks/{}/transitions", cast.task.id),
            &author.id,
            serde_json::json!({"to": "under_review", "reason": summary}),
        ),
        StatusCode::OK,
    )
    .await;
    let request = h
        .store
        .list_messages(ariadne_store::MessageFilter {
            task_id: Some(cast.task.id.clone()),
            to_agent_id: Some(cast.reviewer.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .find(|message| {
            message.kind() == Some(MessageKind::ReviewRequest) && message.body == summary
        })
        .expect("the second review request");

    eventually(
        TIMEOUT,
        "the live reviewer to receive the new briefing",
        async || h.prompted(&reviewer).contains(summary),
    )
    .await;
    assert!(
        h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered(),
        "the second briefing stamps its request delivered"
    );
}

/// A review the author has just asked for is not closed by the answers to the
/// review before it.
///
/// A review opens in two writes: `transition_task` commits the status, and the
/// announcement writes one request row per reviewer. A scheduler pass between
/// the two reads the task under review while the newest request row is still
/// the review before this one's, so the verdicts that row bounds are that
/// review's answers. Read as this review's, the `request_changes` that closed
/// the round the author has just finished sends the task straight back to its
/// author, and the review it asked for is never held at all: no reviewer is
/// ever briefed for it.
///
/// The window is seeded rather than raced for. The store writes the status,
/// and the store announces nothing, so the pass below is exactly the pass that
/// lands in it.
#[tokio::test]
async fn a_review_is_not_closed_by_the_answers_to_the_review_before_it() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.active_cast().await;
    let answer = |kind: MessageKind, from: Actor, body: &str| ariadne_store::NewMessage {
        goal_id: cast.goal.id.clone(),
        task_id: Some(cast.task.id.clone()),
        kind,
        from_actor: from,
        from_agent_id: Some(match from {
            Actor::Author => cast.author.id.clone(),
            _ => cast.reviewer.id.clone(),
        }),
        from_session: None,
        to_actor: match from {
            Actor::Author => Actor::Reviewer,
            _ => Actor::Author,
        },
        to_agent_id: Some(match from {
            Actor::Author => cast.reviewer.id.clone(),
            _ => cast.author.id.clone(),
        }),
        body: body.to_string(),
    };

    // One review, asked for, announced, and answered with changes: what the
    // task carries when its author asks for the next one.
    h.advance(&cast.task, TaskStatus::UnderReview).await;
    h.store
        .send_message(answer(
            MessageKind::ReviewRequest,
            Actor::Author,
            "the first review",
        ))
        .await
        .unwrap();
    h.store
        .send_message(answer(
            MessageKind::RequestChanges,
            Actor::Reviewer,
            "take another look at the bounds",
        ))
        .await
        .unwrap();
    for status in [TaskStatus::ChangesRequested, TaskStatus::InProgress] {
        h.store
            .transition_task(&cast.task.id, status, Actor::Daemon, None, None)
            .await
            .unwrap();
    }

    // The author asks again, and nothing has announced it yet: the window.
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            Some("the revised review"),
            None,
        )
        .await
        .unwrap();
    // Waited out rather than slept past: the flush answers only once the
    // notify above's own reconciliation is done, which is the pass in the
    // window having actually run.
    h.notify(&cast.task.id);
    h.flush_scheduler().await;

    assert_eq!(
        h.status(&cast.task.id).await,
        TaskStatus::UnderReview,
        "the review the author just asked for is the review the task is under"
    );
}

/// A review request is not delivered and forgotten if the hand-off to the
/// live reviewer failed. Its runtime entry can close its prompt channel in
/// the moment between the liveness check and the hand-off — the same state
/// its own connection ending leaves behind for a few awaits before it is
/// deregistered — and `hand_prompt` says so by failing. Marked delivered at
/// that failed attempt regardless, the request would never reach it.
#[tokio::test]
async fn a_review_request_survives_a_failed_hand_off_to_a_live_reviewer() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.active_cast().await;
    h.notify(&cast.task.id);
    eventually(TIMEOUT, "the author to start", async || {
        h.status(&cast.task.id).await == TaskStatus::InProgress
            && h.running_session(&cast.task.id, Seat::Author)
                .await
                .is_some()
    })
    .await;
    let author = h
        .running_session(&cast.task.id, Seat::Author)
        .await
        .expect("a live author session");

    h.json::<serde_json::Value>(
        as_session(
            &format!("/v1/tasks/{}/transitions", cast.task.id),
            &author.id,
            serde_json::json!({"to": "under_review", "reason": "the first review"}),
        ),
        StatusCode::OK,
    )
    .await;
    eventually(TIMEOUT, "the reviewer to start", async || {
        h.running_session(&cast.task.id, Seat::Reviewer)
            .await
            .is_some()
    })
    .await;
    let reviewer = h
        .running_session(&cast.task.id, Seat::Reviewer)
        .await
        .expect("a live reviewer session");
    eventually(
        TIMEOUT,
        "the reviewer to finish its first turn",
        async || h.session_status(&reviewer).await == SessionStatus::Idle,
    )
    .await;

    h.json::<MessageDto>(
        as_session(
            &messages_uri(&cast),
            &reviewer.id,
            serde_json::json!({
                "kind": "request_changes",
                "to_actor": "author",
                "to_agent_id": cast.author.id,
                "body": "take another look at the bounds",
            }),
        ),
        StatusCode::CREATED,
    )
    .await;
    eventually(
        TIMEOUT,
        "the author to resume after the changes",
        async || h.status(&cast.task.id).await == TaskStatus::InProgress,
    )
    .await;

    // Live per the registry, but its prompt channel is already closed —
    // closed before the second round opens, so this is the hand-off that
    // fails.
    h.launcher.acp.close_prompt_channel_for_test(&reviewer.id);

    let summary = "the revised review";
    h.json::<serde_json::Value>(
        as_session(
            &format!("/v1/tasks/{}/transitions", cast.task.id),
            &author.id,
            serde_json::json!({"to": "under_review", "reason": summary}),
        ),
        StatusCode::OK,
    )
    .await;
    // Waited out rather than slept past: the flush answers only once the
    // transition above's own reconciliation is done, which is the failed
    // hand-off actually having been attempted.
    h.flush_scheduler().await;
    assert!(
        !h.prompted(&reviewer).contains(summary),
        "the closed channel could not have delivered anything"
    );
    let request = h
        .store
        .list_messages(ariadne_store::MessageFilter {
            task_id: Some(cast.task.id.clone()),
            to_agent_id: Some(cast.reviewer.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .find(|message| {
            message.kind() == Some(MessageKind::ReviewRequest) && message.body == summary
        })
        .expect("the second review request");
    assert!(
        !h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered(),
        "a failed hand-off does not stamp the request delivered"
    );

    // The agent comes back — killed and resumed through the daemon's own
    // relaunch, a fresh registration under the same session — and the
    // request is still owed.
    h.launcher.kill_session(&reviewer.id).await.unwrap();
    h.notify(&cast.task.id);
    eventually(
        TIMEOUT,
        "the live reviewer to be briefed now that it can hear it",
        async || h.prompted(&reviewer).contains(summary),
    )
    .await;
    assert_eq!(
        h.prompted(&reviewer).matches(summary).count(),
        1,
        "exactly one delivery, once the closed channel could carry it"
    );
    assert!(
        h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered(),
        "the retried briefing stamps its request delivered"
    );
}

/// A review request is not skipped and forgotten if the resume that would
/// spawn its first reviewer fails. `resume_reviewer`'s worktree setup
/// refuses a branch that does not exist — the same refusal a task whose
/// author never pushed anything would hit. Marking the delivery regardless
/// would not even show on a plain retry of the same resume: with no live
/// session, that path ignores the marker and tries the resume fresh every
/// pass. What it does poison is the session's own live path, later, once it
/// comes up on its own — so that is where this proves the fix landed: a
/// session already seeded starting, resumed by its own agent rather than by
/// another call the scheduler drives, comes up live and is briefed only if
/// the failed attempt left the marker clear.
#[tokio::test]
async fn a_review_request_survives_a_failed_resume_of_its_first_reviewer() {
    let h = harness().scheduler().await;
    let repo_path = h.git_repo("repo");
    let mut cast = h.cast_pinned(&test_pin().model, 1).await;
    cast.goal = h.activate(&cast.goal).await;
    // No branch for the task yet: the reviewer's worktree setup has nothing
    // to check out. No author ever spawns to create one either — that is
    // the point, an announcement with nothing behind it yet. The summary is
    // this test's own, so the briefing it travels in is unmistakable later.
    const SUMMARY: &str = "look over the seeded change";
    h.store
        .transition_task(&cast.task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::InProgress,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            Some(SUMMARY),
            None,
        )
        .await
        .unwrap();
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
            body: SUMMARY.to_string(),
        })
        .await
        .unwrap();

    // A reviewer session already starting, as if an earlier round had once
    // reported from it: the resume below finds this row resumable — the
    // same one that later comes up on its own — rather than falling back to
    // a fresh spawn.
    let seeded = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Reviewer,
            &cast.reviewer.id,
        )
        .await;
    h.store
        .set_session_internal_id(&seeded.id, "seeded-reviewer-session")
        .await
        .unwrap();

    h.notify(&cast.task.id);
    // Waited out rather than slept past: the flush answers only once the
    // notify above's own reconciliation is done, which is the failed resume
    // actually having been attempted.
    h.flush_scheduler().await;
    assert_eq!(
        h.session_status(&seeded).await,
        SessionStatus::Starting,
        "the worktree refusal lands before restart_session ever touches the row"
    );
    let request = h
        .store
        .list_messages(ariadne_store::MessageFilter {
            task_id: Some(cast.task.id.clone()),
            to_agent_id: Some(cast.reviewer.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .find(|message| message.kind() == Some(MessageKind::ReviewRequest))
        .expect("the review request");
    assert!(
        !h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered(),
        "a failed resume does not stamp the request delivered"
    );

    // The branch exists now — what a pushed change looks like. The same
    // seeded session comes up on its own, the way an agent already
    // mid-launch would — not through another resume the scheduler drives —
    // so whether it is briefed depends only on what the failed attempt
    // above left on `review_briefed`.
    sh(&repo_path, &format!("git branch {}", cast.task.branch));
    h.agent_runs(&seeded).await;
    h.notify(&cast.task.id);
    eventually(
        TIMEOUT,
        "the live reviewer to receive the briefing now that it can hear it",
        async || h.prompted(&seeded).contains(SUMMARY),
    )
    .await;
    assert_eq!(
        h.prompted(&seeded).matches(SUMMARY).count(),
        1,
        "exactly one delivery, once the failed attempt left the marker clear"
    );
    assert!(
        h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered(),
        "the live briefing stamps its request delivered"
    );
}

/// A reviewer started before the review's request rows were written is not
/// briefed a second time when they land.
///
/// A review opens in two writes: the status, then one request row per
/// reviewer. A scheduler pass between the two reads the review open and no
/// request, and the briefing it sends carries the summary all the same — it
/// is the transition's own reason. So the request that lands next is the one
/// that briefing carried, and it owes the reviewer nothing more. The two
/// writes are made apart here, because one pass in that window is what the
/// daemon hits by chance.
#[tokio::test]
async fn a_reviewer_briefed_before_the_request_row_is_not_briefed_again() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.active_cast().await;
    h.notify(&cast.task.id);
    eventually(TIMEOUT, "the author to start", async || {
        h.status(&cast.task.id).await == TaskStatus::InProgress
            && h.running_session(&cast.task.id, Seat::Author)
                .await
                .is_some()
    })
    .await;
    let summary = "Renamed the flag and tested it.";

    // The first write on its own: the review is open and its channel rows are
    // not in yet.
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            Some(summary),
            None,
        )
        .await
        .unwrap();
    h.notify(&cast.task.id);
    eventually(TIMEOUT, "the reviewer to start", async || {
        h.running_session(&cast.task.id, Seat::Reviewer)
            .await
            .is_some()
    })
    .await;
    let reviewer = h
        .running_session(&cast.task.id, Seat::Reviewer)
        .await
        .expect("a live reviewer session");
    eventually(TIMEOUT, "the reviewer's first turn to end", async || {
        h.session_status(&reviewer).await == SessionStatus::Idle
    })
    .await;

    // And the second write, which is the announcement the briefing went out
    // ahead of.
    let request = h
        .store
        .send_message(ariadne_store::NewMessage {
            goal_id: cast.goal.id.clone(),
            task_id: Some(cast.task.id.clone()),
            kind: MessageKind::ReviewRequest,
            from_actor: Actor::Author,
            from_agent_id: Some(cast.author.id.clone()),
            from_session: None,
            to_actor: Actor::Reviewer,
            to_agent_id: Some(cast.reviewer.id.clone()),
            body: summary.to_string(),
        })
        .await
        .unwrap();
    h.notify(&cast.task.id);
    eventually(TIMEOUT, "the request to be stamped delivered", async || {
        h.store
            .get_message(&request.id)
            .await
            .unwrap()
            .is_delivered()
    })
    .await;
    // Several passes over the request, so a briefing owed to it has every
    // chance to go out.
    for _ in 0..3 {
        h.notify(&cast.task.id);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    let prompts = h.prompts_to(&reviewer);
    assert_eq!(
        prompts.len(),
        1,
        "the reviewer was briefed twice for one request: {prompts:#?}"
    );
    assert_eq!(
        prompts[0].matches(summary).count(),
        1,
        "the briefing did not carry the summary once: {}",
        prompts[0]
    );
}

/// Two reviewers whose request rows land one after the other are each briefed
/// once.
///
/// The announcement writes one row per reviewer, so a pass can read one
/// reviewer's request written and the other's not. What each reviewer was
/// briefed for is its own row, which is written once and never moves — not
/// the newest row on the task, which walks forward as the announcement goes
/// out and would leave both reviewers briefed twice.
#[tokio::test]
async fn each_reviewer_is_briefed_once_when_its_request_row_lands_late() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let mut cast = h.cast_reviewed_by(2).await;
    cast.goal = h.activate(&cast.goal).await;
    let reviewers = h.store.list_task_reviewers(&cast.task.id).await.unwrap();
    h.notify(&cast.task.id);
    eventually(TIMEOUT, "the author to start", async || {
        h.status(&cast.task.id).await == TaskStatus::InProgress
            && h.running_session(&cast.task.id, Seat::Author)
                .await
                .is_some()
    })
    .await;
    let summary = "Renamed the flag and tested it.";

    // The status on its own, as the announcement is about to run.
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            Some(summary),
            None,
        )
        .await
        .unwrap();
    h.notify(&cast.task.id);
    eventually(TIMEOUT, "both reviewers to start", async || {
        reviewer_sessions(&h, &cast, &reviewers).await.len() == 2
    })
    .await;
    let sessions = reviewer_sessions(&h, &cast, &reviewers).await;
    for session in &sessions {
        eventually(TIMEOUT, "the reviewer's first turn to end", async || {
            h.session_status(session).await == SessionStatus::Idle
        })
        .await;
    }

    // And the announcement, one reviewer at a time with a pass in between.
    for reviewer in &reviewers {
        let request = h
            .store
            .send_message(ariadne_store::NewMessage {
                goal_id: cast.goal.id.clone(),
                task_id: Some(cast.task.id.clone()),
                kind: MessageKind::ReviewRequest,
                from_actor: Actor::Author,
                from_agent_id: Some(cast.author.id.clone()),
                from_session: None,
                to_actor: Actor::Reviewer,
                to_agent_id: Some(reviewer.id.clone()),
                body: summary.to_string(),
            })
            .await
            .unwrap();
        h.notify(&cast.task.id);
        eventually(TIMEOUT, "the request to be stamped delivered", async || {
            h.store
                .get_message(&request.id)
                .await
                .unwrap()
                .is_delivered()
        })
        .await;
    }
    // Several passes over both requests, so a briefing owed to either has
    // every chance to go out.
    for _ in 0..3 {
        h.notify(&cast.task.id);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    for session in &sessions {
        let prompts = h.prompts_to(session);
        assert_eq!(
            prompts.len(),
            1,
            "reviewer {} was briefed twice for one request: {prompts:#?}",
            session.task_agent_id.as_deref().unwrap_or_default()
        );
    }
}

/// The live session of each of a task's reviewers, in the order they are
/// staffed, and only once every one of them has one.
async fn reviewer_sessions(
    h: &common::Harness,
    cast: &Cast,
    reviewers: &[ariadne_store::TaskAgent],
) -> Vec<ariadne_store::AgentSession> {
    let sessions = h.sessions_of(&cast.task.id).await;
    reviewers
        .iter()
        .filter_map(|reviewer| {
            sessions.iter().find(|s| {
                s.seat() == Seat::Reviewer
                    && s.task_agent_id.as_deref() == Some(reviewer.id.as_str())
                    && s.launched_at.is_some()
            })
        })
        .cloned()
        .collect()
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

/// A message the daemon handed to its agent as a prompt is gone from what a
/// default read gives that agent back.
///
/// One stamp gates every way a message reaches an agent. The prompt is the
/// first of them, so what the read has left to hand over is what the prompt
/// did not: a default read is the inbox, not the transcript. The transcript
/// is `all`, and it holds the delivered message too — it is what a second
/// review reads the last verdict off.
#[tokio::test]
async fn a_message_handed_over_as_a_prompt_is_absent_from_a_default_read() {
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
                    "The bound is the caller's.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();
    eventually(TIMEOUT, "the message to be stamped delivered", async || {
        h.store.get_message(&sent.id).await.unwrap().is_delivered()
    })
    .await;

    let inbox: Vec<MessageDto> = h
        .json(
            read_as(&format!("{}?deliver=true", messages_uri(&cast)), &author.id),
            StatusCode::OK,
        )
        .await;
    assert!(
        inbox.is_empty(),
        "the agent was handed a message it had already read as a turn: {inbox:?}"
    );

    let thread: Vec<MessageDto> = h
        .json(read_as(&messages_uri(&cast), &author.id), StatusCode::OK)
        .await;
    assert_eq!(
        thread.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [sent.id.as_str()],
        "the whole thread holds what was delivered"
    );
    assert!(thread[0].delivered_at.is_some());
}

/// A message a read handed over is never handed over as a prompt afterwards,
/// and it survives a resume of its recipient and a restart of the daemon.
///
/// The read is a delivery like the prompt, so it stamps what it gives. The
/// stamp is a row in the database rather than anything the scheduler holds in
/// memory, which is what makes it hold across a session that came up again
/// and a scheduler that started from nothing.
///
/// What proves the skip is the next message: the transport walks one batch in
/// the order it was written, so a second message reaching the agent is that
/// pass having read the first and passed it over.
#[tokio::test]
async fn a_message_a_read_hands_over_is_never_handed_over_as_a_prompt() {
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
    let write = |body: &'static str| {
        h.json::<MessageDto>(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message("author", Some(&cast.author.id), body),
            ),
            StatusCode::CREATED,
        )
    };

    let read = write("READ: the bound is the caller's.").await;
    let handed: Vec<MessageDto> = h
        .json(
            read_as(&format!("{}?deliver=true", messages_uri(&cast)), &author.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        handed.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [read.id.as_str()]
    );
    assert!(
        handed[0].delivered_at.is_some(),
        "a read that hands a message over stamps it: {handed:?}"
    );

    // The recipient comes up again, and the daemon with it.
    h.set_status(&author, SessionStatus::Exited).await;
    let resumed = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.agent_runs(&resumed).await;
    h.set_status(&resumed, SessionStatus::Idle).await;
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    let after = write("AFTER: and the inner one stays.").await;
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();

    eventually(TIMEOUT, "the message written after the read", async || {
        h.prompted(&resumed).contains("AFTER:")
    })
    .await;
    assert!(
        !h.told(&resumed.id).contains("READ:"),
        "a message the agent had read was typed at it again: {}",
        h.told(&resumed.id)
    );
    assert!(
        h.store.get_message(&after.id).await.unwrap().is_delivered(),
        "and the one that did go out is stamped too"
    );
}

/// A change request reaches its author once.
///
/// The author of a task with one author is resumed with the feedback of every
/// reviewer that asked for changes, in a briefing that says what the round
/// decided. That briefing is the delivery of each verdict it carries, and
/// stamps it, so the transport does not type the same words at the author a
/// second time as a bare message.
#[tokio::test]
async fn a_change_request_reaches_its_author_once() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
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
            body: "the first review".into(),
        })
        .await
        .unwrap();

    let verdict: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                serde_json::json!({
                    "kind": "request_changes",
                    "to_actor": "author",
                    "to_agent_id": cast.author.id,
                    "body": "BOUND: the retry loop has no bound.",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;

    h.notify(&cast.task.id);
    eventually(TIMEOUT, "the author to be sent back to work", async || {
        h.status(&cast.task.id).await == TaskStatus::InProgress
    })
    .await;
    h.flush_scheduler().await;

    let told = h.told(&author.id);
    assert_eq!(
        told.matches("BOUND:").count(),
        1,
        "the change request reached its author more than once: {told}"
    );
    assert!(
        h.store
            .get_message(&verdict.id)
            .await
            .unwrap()
            .is_delivered(),
        "the briefing that carried it is its delivery, so it is stamped"
    );
}

/// A delivering read is refused a channel of another goal.
///
/// The stamp a delivering read spends cannot be given back, and the
/// orchestrator is the one seat a delivery narrows by its seat alone: every
/// goal has one, so `to_actor = orchestrator` on another goal's task names
/// that goal's orchestrator's messages. Naming a task is all it would take,
/// and the task scope check exempts the orchestrator — it reads and moves
/// every task of its own goal. So the goal is what is checked here.
#[tokio::test]
async fn a_delivering_read_is_refused_a_channel_of_another_goal() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let elsewhere = h.lone_session("elsewhere").await;

    let waiting = h
        .store
        .send_message(ariadne_store::NewMessage {
            goal_id: cast.goal.id.clone(),
            task_id: Some(cast.task.id.clone()),
            kind: MessageKind::Message,
            from_actor: Actor::Author,
            from_agent_id: Some(cast.author.id.clone()),
            from_session: None,
            to_actor: Actor::Orchestrator,
            to_agent_id: None,
            body: "The task names no CLI, and the spec it cites has one.".into(),
        })
        .await
        .unwrap();

    let envelope: ErrorBody = h
        .json(
            read_as(
                &format!("{}?deliver=true", messages_uri(&cast)),
                &elsewhere.id,
            ),
            StatusCode::FORBIDDEN,
        )
        .await;
    assert!(
        envelope.error.message.contains(&cast.goal.id),
        "{}",
        envelope.error.message
    );
    assert!(
        !h.store
            .get_message(&waiting.id)
            .await
            .unwrap()
            .is_delivered(),
        "another goal's orchestrator took delivery of a message meant for this one's"
    );
}

/// A goal's channel is the orchestrator's inbox, and holds nothing its tasks
/// said.
///
/// Every message carries the goal it belongs to, the ones about a task
/// included, so a read narrowed by the goal alone would hand every task's
/// author-to-reviewer thread to whoever read the goal — the whole of what
/// this task cut out of `read_messages`.
#[tokio::test]
async fn a_goals_channel_holds_none_of_what_its_tasks_said() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let about_the_goal = h
        .store
        .send_message(ariadne_store::NewMessage {
            goal_id: cast.goal.id.clone(),
            task_id: None,
            kind: MessageKind::Message,
            from_actor: Actor::Author,
            from_agent_id: Some(cast.author.id.clone()),
            from_session: None,
            to_actor: Actor::Orchestrator,
            to_agent_id: None,
            body: "The plan names no CLI.".into(),
        })
        .await
        .unwrap();
    h.store
        .send_message(ariadne_store::NewMessage {
            goal_id: cast.goal.id.clone(),
            task_id: Some(cast.task.id.clone()),
            kind: MessageKind::Message,
            from_actor: Actor::Reviewer,
            from_agent_id: Some(cast.reviewer.id.clone()),
            from_session: None,
            to_actor: Actor::Author,
            to_agent_id: Some(cast.author.id.clone()),
            body: "PRIVATE: the retry loop has no bound.".into(),
        })
        .await
        .unwrap();

    let thread: Vec<MessageDto> = h
        .json(
            get(&format!("/v1/goals/{}/messages", cast.goal.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        thread.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [about_the_goal.id.as_str()],
        "a task's own thread reached the goal's channel"
    );
}
