//! Integration tests for the `models` stats family (023): Which model does the job?

use crate::common;

use ariadne_api::stats::ModelStatsDto;
use ariadne_core::{Actor, TaskStatus};

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
/// and the tasks its column ended `finished`, under both filters.
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
            "agent",
            "one",
            json!({"input_tokens": 1_000, "cached_input_tokens": 600, "output_tokens": 200,
                "lifetime_secs": 300}),
        ),
        fact(
            "session_ended",
            "writer",
            "agent",
            "two",
            json!({"input_tokens": 500, "output_tokens": 100, "lifetime_secs": 120}),
        ),
        fact(
            "message",
            "writer",
            "agent",
            "one",
            json!({"kind": "message"}),
        ),
        fact(
            "task_ended",
            "writer",
            "agent",
            "one",
            json!({"status": "finished", "landed": true, "step": "merge"}),
        ),
        fact(
            "task_ended",
            "writer",
            "agent",
            "two",
            json!({"status": "failed", "landed": false, "step": "develop"}),
        ),
        fact(
            "message",
            "planner",
            "orchestrator",
            "one",
            json!({"kind": "message"}),
        ),
    ];
    for fact in facts {
        h.store.record_fact(fact).await.unwrap();
    }

    let stats: ModelStatsDto = h
        .get("/v1/stats/models?since=2000-01-01T00:00:00Z&repo=repo")
        .await;
    // The orchestrator's rows come first, then the agents', then the rows
    // of no seat.
    assert_eq!(
        stats.items,
        vec![
            ModelStatDto {
                model: "planner".into(),
                seat: Some("orchestrator".into()),
                messages: 1,
                ..ModelStatDto::default()
            },
            ModelStatDto {
                model: "writer".into(),
                seat: Some("agent".into()),
                tasks: 2,
                goals: 1,
                tokens: 1_800,
                time_secs: 420.0,
                messages: 1,
                tasks_finished: 1,
            },
        ]
    );
    let json: Value = h.get("/v1/stats/models?repo=repo").await;
    let mut keys: Vec<_> = json["items"][0]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "goals",
            "messages",
            "model",
            "seat",
            "tasks",
            "tasks_finished",
            "time_secs",
            "tokens"
        ]
    );
    let excluded: ModelStatsDto = h.get("/v1/stats/models?repo=elsewhere").await;
    assert!(excluded.items.is_empty());
}

/// A real task finished in its column, the fact the daemon writes for it,
/// counts in `tasks_finished` under the model of the agent of that column.
#[tokio::test]
async fn a_task_finished_in_its_column_lifts_its_agents_tasks_finished() {
    let h = harness().await;
    let cast = h.cast_pinned("stub:finisher").await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Finished,
            Actor::Agent,
            Some("Done."),
            Some("abc123"),
        )
        .await
        .unwrap();

    let stats: ModelStatsDto = h.get("/v1/stats/models").await;
    let agent_row = stats
        .items
        .iter()
        .find(|r| r.seat.as_deref() == Some("agent"))
        .unwrap_or_else(|| panic!("no agent row: {stats:?}"));
    assert_eq!(agent_row.model, "stub:finisher");
    assert_eq!(agent_row.tasks_finished, 1);
    assert!(
        stats
            .items
            .iter()
            .all(|r| r.seat.as_deref() != Some("author")),
        "no author seat is left: {stats:?}"
    );
}
