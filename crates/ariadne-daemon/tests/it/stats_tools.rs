//! Integration tests for the `tools` stats family (023): What do the agents do?

use crate::common;

use ariadne_api::stats::ToolStatsDto;

use common::harness;

/// `GET /v1/stats/tools` answers 200 with the family's DTO, and the route is
/// in the API document under the `stats` tag.
#[tokio::test]
async fn the_tools_stat_answers_and_is_in_the_api_document() {
    let h = harness().await;
    let stats: ToolStatsDto = h.get("/v1/stats/tools").await;
    assert_eq!(stats, ToolStatsDto::default());
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/stats/tools"]["get"]["tags"][0], "stats");
}
