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

/// A finished task and its approval reach the HTTP response under both filters.
#[tokio::test]
async fn a_finished_task_and_approval_answer_author_and_reviewer_rows() {
    use ariadne_core::{Actor, TaskStatus};
    use ariadne_store::NewStatFact;
    use serde_json::json;

    let h = harness().await;
    let cast = h.cast().await;
    for (status, actor) in [
        (TaskStatus::Ready, Actor::Daemon),
        (TaskStatus::InProgress, Actor::Daemon),
        (TaskStatus::UnderReview, Actor::Author),
        (TaskStatus::Approved, Actor::Daemon),
        (TaskStatus::Finished, Actor::Author),
    ] {
        h.store
            .transition_task(&cast.task.id, status, actor, None, Some("abc123"))
            .await
            .unwrap();
    }
    let repo = cast.repo.id.clone();
    h.store.record_fact(NewStatFact {
        kind: "verdict".into(), repo_id: Some(repo.clone()), goal_id: None,
        task_id: Some(cast.task.id.clone()), session_id: None, launch_id: None,
        seat: Some("reviewer".into()), model: Some("judge".into()), effort: None, skills: vec![],
        data: json!({"verdict":"approve", "author_model":"stub:test-model", "round":1, "latency_secs":12}),
    }).await.unwrap();
    let stats: ModelStatsDto = h
        .get(&format!(
            "/v1/stats/models?since=2000-01-01T00:00:00Z&repo={repo}"
        ))
        .await;
    assert_eq!(stats.items.len(), 2);
    let author = stats.items[0].author.as_ref().unwrap();
    assert_eq!(stats.items[0].model, "stub:test-model");
    assert_eq!(
        (
            author.tasks_finished,
            author.finish_rate,
            author.first_pass_rate
        ),
        (1, 1.0, 1.0)
    );
    let reviewer = stats.items[1].reviewer.as_ref().unwrap();
    assert_eq!(stats.items[1].model, "judge");
    assert_eq!(
        (
            reviewer.verdicts,
            reviewer.approve_share,
            reviewer.mean_latency_secs
        ),
        (1, 1.0, 12.0)
    );
    let excluded: ModelStatsDto = h.get("/v1/stats/models?repo=elsewhere").await;
    assert!(excluded.items.is_empty());
}
