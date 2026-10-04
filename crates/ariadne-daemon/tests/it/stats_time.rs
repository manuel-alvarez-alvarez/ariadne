//! Integration tests for the `time` stats family (023): How long does it take?

use crate::common;

use ariadne_api::stats::TimeStatsDto;
use ariadne_core::{Actor, TaskStatus};

use common::harness;

/// A finished task reaches the time DTO, and the route is in the API document
/// under the `stats` tag.
#[tokio::test]
async fn the_time_stat_answers_and_is_in_the_api_document() {
    let h = harness().await;
    let cast = h.cast().await;
    for (to, actor) in [
        (TaskStatus::Ready, Actor::Daemon),
        (TaskStatus::InProgress, Actor::Daemon),
        (TaskStatus::UnderReview, Actor::Author),
        (TaskStatus::Approved, Actor::Daemon),
        (TaskStatus::Finished, Actor::Author),
    ] {
        h.store
            .transition_task(
                &cast.task.id,
                to,
                actor,
                None,
                (to == TaskStatus::Finished).then_some("abc123"),
            )
            .await
            .unwrap();
    }
    let stats: TimeStatsDto = h.get("/v1/stats/time").await;
    assert_eq!(stats.tasks, 1);
    assert_eq!(stats.in_status.len(), 6);
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/stats/time"]["get"]["tags"][0], "stats");
}
