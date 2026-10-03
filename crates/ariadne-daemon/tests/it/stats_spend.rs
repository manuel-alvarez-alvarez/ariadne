//! Integration tests for the `spend` stats family (023): What did it spend?

use crate::common;

use ariadne_api::stats::SpendStatsDto;

use common::harness;

/// `GET /v1/stats/spend` answers 200 with the family's DTO, and the route is
/// in the API document under the `stats` tag.
#[tokio::test]
async fn the_spend_stat_answers_and_is_in_the_api_document() {
    let h = harness().await;
    let stats: SpendStatsDto = h.get("/v1/stats/spend").await;
    assert_eq!(stats, SpendStatsDto::default());
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/stats/spend"]["get"]["tags"][0], "stats");
}
