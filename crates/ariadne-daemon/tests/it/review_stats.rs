//! The review facts: a `verdict` fact for each verdict, and a `message` fact
//! for each stored task message.

use crate::common;

use ariadne_core::{Seat, TaskStatus};
use axum::http::StatusCode;

use common::{as_session, harness};

#[tokio::test]
async fn verdicts_record_rounds_and_messages_record_each_message() {
    let h = harness().await;
    let cast = h.active_cast().await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    let reviewer = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Reviewer,
            &cast.reviewer.id,
        )
        .await;
    h.advance(&cast.task, TaskStatus::UnderReview).await;
    let uri = format!("/v1/tasks/{}/messages", cast.task.id);
    for (kind, body) in [("request_changes", "revise"), ("approve", "good")] {
        let review = as_session(
            &uri,
            &author.id,
            serde_json::json!({"kind":"review_request", "to_actor":"reviewer", "to_agent_id":cast.reviewer.id, "body":"review"}),
        );
        let _: ariadne_api::messages::MessageDto = h.json(review, StatusCode::CREATED).await;
        let request = as_session(
            &uri,
            &reviewer.id,
            serde_json::json!({"kind": kind, "to_actor":"author", "to_agent_id":cast.author.id, "body":body}),
        );
        let _: ariadne_api::messages::MessageDto = h.json(request, StatusCode::CREATED).await;
    }
    let verdicts = h.facts("verdict").await;
    assert_eq!(verdicts.len(), 2);
    assert_eq!(verdicts[0].data["round"], 1);
    assert_eq!(verdicts[1].data["round"], 2);
    assert_eq!(h.facts("message").await.len(), 4);
}
