//! Integration tests for the `spend` stats family (023): What did it spend?

use crate::common;

use ariadne_api::stats::SpendStatsDto;
use ariadne_core::{Seat, SessionStatus, TokenUsage};

use common::{Harness, harness};

const LAUNCH: &str = "01launchonexxxxxxxxxxxxxxx";

/// An ended session of a column's agent on a task of its own, having spent
/// 1000 tokens in, 800 of them cached, and 100 out.
async fn ended_session(h: &Harness) {
    h.git_repo("repo");
    let cast = h.cast().await;
    let session = h
        .session(
            &cast.goal,
            Some(&cast.task),
            Seat::Agent,
            &cast.develop().id,
        )
        .await;
    h.store
        .set_session_launch(&session.id, LAUNCH)
        .await
        .unwrap();
    h.set_status(&session, SessionStatus::Running).await;
    h.reports(&session, "stop").await;
    h.store
        .upsert_session_usage(
            &session.id,
            "transcript",
            TokenUsage {
                input_tokens: 1000,
                cached_input_tokens: 800,
                output_tokens: 100,
            },
        )
        .await
        .unwrap();
    h.ingest_from(&session, LAUNCH, "session_end", serde_json::json!({}))
        .await;
}

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

/// The totals, the buckets and the by-model rows all answer for an ended
/// session: its tokens are summed into the totals, pooled under its model,
/// and fall in the one bucket "now" belongs to.
#[tokio::test]
async fn the_spend_stat_answers_the_totals_the_buckets_and_the_models_for_an_ended_session() {
    let h = harness().await;
    ended_session(&h).await;

    let stats: SpendStatsDto = h.get("/v1/stats/spend").await;

    assert_eq!(stats.totals.sessions, 1);
    assert_eq!(stats.totals.input_tokens, 1000);
    assert_eq!(stats.totals.cached_input_tokens, 800);
    assert_eq!(stats.totals.output_tokens, 100);
    assert!((stats.totals.cached_share - 0.8).abs() < 1e-9, "{stats:?}");

    assert_eq!(stats.by_model.len(), 1, "{stats:?}");
    assert_eq!(stats.by_model[0].model, "stub:test-model");
    assert_eq!(stats.by_model[0].input_tokens, 1000);
    assert_eq!(stats.by_model[0].output_tokens, 100);
    assert!((stats.by_model[0].share - 1.0).abs() < 1e-9, "{stats:?}");

    assert_eq!(stats.buckets.len(), 1, "{stats:?}");
    assert_eq!(stats.buckets[0].input_tokens, 1000);
    assert_eq!(stats.buckets[0].cached_input_tokens, 800);
    assert_eq!(stats.buckets[0].output_tokens, 100);
}
