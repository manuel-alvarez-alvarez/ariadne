//! Integration tests for the `attention` stats family (023): How much did it need me?

use crate::common;

use ariadne_api::stats::AttentionStatsDto;

use common::harness;

/// `GET /v1/stats/attention` answers 200 with the family's DTO, and the route is
/// in the API document under the `stats` tag.
#[tokio::test]
async fn the_attention_stat_answers_and_is_in_the_api_document() {
    let h = harness().await;
    let stats: AttentionStatsDto = h.get("/v1/stats/attention").await;
    assert_eq!(stats, AttentionStatsDto::default());
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(
        doc["paths"]["/v1/stats/attention"]["get"]["tags"][0],
        "stats"
    );
}
