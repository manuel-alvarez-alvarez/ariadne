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

/// Each model and seat answers its tasks, goals, tokens, time and messages,
/// authors their rounds per task and reviewers their changes per task, under
/// both filters.
#[tokio::test]
async fn each_model_and_seat_answers_its_figures_under_both_filters() {
    use ariadne_api::stats::ModelStatDto;
    use ariadne_store::NewStatFact;
    use serde_json::{Value, json};

    let h = harness().await;
    let fact = |kind: &str, model: &str, seat: &str, task: &str, data: Value| NewStatFact {
        kind: kind.into(),
        repo_id: Some("repo".into()),
        goal_id: Some("goal".into()),
        task_id: Some(task.into()),
        session_id: None,
        launch_id: None,
        seat: Some(seat.into()),
        model: Some(model.into()),
        effort: None,
        skills: vec![],
        data,
    };
    let facts = [
        fact(
            "session_ended",
            "writer",
            "author",
            "one",
            json!({"input_tokens": 1_000, "cached_input_tokens": 600, "output_tokens": 200,
                "lifetime_secs": 300}),
        ),
        fact(
            "session_ended",
            "writer",
            "author",
            "two",
            json!({"input_tokens": 500, "output_tokens": 100, "lifetime_secs": 120}),
        ),
        fact(
            "message",
            "writer",
            "author",
            "one",
            json!({"kind": "review_request"}),
        ),
        fact(
            "task_ended",
            "writer",
            "author",
            "one",
            json!({"status": "finished", "review_requests": 3}),
        ),
        fact(
            "task_ended",
            "writer",
            "author",
            "two",
            json!({"status": "finished", "review_requests": 1}),
        ),
        fact(
            "verdict",
            "judge",
            "reviewer",
            "one",
            json!({"verdict": "changes_requested", "author_model": "writer"}),
        ),
        fact(
            "verdict",
            "judge",
            "reviewer",
            "one",
            json!({"verdict": "approve", "author_model": "writer"}),
        ),
        fact(
            "verdict",
            "judge",
            "reviewer",
            "two",
            json!({"verdict": "approve", "author_model": "writer"}),
        ),
    ];
    for fact in facts {
        h.store.record_fact(fact).await.unwrap();
    }

    let stats: ModelStatsDto = h
        .get("/v1/stats/models?since=2000-01-01T00:00:00Z&repo=repo")
        .await;
    assert_eq!(
        stats.items,
        vec![
            ModelStatDto {
                model: "writer".into(),
                seat: Some("author".into()),
                tasks: 2,
                goals: 1,
                tokens: 1_800,
                time_secs: 420.0,
                messages: 1,
                rounds_per_task: Some(2.0),
                changes_per_task: None,
            },
            ModelStatDto {
                model: "judge".into(),
                seat: Some("reviewer".into()),
                rounds_per_task: None,
                changes_per_task: Some(0.5),
                ..ModelStatDto::default()
            },
        ]
    );
    let json: Value = h.get("/v1/stats/models?repo=repo").await;
    let mut keys: Vec<_> = json["items"][1]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "changes_per_task",
            "goals",
            "messages",
            "model",
            "rounds_per_task",
            "seat",
            "tasks",
            "time_secs",
            "tokens"
        ]
    );
    assert_eq!(json["items"][1]["rounds_per_task"], Value::Null);
    let excluded: ModelStatsDto = h.get("/v1/stats/models?repo=elsewhere").await;
    assert!(excluded.items.is_empty());
}
