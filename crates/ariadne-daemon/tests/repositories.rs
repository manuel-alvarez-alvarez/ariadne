//! Integration tests for the repository endpoints.
//!
//! The contract is that a repository is validated exactly the way a goal's
//! repos are — an absolute path into a real git work tree, on a branch that
//! exists and has a commit — that the same checkout is registered once per
//! base branch, that every write reaches the domain-event stream, and that
//! the landing briefing it hands its engineer is its own: prefilled from its
//! merge strategy, editable, and reset by writing an empty one.

mod common;

use axum::http::StatusCode;

use ariadne_api::repositories::{MergeStrategyDto, RepositoryDto};
use ariadne_api::stream::DomainEvent;
use ariadne_core::MergeStrategy;
use ariadne_store::defaults::default_landing_prompt;

use common::{delete, get, harness, next_event, post_json, put_json, sh};

#[tokio::test]
async fn crud_round_trip_emits_a_fat_event_per_write() {
    let h = harness().await;
    let repo = h.git_repo("repo");
    let mut rx = h.bus.subscribe();

    let created: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(), "base_branch": "main",
                                   "description": "the toy repo"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created.path, repo.display().to_string());
    assert_eq!(created.base_branch, "main");
    assert_eq!(created.description.as_deref(), Some("the toy repo"));

    // Fat payload: the whole DTO, not just an id to refetch. Repositories are
    // global, so the event belongs to no goal or task.
    let event = next_event(&mut rx, |e| e.event.kind() == "repository_created").await;
    assert_eq!(event.goal_id, None);
    assert_eq!(event.task_id, None);
    let DomainEvent::RepositoryCreated(dto) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(dto.id, created.id);
    assert_eq!(dto.base_branch, "main");

    // Listed and readable by id.
    let all: Vec<RepositoryDto> = h.json(get("/v1/repositories"), StatusCode::OK).await;
    assert_eq!(all.len(), 1);
    let one: RepositoryDto = h
        .json(
            get(&format!("/v1/repositories/{}", created.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(one.id, created.id);

    // Moving the base branch revalidates it; an empty description clears it.
    let edited: RepositoryDto = h
        .json(
            put_json(
                &format!("/v1/repositories/{}", created.id),
                serde_json::json!({"base_branch": "next", "description": ""}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.path, created.path, "the path is left alone");
    assert_eq!(edited.base_branch, "next");
    assert!(edited.description.is_none());
    let event = next_event(&mut rx, |e| e.event.kind() == "repository_updated").await;
    let DomainEvent::RepositoryUpdated(dto) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(dto.base_branch, "next");

    let (status, _) = h
        .send(delete(&format!("/v1/repositories/{}", created.id)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let event = next_event(&mut rx, |e| e.event.kind() == "repository_deleted").await;
    let DomainEvent::RepositoryDeleted(gone) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(gone.id, created.id);

    h.error(
        get(&format!("/v1/repositories/{}", created.id)),
        StatusCode::NOT_FOUND,
    )
    .await;
}

/// Omitting the base branch takes the repo's checked-out one.
#[tokio::test]
async fn base_branch_defaults_to_the_current_branch() {
    let h = harness().await;
    let repo = h.git_repo("repo");
    sh(&repo, "git checkout -q next");

    let created: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string()}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created.base_branch, "next");
    assert!(created.description.is_none());
}

#[tokio::test]
async fn create_refuses_a_path_or_branch_it_cannot_use() {
    let h = harness().await;
    let repo = h.git_repo("repo");

    // Relative path.
    let err = h
        .error(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": "relative/repo"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("must be absolute"),
        "{}",
        err.error.message
    );

    // Absolute, but not a git work tree.
    let not_a_repo = h.dir.path().join("empty");
    std::fs::create_dir_all(&not_a_repo).unwrap();
    h.error(
        post_json(
            "/v1/repositories",
            serde_json::json!({"path": not_a_repo.display().to_string()}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;

    // A branch the repo does not have.
    let err = h
        .error(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(), "base_branch": "nope"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("nope"),
        "the refusal names the branch: {}",
        err.error.message
    );
}

/// One registration per checkout and base branch; another branch of the same
/// checkout is a repository of its own.
#[tokio::test]
async fn the_same_path_and_branch_cannot_be_registered_twice() {
    let h = harness().await;
    let repo = h.git_repo("repo");
    let body = serde_json::json!({"path": repo.display().to_string(), "base_branch": "main"});

    let first: RepositoryDto = h
        .json(
            post_json("/v1/repositories", body.clone()),
            StatusCode::CREATED,
        )
        .await;
    h.error(post_json("/v1/repositories", body), StatusCode::CONFLICT)
        .await;

    let other: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(), "base_branch": "next"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_ne!(other.id, first.id);

    // Editing one onto the other's (path, base_branch) conflicts the same way.
    h.error(
        put_json(
            &format!("/v1/repositories/{}", other.id),
            serde_json::json!({"base_branch": "main"}),
        ),
        StatusCode::CONFLICT,
    )
    .await;
}

/// The landing briefing of a new repository is the default of its merge
/// strategy, and a text given at creation is what stands instead.
#[tokio::test]
async fn a_new_repository_lands_by_its_strategys_briefing_unless_it_was_given_one() {
    let h = harness().await;
    let repo = h.git_repo("repo");

    let created: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(),
                                   "merge_strategy": "pull_request"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(
        created.landing_prompt,
        default_landing_prompt(MergeStrategy::PullRequest)
    );
    assert!(created.landing_prompt_is_default);

    let mine = "Squash {branch} onto {base_branch} in {repo_path}.";
    let own: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(),
                                   "base_branch": "next", "landing_prompt": mine}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(own.landing_prompt, mine);
    assert!(!own.landing_prompt_is_default);
}

/// An update sets the landing briefing, an empty one puts the strategy's
/// default back, and a strategy that moves under a text of the repository's
/// own leaves those words exactly where they are.
#[tokio::test]
async fn the_landing_briefing_is_set_reset_and_kept_across_a_strategy_change() {
    let h = harness().await;
    let repo = h.git_repo("repo");
    let created: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string()}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let uri = format!("/v1/repositories/{}", created.id);

    let mine = "Ship {task_title} from {repo_path}.";
    let set: RepositoryDto = h
        .json(
            put_json(&uri, serde_json::json!({"landing_prompt": mine})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(set.landing_prompt, mine);
    assert!(!set.landing_prompt_is_default);

    // An update that says nothing about it leaves it, and so does one that
    // moves the repository onto the other strategy.
    let described: RepositoryDto = h
        .json(
            put_json(&uri, serde_json::json!({"description": "the toy repo"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(described.landing_prompt, mine);
    let moved: RepositoryDto = h
        .json(
            put_json(&uri, serde_json::json!({"merge_strategy": "pull_request"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(moved.merge_strategy, MergeStrategy::PullRequest);
    assert_eq!(moved.landing_prompt, mine, "the words are the user's");

    // Empty resets, and what stands is the default of the strategy in force.
    let reset: RepositoryDto = h
        .json(
            put_json(&uri, serde_json::json!({"landing_prompt": ""})),
            StatusCode::OK,
        )
        .await;
    assert!(reset.landing_prompt_is_default);
    assert_eq!(
        reset.landing_prompt,
        default_landing_prompt(MergeStrategy::PullRequest)
    );
}

/// A landing briefing naming a placeholder nothing fills in is a 400 that
/// says which token and what it could have used instead — on both writes.
#[tokio::test]
async fn a_landing_briefing_with_an_unknown_placeholder_is_a_400() {
    let h = harness().await;
    let repo = h.git_repo("repo");
    let broken = "Land {branch} the {nope} way.";

    let err = h
        .error(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string(),
                                   "landing_prompt": broken}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(err.error.code, "invalid_request");
    for named in [
        "{nope}",
        "{task_title}",
        "{branch}",
        "{base_branch}",
        "{repo_path}",
    ] {
        assert!(err.error.message.contains(named), "{}", err.error.message);
    }

    let created: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": repo.display().to_string()}),
            ),
            StatusCode::CREATED,
        )
        .await;
    h.error(
        put_json(
            &format!("/v1/repositories/{}", created.id),
            serde_json::json!({"landing_prompt": broken}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    // ...and nothing was written.
    let unchanged: RepositoryDto = h
        .json(
            get(&format!("/v1/repositories/{}", created.id)),
            StatusCode::OK,
        )
        .await;
    assert!(unchanged.landing_prompt_is_default);
}

/// Every merge strategy and the landing briefing it prefills, so a client can
/// show one before the repository exists — and the endpoint is in the OpenAPI
/// document, since that is what the clients are generated from.
#[tokio::test]
async fn the_merge_strategies_are_listed_with_their_landing_briefings() {
    let h = harness().await;
    let strategies: Vec<MergeStrategyDto> =
        h.json(get("/v1/merge-strategies"), StatusCode::OK).await;
    assert_eq!(
        strategies
            .iter()
            .map(|s| s.merge_strategy)
            .collect::<Vec<_>>(),
        MergeStrategy::ALL
    );
    for listed in &strategies {
        assert_eq!(
            listed.landing_prompt,
            default_landing_prompt(listed.merge_strategy)
        );
    }

    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/merge-strategies"]["get"].is_object());
    assert!(doc["components"]["schemas"]["MergeStrategyDto"].is_object());
}
