//! Integration tests for the `time` stats family (023): How long does it take?

use crate::common;

use ariadne_api::stats::TimeStatsDto;

use common::harness;

/// `GET /v1/stats/time` answers 200 with the family's DTO, and the route is
/// in the API document under the `stats` tag.
#[tokio::test]
async fn the_time_stat_answers_and_is_in_the_api_document() {
    let h = harness().await;
    let stats: TimeStatsDto = h.get("/v1/stats/time").await;
    assert_eq!(stats, TimeStatsDto::default());
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/stats/time"]["get"]["tags"][0], "stats");
}
