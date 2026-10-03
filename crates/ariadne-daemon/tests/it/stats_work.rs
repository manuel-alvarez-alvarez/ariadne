//! Integration tests for the `work` stats family (023): What got done?

use crate::common;

use ariadne_api::stats::WorkStatsDto;

use common::harness;

/// `GET /v1/stats/work` answers 200 with the family's DTO, and the route is
/// in the API document under the `stats` tag.
#[tokio::test]
async fn the_work_stat_answers_and_is_in_the_api_document() {
    let h = harness().await;
    let stats: WorkStatsDto = h.get("/v1/stats/work").await;
    assert_eq!(stats, WorkStatsDto::default());
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/stats/work"]["get"]["tags"][0], "stats");
}
