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

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;

use ariadne_api::SESSION_HEADER;
use ariadne_api::error::ErrorBody;
use ariadne_api::messages::MessageDto;
use ariadne_core::{Actor, MessageKind, Seat, SessionStatus, TaskStatus};
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_store::{AgentSession, EventFilter};

use common::acp;
use common::{Cast, Harness, TIMEOUT, as_session, eventually, get, harness, post_json, test_pin};

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
        "to_actor": to_actor,
        "to_agent_id": to_agent_id,
        "body": body,
    })
}

/// The review column's agent writes to the develop column's agent, and that
/// agent writes back what it needs. Neither of them left the task to do it,
/// and the task is in the column it was.
///
/// Both are the same thing: one message naming the agent it is for. Nothing
/// threads and nothing is a reply — each one reaches its agent as a turn, so
/// a channel that invites one back spends two turns saying nothing.
#[tokio::test]
async fn agents_write_to_each_other_without_leaving_the_task() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let reviewer = h.agent_session(&cast, "review").await;
    let author = h.agent_session(&cast, "develop").await;
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message(
                    "agent",
                    Some(&cast.develop().id),
                    "The retry loop has no bound and the caller has one.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(sent.kind, MessageKind::Message);
    assert_eq!(sent.from_actor, Actor::Agent);
    assert_eq!(
        sent.from_agent_id.as_deref(),
        Some(cast.review().id.as_str())
    );
    assert_eq!(
        sent.to_agent_id.as_deref(),
        Some(cast.develop().id.as_str())
    );
    assert_eq!(sent.delivered_at, None, "nothing has typed it yet");

    let answered: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &author.id,
                message(
                    "agent",
                    Some(&cast.review().id),
                    "The caller retries too, so the inner one stays.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(answered.kind, MessageKind::Message);
    assert_eq!(answered.to_actor, Actor::Agent, "back to whoever wrote");
    assert_eq!(
        answered.to_agent_id.as_deref(),
        Some(cast.review().id.as_str())
    );

    // And the task is where it was: a question moves no column.
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    assert_eq!(
        (task.status(), task.step.as_deref()),
        (TaskStatus::InProgress, Some("develop"))
    );
}

/// The transport: the daemon hands the message to the recipient's agent as a
/// prompt and stamps it delivered, so the agent reads it as a turn.
#[tokio::test]
async fn a_message_is_handed_to_the_agent_it_was_sent_to() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let author = h.agent_session(&cast, "develop").await;
    h.agent_runs(&author).await;
    h.set_status(&author, SessionStatus::Idle).await;
    let reviewer = h.agent_session(&cast, "review").await;
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message(
                    "agent",
                    Some(&cast.develop().id),
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
    // Named by its seat, its agent id, the task it is of and its skills: an
    // agent has no name of its own, and two agents on the same skills would
    // otherwise be the same sender to the reader.
    assert!(
        pasted.contains(&format!(
            r#"the agent {} of task {} "{}" (code-review)"#,
            cast.review().id,
            cast.task.id,
            cast.task.title
        )),
        "{pasted}"
    );
    assert!(
        pasted.contains("Answer with send_message to that agent id and task id."),
        "{pasted}"
    );

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
    let reviewer = h.agent_session(&cast, "review").await;

    let envelope: ErrorBody = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message("agent", Some("01NOBODY"), "anyone there?"),
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
    let author = h.agent_session(&cast, "develop").await;

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

/// A goal with two tasks staffed on the same skills would leave the
/// orchestrator unable to tell their agents' relays apart by seat and skills
/// alone, so each one also names its own task and agent.
#[tokio::test]
async fn two_tasks_on_the_same_skills_are_named_apart_in_their_relay_to_the_orchestrator() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let second_task = h
        .task_on(&cast.goal, &cast.repo, "Second task", test_pin())
        .await;
    let second_author = h
        .store
        .list_task_agents(&second_task.id)
        .await
        .unwrap()
        .remove(0);

    let orchestrator = h.orchestrator_session(&cast.goal).await;
    h.agent_runs(&orchestrator).await;
    h.set_status(&orchestrator, SessionStatus::Idle).await;
    let author = h.agent_session(&cast, "develop").await;
    let second_author_session = h
        .session(
            &cast.goal,
            Some(&second_task),
            Seat::Agent,
            &second_author.id,
        )
        .await;

    h.json::<MessageDto>(
        as_session(
            &messages_uri(&cast),
            &author.id,
            message("orchestrator", None, "The first task needs a decision."),
        ),
        StatusCode::CREATED,
    )
    .await;
    h.json::<MessageDto>(
        as_session(
            &format!("/v1/tasks/{}/messages", second_task.id),
            &second_author_session.id,
            message("orchestrator", None, "The second task needs a decision."),
        ),
        StatusCode::CREATED,
    )
    .await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();
    eventually(
        TIMEOUT,
        "the first task's relay to reach the orchestrator",
        async || {
            h.prompted(&orchestrator)
                .contains("The first task needs a decision.")
        },
    )
    .await;

    sched
        .send(SchedEvent::TaskChanged(second_task.id.clone()))
        .unwrap();
    eventually(
        TIMEOUT,
        "the second task's relay to reach the orchestrator",
        async || {
            h.prompted(&orchestrator)
                .contains("The second task needs a decision.")
        },
    )
    .await;

    let pasted = h.prompted(&orchestrator);
    assert!(
        pasted.contains(&format!(
            r#"the agent {} of task {} "{}" (coding)"#,
            cast.develop().id,
            cast.task.id,
            cast.task.title
        )),
        "{pasted}"
    );
    assert!(
        pasted.contains(&format!(
            r#"the agent {} of task {} "{}" (coding)"#,
            second_author.id, second_task.id, second_task.title
        )),
        "{pasted}"
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
    let author = h.agent_session(&cast, "develop").await;
    h.agent_runs(&author).await;
    h.set_status(&author, SessionStatus::Idle).await;
    let reviewer = h.agent_session(&cast, "review").await;
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message(
                    "agent",
                    Some(&cast.develop().id),
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
    let author = h.agent_session(&cast, "develop").await;
    h.agent_runs(&author).await;
    h.set_status(&author, SessionStatus::Idle).await;
    let reviewer = h.agent_session(&cast, "review").await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    let write = |body: &'static str| {
        h.json::<MessageDto>(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message("agent", Some(&cast.develop().id), body),
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
    let resumed = h.agent_session(&cast, "develop").await;
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

/// A develop agent that sits inside a turn until the test writes the file
/// this answers with: the agent of a task that waits for an answer inside
/// one turn. The review agent is the session the test writes to it as.
async fn author_held_in_a_turn(h: &Harness) -> (Cast, AgentSession, AgentSession, PathBuf) {
    author_held_in(h, json!({})).await
}

/// [`author_held_in_a_turn`], with `ending` added to the held turn's script:
/// what the agent does once the test lets the turn go.
async fn author_held_in(
    h: &Harness,
    ending: serde_json::Value,
) -> (Cast, AgentSession, AgentSession, PathBuf) {
    let release = h.at("release-turn");
    let mut turn = json!({"wait_for": release.display().to_string(), "updates": [], "stop_reason": "end_turn"});
    turn.as_object_mut()
        .unwrap()
        .extend(ending.as_object().unwrap().clone());
    let mut held = acp::script();
    held["prompts"] = json!([turn]);
    h.agent.reprogram(held);
    // A real repository: the scheduler cuts the task's worktree from it when
    // it puts the agent back on its feet.
    h.git_repo("repo");
    let cast = h.active_cast().await;
    // In its first column and briefed, so the console input below is the
    // agent's first turn and a message its next prompt.
    h.advance(&cast.task, TaskStatus::InProgress).await;
    h.briefed(&cast.task).await;
    let author = h.agent_session(&cast, "develop").await;
    h.agent_runs(&author).await;
    h.set_status(&author, SessionStatus::Idle).await;
    let reviewer = h.agent_session(&cast, "review").await;
    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", author.id),
            json!({"text": "HOLD: wait for the reviewer's answer."}),
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let reached = h.at("release-turn.reached");
    eventually(TIMEOUT, "the author's turn to be held open", async || {
        reached.exists()
    })
    .await;
    (cast, author, reviewer, release)
}

/// One scheduler pass over the task, run to its end.
async fn pass(sched: &UnboundedSender<SchedEvent>, cast: &Cast) {
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();
    scheduler::flush_for_test(sched).await;
}

/// The kind of every event this session stored, in order.
async fn event_kinds(h: &Harness, session: &AgentSession) -> Vec<String> {
    h.store
        .list_events(EventFilter {
            session_id: Some(session.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect()
}

/// The prompts this session's agent was sent that carry `body`.
fn prompts_carrying(h: &Harness, session: &AgentSession, body: &str) -> usize {
    h.prompts_to(session)
        .iter()
        .filter(|prompt| prompt.contains(body))
        .count()
}

/// A message to an agent mid-turn reaches it through a read.
///
/// The agent waits for an answer inside one turn, and polls the channel. The
/// scheduler queues the message behind that turn, which never ends while the
/// agent waits, so a stamp at the queue would hide the message from every
/// poll. The stamp is at the read instead, and the queued prompt is then
/// skipped. The second message, which arrives as a prompt after the turn,
/// proves the queue was walked past the first.
#[tokio::test]
async fn a_message_to_an_agent_mid_turn_reaches_it_through_a_read() {
    let h = harness().await;
    let (cast, author, reviewer, release) = author_held_in_a_turn(&h).await;
    let write = |body: &'static str| {
        h.json::<MessageDto>(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message("agent", Some(&cast.develop().id), body),
            ),
            StatusCode::CREATED,
        )
    };

    let first = write("FIRST: the bound is the caller's.").await;
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    pass(&sched, &cast).await;
    assert!(
        !h.store.get_message(&first.id).await.unwrap().is_delivered(),
        "a message queued behind a turn is not stamped"
    );

    let read: Vec<MessageDto> = h
        .json(
            read_as(&format!("{}?deliver=true", messages_uri(&cast)), &author.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        read.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [first.id.as_str()],
        "the agent mid-turn reads the queued message"
    );
    assert!(read[0].delivered_at.is_some(), "{read:?}");

    std::fs::write(&release, "go").unwrap();
    eventually(TIMEOUT, "the held turn to end", async || {
        event_kinds(&h, &author)
            .await
            .iter()
            .any(|kind| kind == "stop")
    })
    .await;
    write("SECOND: and the inner one stays.").await;
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();
    eventually(
        TIMEOUT,
        "the second message to arrive as a prompt",
        async || h.prompted(&author).contains("SECOND:"),
    )
    .await;
    assert_eq!(
        prompts_carrying(&h, &author, "FIRST:"),
        0,
        "a message the agent read was typed at it as well: {}",
        h.prompted(&author)
    );
}

/// A message queued behind a turn is stamped when its prompt goes out, and
/// not before.
#[tokio::test]
async fn a_message_queued_behind_a_turn_is_stamped_when_its_prompt_goes_out() {
    let h = harness().await;
    let (cast, author, reviewer, release) = author_held_in_a_turn(&h).await;
    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message(
                    "agent",
                    Some(&cast.develop().id),
                    "QUEUED: the bound is the caller's.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    pass(&sched, &cast).await;
    assert!(
        !h.store.get_message(&sent.id).await.unwrap().is_delivered(),
        "a message queued behind a turn is not stamped"
    );

    std::fs::write(&release, "go").unwrap();
    eventually(TIMEOUT, "the queued prompt to go out", async || {
        h.prompted(&author).contains("QUEUED:")
    })
    .await;
    assert!(
        h.store.get_message(&sent.id).await.unwrap().is_delivered(),
        "the driver claims the message before its prompt goes out"
    );
    let inbox: Vec<MessageDto> = h
        .json(
            read_as(&format!("{}?deliver=true", messages_uri(&cast)), &author.id),
            StatusCode::OK,
        )
        .await;
    assert!(
        inbox.is_empty(),
        "a read handed over a message the agent had as a prompt: {inbox:?}"
    );
}

/// A message is queued once, however many scheduler passes hand it while
/// the turn it waits behind runs.
///
/// The message is unstamped while it waits, so every pass hands it again.
/// The message written after the turn proves the queue is walked to its end.
#[tokio::test]
async fn a_message_is_queued_once_across_scheduler_passes() {
    let h = harness().await;
    let (cast, author, reviewer, release) = author_held_in_a_turn(&h).await;
    let write = |body: &'static str| {
        h.json::<MessageDto>(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message("agent", Some(&cast.develop().id), body),
            ),
            StatusCode::CREATED,
        )
    };

    write("ONCE: the bound is the caller's.").await;
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    pass(&sched, &cast).await;
    pass(&sched, &cast).await;

    std::fs::write(&release, "go").unwrap();
    eventually(TIMEOUT, "the queued prompt to go out", async || {
        h.prompted(&author).contains("ONCE:")
    })
    .await;
    write("AFTER: and the inner one stays.").await;
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();
    eventually(
        TIMEOUT,
        "the next message to arrive as a prompt",
        async || h.prompted(&author).contains("AFTER:"),
    )
    .await;
    assert_eq!(
        prompts_carrying(&h, &author, "ONCE:"),
        1,
        "{}",
        h.prompted(&author)
    );
}

/// Messages keep their order across a read and the queue: a read mid-turn
/// returns every queued message, oldest first, and none of them is typed at
/// the agent after the turn.
#[tokio::test]
async fn messages_keep_their_order_across_a_read_and_the_queue() {
    let h = harness().await;
    let (cast, author, reviewer, release) = author_held_in_a_turn(&h).await;
    let write = |body: &'static str| {
        h.json::<MessageDto>(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message("agent", Some(&cast.develop().id), body),
            ),
            StatusCode::CREATED,
        )
    };

    let older = write("OLDER: the bound is the caller's.").await;
    let newer = write("NEWER: and the inner one stays.").await;
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    pass(&sched, &cast).await;

    let read: Vec<MessageDto> = h
        .json(
            read_as(&format!("{}?deliver=true", messages_uri(&cast)), &author.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        read.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [older.id.as_str(), newer.id.as_str()],
        "a read returns the queued messages oldest first"
    );

    std::fs::write(&release, "go").unwrap();
    write("THIRD: the caller retries too.").await;
    sched
        .send(SchedEvent::TaskChanged(cast.task.id.clone()))
        .unwrap();
    eventually(
        TIMEOUT,
        "the third message to arrive as a prompt",
        async || h.prompted(&author).contains("THIRD:"),
    )
    .await;
    let prompted = h.prompted(&author);
    assert!(
        !prompted.contains("OLDER:") && !prompted.contains("NEWER:"),
        "a message the agent read was typed at it as well: {prompted}"
    );
}

/// A message whose prompt the agent answered with an error stays delivered.
///
/// The error is the agent's answer, so the agent read the prompt and has the
/// text. Only a prompt the driver never wrote gives its claim back. The
/// driver's `session.error` comes after that decision, so it is what the test
/// waits for.
#[tokio::test]
async fn a_message_the_agent_answered_with_an_error_stays_delivered() {
    let h = harness().await;
    let mut refusing = acp::script();
    refusing["unsupported_methods"] = json!(["session/prompt"]);
    h.agent.reprogram(refusing);
    let cast = h.active_cast().await;
    let author = h.agent_session(&cast, "develop").await;
    h.agent_runs(&author).await;
    h.set_status(&author, SessionStatus::Idle).await;
    let reviewer = h.agent_session(&cast, "review").await;
    // In its column, and already briefed: the message is the next prompt.
    h.advance(&cast.task, TaskStatus::InProgress).await;
    h.briefed(&cast.task).await;
    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message(
                    "agent",
                    Some(&cast.develop().id),
                    "REFUSED: the bound is the caller's.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    pass(&sched, &cast).await;
    eventually(TIMEOUT, "the agent's error to end the driver", async || {
        event_kinds(&h, &author)
            .await
            .iter()
            .any(|kind| kind == "session.error")
    })
    .await;
    assert!(
        h.prompted(&author).contains("REFUSED:"),
        "the prompt reached the agent: {}",
        h.prompted(&author)
    );
    assert!(
        h.store.get_message(&sent.id).await.unwrap().is_delivered(),
        "an error answer gave the claim back, so the message would go out again"
    );
}

/// A claimed message whose prompt the agent's stdin never took gives its
/// claim back, and reaches the agent's next launch.
///
/// The agent closes its stdin as it ends the held turn, so the queued
/// prompt finds no reader: the driver claims the message, the write fails,
/// and the connection goes down with the prompt unwritten. A stamp kept
/// there would hide the message from every later pass and every read.
#[tokio::test]
async fn an_unwritten_prompt_gives_its_message_back_for_the_relaunch() {
    let h = harness().await;
    let (cast, author, reviewer, release) = author_held_in(&h, json!({"close_stdin": true})).await;
    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message(
                    "agent",
                    Some(&cast.develop().id),
                    "UNWRITTEN: the bound is the caller's.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    pass(&sched, &cast).await;
    // The scheduler puts the agent back on its feet, and the next launch is
    // an agent that reads what it is sent.
    let mut next = acp::script();
    next["stored_sessions"] = json!(["stub-session"]);
    h.agent.reprogram(next);

    std::fs::write(&release, "go").unwrap();
    eventually(
        TIMEOUT,
        "the message to reach the relaunched agent",
        async || h.prompted(&author).contains("UNWRITTEN:"),
    )
    .await;
    assert_eq!(
        prompts_carrying(&h, &author, "UNWRITTEN:"),
        1,
        "{}",
        h.prompted(&author)
    );
    assert!(
        event_kinds(&h, &author)
            .await
            .iter()
            .any(|kind| kind == "session_end"),
        "the message reached a later launch, not the one whose stdin closed"
    );
    eventually(TIMEOUT, "the relaunch to stamp the message", async || {
        h.store.get_message(&sent.id).await.unwrap().is_delivered()
    })
    .await;
}

/// A message to the agent of a column the task is not in waits for that
/// column: the agent sits idle, and a message is no reason to wake it. Once
/// the task comes back to its column, the next pass hands the message over.
#[tokio::test]
async fn a_message_to_an_idle_column_waits_for_its_column() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let author = h.agent_session(&cast, "develop").await;
    h.agent_runs(&author).await;
    h.set_status(&author, SessionStatus::Idle).await;
    // The review column's agent is up and briefed, so the scheduler has
    // nothing to start and nothing to say to it.
    let reviewer = h.agent_session(&cast, "review").await;
    h.agent_runs(&reviewer).await;
    h.set_status(&reviewer, SessionStatus::Idle).await;
    h.advance_to(&cast.task, "review").await;
    h.briefed(&cast.task).await;

    let sent: MessageDto = h
        .json(
            as_session(
                &messages_uri(&cast),
                &reviewer.id,
                message(
                    "agent",
                    Some(&cast.develop().id),
                    "WAITING: the bound is the caller's.",
                ),
            ),
            StatusCode::CREATED,
        )
        .await;
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    pass(&sched, &cast).await;
    pass(&sched, &cast).await;
    assert_eq!(
        prompts_carrying(&h, &author, "WAITING:"),
        0,
        "a message reached the agent of a column the task is not in: {}",
        h.prompted(&author)
    );
    assert!(!h.store.get_message(&sent.id).await.unwrap().is_delivered());

    h.store
        .move_step(
            &cast.task.id,
            "develop",
            Actor::Daemon,
            "the test sent the task back",
            None,
        )
        .await
        .unwrap();
    pass(&sched, &cast).await;
    eventually(
        TIMEOUT,
        "the message to reach its column's agent",
        async || h.prompted(&author).contains("WAITING:"),
    )
    .await;
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
            from_actor: Actor::Agent,
            from_agent_id: Some(cast.develop().id.clone()),
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
/// column-to-column thread to whoever read the goal — the whole of what
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
            from_actor: Actor::Agent,
            from_agent_id: Some(cast.develop().id.clone()),
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
            from_actor: Actor::Agent,
            from_agent_id: Some(cast.review().id.clone()),
            from_session: None,
            to_actor: Actor::Agent,
            to_agent_id: Some(cast.develop().id.clone()),
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
