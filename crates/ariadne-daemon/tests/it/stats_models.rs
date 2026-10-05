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

/// Interventions, session time and the author's lead time and rate reach the
/// HTTP response per model and seat. A `waiting_permission` attention fact and
/// a permission an AI decided count for nothing.
#[tokio::test]
async fn interventions_and_time_answer_per_model_and_seat() {
    use ariadne_api::stats::ModelInterventionsDto;
    use ariadne_store::NewStatFact;
    use serde_json::{Value, json};

    let h = harness().await;
    let fact = |kind: &str, model: &str, seat: &str, data: Value| NewStatFact {
        kind: kind.into(),
        repo_id: Some("repo".into()),
        goal_id: None,
        task_id: None,
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
            "permission",
            "writer",
            "author",
            json!({"decided_by": "console", "wait_ms": 12_000}),
        ),
        fact(
            "permission",
            "writer",
            "author",
            json!({"decided_by": "ai", "wait_ms": 90_000}),
        ),
        fact(
            "attention",
            "writer",
            "author",
            json!({"reason": "waiting_input", "wait_secs": 30}),
        ),
        fact(
            "attention",
            "writer",
            "author",
            json!({"reason": "waiting_user", "wait_secs": 40}),
        ),
        fact(
            "attention",
            "writer",
            "author",
            json!({"reason": "stalled", "wait_secs": 50}),
        ),
        fact(
            "attention",
            "writer",
            "author",
            json!({"reason": "agent_error", "wait_secs": 60}),
        ),
        fact(
            "attention",
            "writer",
            "author",
            json!({"reason": "waiting_permission", "wait_secs": 999}),
        ),
        fact(
            "task_ended",
            "writer",
            "author",
            json!({"status": "finished", "lead_time_secs": 600}),
        ),
        fact(
            "task_ended",
            "writer",
            "author",
            json!({"status": "finished", "lead_time_secs": 200}),
        ),
        fact(
            "task_ended",
            "writer",
            "author",
            json!({"status": "cancelled", "lead_time_secs": 5}),
        ),
        fact(
            "session_ended",
            "writer",
            "author",
            json!({"lifetime_secs": 1_000}),
        ),
        fact(
            "session_ended",
            "writer",
            "author",
            json!({"lifetime_secs": 500}),
        ),
        fact(
            "permission",
            "judge",
            "reviewer",
            json!({"decided_by": "console", "wait_ms": 3_000}),
        ),
        fact(
            "attention",
            "judge",
            "reviewer",
            json!({"reason": "waiting_permission", "wait_secs": 999}),
        ),
        fact(
            "session_ended",
            "judge",
            "reviewer",
            json!({"lifetime_secs": 90}),
        ),
    ];
    for fact in facts {
        h.store.record_fact(fact).await.unwrap();
    }

    let stats: ModelStatsDto = h.get("/v1/stats/models?repo=repo").await;
    assert_eq!(stats.items.len(), 2);
    let writer = &stats.items[0];
    assert_eq!(
        (writer.model.as_str(), writer.seat.as_deref()),
        ("writer", Some("author"))
    );
    assert_eq!(
        writer.interventions,
        ModelInterventionsDto {
            permissions: 1,
            questions: 2,
            stalls: 2,
            total: 5,
            person_secs: 192.0
        }
    );
    assert_eq!(writer.total_lifetime_secs, 1_500.0);
    let author = writer.author.as_ref().unwrap();
    assert_eq!(author.median_lead_time_secs, 400.0);
    assert_eq!(author.interventions_per_finished_task, Some(2.5));

    let judge = &stats.items[1];
    assert_eq!(
        (judge.model.as_str(), judge.seat.as_deref()),
        ("judge", Some("reviewer"))
    );
    assert_eq!(
        judge.interventions,
        ModelInterventionsDto {
            permissions: 1,
            questions: 0,
            stalls: 0,
            total: 1,
            person_secs: 3.0
        }
    );
    assert_eq!(judge.total_lifetime_secs, 90.0);
    assert!(judge.author.is_none());
    let json: Value = h.get("/v1/stats/models?repo=repo").await;
    assert!(
        json["items"][1]
            .get("interventions_per_finished_task")
            .is_none()
    );
    assert_eq!(json["items"][0]["interventions"]["person_secs"], 192.0);
}
