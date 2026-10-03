//! Integration tests for the `models` stats family (023): Which model does the job?

use crate::common;

use ariadne_api::stats::ModelStatsDto;

use common::harness;

/// `GET /v1/stats/models` answers 200 with the family's DTO, and the route is
/// in the API document under the `stats` tag.
#[tokio::test]
async fn the_models_stat_answers_and_is_in_the_api_document() {
    let h = harness().await;
    let stats: ModelStatsDto = h.get("/v1/stats/models").await;
    assert_eq!(stats, ModelStatsDto::default());
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/stats/models"]["get"]["tags"][0], "stats");
}
