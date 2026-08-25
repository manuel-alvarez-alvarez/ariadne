//! Integration tests for addressing a conversation message.
//!
//! A message may name one addressee, the way a task names its profiles: a
//! profile id, a profile name, or the literal `"user"`. What each thread
//! accepts is who works in it — the planner in a goal's planning thread, and
//! the engineer, the reviewers and the planner in a task's — so that a
//! message never names someone who is not there to read it. Anything else is
//! refused with a sentence naming the addressees that would have worked.
//!
//! No tmux and no agent CLI: nothing here launches anything, the rows are
//! seeded through the store and only the message endpoints are exercised.

mod common;

use axum::http::StatusCode;

use ariadne_api::error::ErrorBody;
use ariadne_api::messages::MessageDto;
use ariadne_core::{AuthorRole, RecipientKind, Role};

use common::{Cast, Harness, harness, post_json};

/// The cast, plus the profile one test names and is refused: an addressee is
/// checked against the thread, not against the profiles that exist.
async fn cast_with_an_outsider(h: &Harness) -> Cast {
    let cast = h.cast().await;
    h.profile("outsider", Role::Reviewer).await;
    cast
}

/// Post a message, expecting it to be accepted.
async fn post_message(h: &Harness, uri: &str, body: serde_json::Value) -> MessageDto {
    h.json(post_json(uri, body), StatusCode::CREATED).await
}

/// Post a message, expecting the addressee to be refused.
async fn refused(h: &Harness, uri: &str, body: serde_json::Value) -> String {
    let envelope: ErrorBody = h
        .json(post_json(uri, body), StatusCode::BAD_REQUEST)
        .await;
    assert_eq!(envelope.error.code, "invalid_request");
    envelope.error.message
}

/// A profile is addressed by name or by id, and the resolved addressee comes
/// back on the message — with the profile's name, so a client renders it
/// without a lookup of its own.
#[tokio::test]
async fn a_task_message_addresses_a_participant_by_name_or_by_id() {
    let h = harness().await;
    let cast = h.cast().await;
    let uri = format!("/v1/tasks/{}/messages", cast.task.id);

    let by_name = post_message(
        &h,
        &uri,
        serde_json::json!({"body": "have a look", "to": "reviewer"}),
    )
    .await;
    let recipient = by_name.recipient.expect("addressed");
    assert_eq!(recipient.kind, RecipientKind::Profile);
    assert_eq!(
        recipient.profile_id.as_deref(),
        Some(cast.reviewer.id.as_str())
    );
    assert_eq!(recipient.profile_name.as_deref(), Some("reviewer"));

    let by_id = post_message(
        &h,
        &uri,
        serde_json::json!({"body": "back to you", "to": cast.engineer.id}),
    )
    .await;
    let recipient = by_id.recipient.expect("addressed");
    assert_eq!(
        recipient.profile_id.as_deref(),
        Some(cast.engineer.id.as_str())
    );
    assert_eq!(recipient.profile_name.as_deref(), Some("engineer"));

    // The planner of the goal takes part in every one of its task threads.
    let to_planner = post_message(
        &h,
        &uri,
        serde_json::json!({"body": "blocked", "to": "planner"}),
    )
    .await;
    assert_eq!(
        to_planner.recipient.and_then(|r| r.profile_id).as_deref(),
        Some(cast.planner.id.as_str())
    );

    // The user is addressed by the literal, and carries no profile.
    let to_user = post_message(
        &h,
        &uri,
        serde_json::json!({"body": "a question", "to": "user"}),
    )
    .await;
    let recipient = to_user.recipient.expect("addressed");
    assert_eq!(recipient.kind, RecipientKind::User);
    assert_eq!(recipient.profile_id, None);
    assert_eq!(recipient.profile_name, None);

    // And saying nothing addresses the thread.
    let unaddressed =
        post_message(&h, &uri, serde_json::json!({"body": "thinking out loud"})).await;
    assert!(unaddressed.recipient.is_none());

    // Every one of them reads back the same way from the thread.
    let thread: Vec<MessageDto> = h.get(&uri).await;
    assert_eq!(
        thread
            .iter()
            .map(|m| m
                .recipient
                .as_ref()
                .map(|r| (r.kind, r.profile_name.as_deref())))
            .collect::<Vec<_>>(),
        vec![
            Some((RecipientKind::Profile, Some("reviewer"))),
            Some((RecipientKind::Profile, Some("engineer"))),
            Some((RecipientKind::Profile, Some("planner"))),
            Some((RecipientKind::User, None)),
            None,
        ]
    );
}

/// A task thread reaches the people working on the task. A profile that is
/// none of them is refused, and so is a name no profile answers to at all;
/// both refusals name the addressees that would have worked.
#[tokio::test]
async fn a_task_thread_refuses_anyone_who_takes_no_part_in_it() {
    let h = harness().await;
    let cast = cast_with_an_outsider(&h).await;
    let uri = format!("/v1/tasks/{}/messages", cast.task.id);

    assert_eq!(
        refused(&h, &uri, serde_json::json!({"body": "psst", "to": "outsider"})).await,
        "outsider takes no part in this thread; address one of: \
         engineer, reviewer, planner, user"
    );
    assert_eq!(
        refused(&h, &uri, serde_json::json!({"body": "hello?", "to": "nobody"})).await,
        "no profile has the id or name nobody; address one of: \
         engineer, reviewer, planner, user"
    );
}

/// The planning thread is the planner's: the agents of a task are addressed in
/// that task's thread, where which task is meant is not in question.
#[tokio::test]
async fn a_goal_thread_addresses_only_its_planner_or_the_user() {
    let h = harness().await;
    let cast = h.cast().await;
    let uri = format!("/v1/goals/{}/messages", cast.goal.id);

    let to_planner = post_message(
        &h,
        &uri,
        serde_json::json!({"body": "how is it going", "to": "planner"}),
    )
    .await;
    assert_eq!(
        to_planner.recipient.and_then(|r| r.profile_id).as_deref(),
        Some(cast.planner.id.as_str())
    );
    let to_user = post_message(
        &h,
        &uri,
        serde_json::json!({"body": "a question", "to": "user"}),
    )
    .await;
    assert_eq!(to_user.recipient.map(|r| r.kind), Some(RecipientKind::User));

    assert_eq!(
        refused(
            &h,
            &uri,
            serde_json::json!({"body": "start please", "to": "engineer"})
        )
        .await,
        "engineer takes no part in this thread; address one of: planner, user"
    );
}

/// A task the user cancels says so in its own thread, addressed to them: an
/// ending is the one moment there is no agent left to notice it.
#[tokio::test]
async fn a_cancelled_task_tells_the_user_it_ended_and_why() {
    let h = harness().await;
    let cast = h.cast().await;

    let _: serde_json::Value = h
        .json(
            post_json(
                &format!("/v1/tasks/{}/cancel", cast.task.id),
                serde_json::json!({}),
            ),
            StatusCode::OK,
        )
        .await;

    let thread: Vec<MessageDto> = h
        .get(&format!("/v1/tasks/{}/messages", cast.task.id))
        .await;
    let told: Vec<&MessageDto> = thread
        .iter()
        .filter(|m| {
            m.recipient
                .as_ref()
                .is_some_and(|r| r.kind == RecipientKind::User)
        })
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].author_role, AuthorRole::System);
    assert!(told[0].body.contains(&cast.task.title), "{}", told[0].body);
    assert!(
        told[0].body.contains("cancelled by user"),
        "the notice does not carry the reason: {}",
        told[0].body
    );
}
