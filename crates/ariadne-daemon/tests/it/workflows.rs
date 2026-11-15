//! Integration tests for the workflow routes: CRUD, reset, parse, and the
//! fat events each write emits.

use axum::http::StatusCode;

use ariadne_api::stream::DomainEvent;
use ariadne_api::workflows::{ParsedWorkflowDto, WorkflowDto};

use ariadne_api::goals::GoalDto;
use ariadne_api::repositories::RepositoryDto;

use crate::common;
use common::{Harness, delete, get, harness, next_event, post, post_json, put_json};

const DOC: &str = "workflow api-design\n  build[Build]\n    Do the work.\n    skills: coding\n";

/// A goal in `planning`, on a workflow named by the caller, on a freshly
/// registered repository.
async fn goal_on(h: &Harness, workflow: &str) -> GoalDto {
    let repo = h.git_repo("goal-on-workflow");
    let registered: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(), "base_branch": "main"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    h.json(
        post_json(
            "/v1/goals",
            serde_json::json!({"title": "Ship it", "repository_ids": [registered.id],
                               "model": "stub:test-model", "workflow": workflow}),
        ),
        StatusCode::CREATED,
    )
    .await
}

#[tokio::test]
async fn every_route_round_trips_and_emits_one_fat_event_per_write() {
    let h = harness().await;
    let mut rx = h.bus.subscribe();

    let (status, body) = h
        .send(post_json(
            "/v1/workflows",
            serde_json::json!({"name": "api-design", "document": DOC}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let created: WorkflowDto = serde_json::from_slice(&body).unwrap();
    assert_eq!(created.name, "api-design");
    assert!(!created.builtin);
    assert_eq!(created.steps.len(), 1);
    assert_eq!(created.steps[0].id, "build");
    assert_eq!(created.steps[0].skills, vec!["coding".to_string()]);

    let event = next_event(&mut rx, |e| e.event.kind() == "workflow_created").await;
    let DomainEvent::WorkflowCreated(fat) = event.event else {
        unreachable!("matched on kind above");
    };
    // Fat payload: the whole DTO, not just a name to refetch.
    assert_eq!(fat.name, "api-design");
    assert_eq!(fat.steps.len(), 1);

    let listed: Vec<WorkflowDto> = h.get("/v1/workflows").await;
    assert!(listed.iter().any(|w| w.name == "api-design"));

    let fetched: WorkflowDto = h.get("/v1/workflows/api-design").await;
    assert_eq!(fetched.document, DOC);

    let edited =
        "workflow api-design\n  build[Build]\n    Do it differently.\n    skills: coding\n";
    let updated: WorkflowDto = h
        .json(
            put_json(
                "/v1/workflows/api-design",
                serde_json::json!({"document": edited}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(updated.steps[0].description, "Do it differently.");
    let event = next_event(&mut rx, |e| e.event.kind() == "workflow_updated").await;
    assert!(matches!(event.event, DomainEvent::WorkflowUpdated(_)));

    let (status, _) = h.send(delete("/v1/workflows/api-design")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let event = next_event(&mut rx, |e| e.event.kind() == "workflow_deleted").await;
    assert!(matches!(event.event, DomainEvent::WorkflowDeleted(_)));

    let (status, _) = h.send(get("/v1/workflows/api-design")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_missing_workflow_is_a_404_naming_workflow_not_found() {
    let h = harness().await;
    let err = h
        .error(get("/v1/workflows/nope"), StatusCode::NOT_FOUND)
        .await;
    assert_eq!(err.error.code, "workflow_not_found");
}

#[tokio::test]
async fn a_taken_name_is_a_409_and_a_built_in_refuses_delete_and_a_user_one_refuses_reset() {
    let h = harness().await;

    let (status, _) = h
        .send(post_json(
            "/v1/workflows",
            serde_json::json!({
                "name": "develop-review-merge",
                "document": "workflow develop-review-merge\n  a[A]\n    Do it.\n    skills: coding\n",
            }),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = h.send(delete("/v1/workflows/develop-review-merge")).await;
    assert_eq!(status, StatusCode::CONFLICT);

    h.send(post_json(
        "/v1/workflows",
        serde_json::json!({"name": "api-design", "document": DOC}),
    ))
    .await;
    let (status, _) = h
        .send(post_json(
            "/v1/workflows/api-design/reset",
            serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// A goal naming a workflow blocks its delete with `workflow_in_use` while
/// the goal is running, and stops blocking it once the goal ends: nothing
/// more will ever staff from it.
#[tokio::test]
async fn a_workflow_is_freed_for_deletion_once_the_goal_naming_it_ends() {
    let h = harness().await;
    h.send(post_json(
        "/v1/workflows",
        serde_json::json!({"name": "api-design", "document": DOC}),
    ))
    .await;
    let goal = goal_on(&h, "api-design").await;

    let err = h
        .error(delete("/v1/workflows/api-design"), StatusCode::CONFLICT)
        .await;
    assert_eq!(err.error.code, "workflow_in_use");

    let _goal: GoalDto = h
        .json(
            post(&format!("/v1/goals/{}/cancel", goal.id)),
            StatusCode::OK,
        )
        .await;

    let (status, _) = h.send(delete("/v1/workflows/api-design")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn resetting_a_built_in_drops_the_override() {
    let h = harness().await;
    h.send(put_json(
        "/v1/workflows/develop-review-merge",
        serde_json::json!({"document": DOC}),
    ))
    .await;

    let (status, body) = h
        .send(post_json(
            "/v1/workflows/develop-review-merge/reset",
            serde_json::json!({}),
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let reset: WorkflowDto = serde_json::from_slice(&body).unwrap();
    assert_eq!(reset.steps[0].id, "develop");
}

#[tokio::test]
async fn the_parse_route_answers_the_steps_of_a_good_document_and_the_line_of_a_bad_one() {
    let h = harness().await;

    let parsed: ParsedWorkflowDto = h
        .json(
            post_json("/v1/workflows/parse", serde_json::json!({"document": DOC})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(parsed.name, "api-design");
    assert_eq!(parsed.steps[0].id, "build");

    let err = h
        .error(
            post_json(
                "/v1/workflows/parse",
                serde_json::json!({"document": "not a workflow\n"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(err.error.code, "workflow_invalid");
    assert_eq!(err.error.details.unwrap()["line"], 1);
}

/// The shipped `pr-review` workflow shows in the catalog with its operation,
/// is edited within its shape, and is refused to ordinary goals and
/// repository defaults.
#[tokio::test]
async fn the_pr_review_workflow_is_listed_edited_in_its_shape_and_closed_to_ordinary_goals() {
    let h = harness().await;
    let listed: Vec<WorkflowDto> = h.get("/v1/workflows").await;
    let review = listed
        .iter()
        .find(|w| w.name == "pr-review")
        .expect("the catalog lists pr-review");
    assert_eq!(
        review.operation,
        Some(ariadne_core::workflow::PrOperation::Review)
    );
    assert!(review.builtin);
    assert_eq!(review.steps.len(), 1);
    assert_eq!(review.steps[0].id, "review");
    assert_eq!(review.steps[0].skills, ["pr-reviewer"]);
    assert!(
        listed
            .iter()
            .filter(|w| w.name != "pr-review" && w.name != "pr-babysit")
            .all(|w| w.operation.is_none())
    );

    let (status, _) = h
        .send(put_json(
            "/v1/workflows/pr-review",
            serde_json::json!({"document":
                "workflow pr-review\n  review[Review]\n    Look.\n    skills: coding\n    ranks: balanced\n"}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a column without pr-reviewer"
    );
    let edited: WorkflowDto = h
        .json(
            put_json(
                "/v1/workflows/pr-review",
                serde_json::json!({"document":
                    "workflow pr-review\n  review[Look]\n    Look hard.\n    skills: pr-reviewer, research\n    ranks: frontier\n    gate: request-merged\n"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.steps[0].title, "Look");

    let repo = h.git_repo("pr-review-default");
    let registered: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(), "base_branch": "main"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let (status, _) = h
        .send(post_json(
            "/v1/goals",
            serde_json::json!({"title": "Ship it", "repository_ids": [registered.id],
                               "model": "stub:test-model", "workflow": "pr-review"}),
        ))
        .await;
    assert!(status.is_client_error(), "{status}");
    let (status, _) = h
        .send(put_json(
            &format!("/v1/repositories/{}", registered.id),
            serde_json::json!({"default_workflow": "pr-review"}),
        ))
        .await;
    assert!(status.is_client_error(), "{status}");
}

/// The shipped `pr-babysit` workflow shows in the catalog with its operation,
/// is edited within its shape, and is refused to ordinary goals and
/// repository defaults.
#[tokio::test]
async fn the_pr_babysit_workflow_is_listed_edited_in_its_shape_and_closed_to_ordinary_goals() {
    let h = harness().await;
    let listed: Vec<WorkflowDto> = h.get("/v1/workflows").await;
    let babysit = listed
        .iter()
        .find(|w| w.name == "pr-babysit")
        .expect("the catalog lists pr-babysit");
    assert_eq!(
        babysit.operation,
        Some(ariadne_core::workflow::PrOperation::Babysit)
    );
    assert!(babysit.builtin);
    assert_eq!(babysit.steps.len(), 1);
    assert_eq!(babysit.steps[0].id, "babysit");
    assert_eq!(babysit.steps[0].skills, ["pr-babysit"]);
    assert_eq!(
        babysit.steps[0].ranks,
        [
            ariadne_core::models::ModelRank::Balanced,
            ariadne_core::models::ModelRank::Frontier
        ]
    );

    for document in [
        "workflow pr-babysit\n  babysit[Babysit]\n    Keep.\n    skills: coding\n    ranks: balanced\n",
        "workflow pr-babysit\n  babysit[Babysit]\n    Keep.\n    skills: pr-babysit\n    ranks: fast\n",
        "workflow pr-babysit\n  babysit[Babysit]\n    Keep.\n    skills: pr-babysit\n    ranks: balanced\n",
    ] {
        let (status, _) = h
            .send(put_json(
                "/v1/workflows/pr-babysit",
                serde_json::json!({ "document": document }),
            ))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{document}");
    }
    let edited: WorkflowDto = h
        .json(
            put_json(
                "/v1/workflows/pr-babysit",
                serde_json::json!({"document":
                    "workflow pr-babysit\n  babysit[Keep]\n    Keep it.\n    skills: pr-babysit, research\n    ranks: balanced\n    gate: request-merged\n"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.steps[0].title, "Keep");

    let repo = h.git_repo("pr-babysit-default");
    let registered: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(), "base_branch": "main"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let (status, _) = h
        .send(post_json(
            "/v1/goals",
            serde_json::json!({"title": "Ship it", "repository_ids": [registered.id],
                               "model": "stub:test-model", "workflow": "pr-babysit"}),
        ))
        .await;
    assert!(status.is_client_error(), "{status}");
    let (status, _) = h
        .send(put_json(
            &format!("/v1/repositories/{}", registered.id),
            serde_json::json!({"default_workflow": "pr-babysit"}),
        ))
        .await;
    assert!(status.is_client_error(), "{status}");
}
