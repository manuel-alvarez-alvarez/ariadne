//! Integration tests for the per-profile prompt endpoints.
//!
//! The contract is that every prompt a profile owns is listable, editable with
//! any text at all, and restorable to the exact Rust constant it was seeded
//! from — and that a kind belonging to another role is refused with a sentence,
//! not a 500.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

use ariadne_api::error::ErrorBody;
use ariadne_api::profiles::{ProfileDto, ProfilePromptDto};
use ariadne_core::{AgentKind, PromptKind, Role};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::defaults::{default_prompt, default_system_prompt};
use ariadne_store::{NewProfile, Profile, Store};

struct Harness {
    store: Store,
    router: Router,
    #[allow(dead_code)]
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: TmuxManager::default(),
        git: GitManager,
    });
    let state = AppState {
        store: store.clone(),
        started_at: Instant::now(),
        launcher,
        sched_tx: None,
        events: bus,
        logs: LogBuffer::new(),
    };
    Harness {
        router: http::router(state),
        store,
        dir,
    }
}

impl Harness {
    async fn profile(&self, name: &str, role: Role) -> Profile {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: format!("You are {name}."),
                extra_flags: vec![],
            })
            .await
            .unwrap()
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    /// Send a request expected to succeed and decode its JSON body.
    async fn json<T: DeserializeOwned>(&self, request: Request<Body>) -> T {
        let (status, body) = self.send(request).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// Send a request expected to fail and decode the error envelope.
    async fn error(&self, request: Request<Body>, expected: StatusCode) -> ErrorBody {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn post(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn put_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::PUT)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn a_profile_lists_the_prompts_of_its_role_in_briefing_order() {
    let h = harness().await;
    let engineer = h.profile("eng", Role::Engineer).await;

    let prompts: Vec<ProfilePromptDto> = h
        .json(get(&format!("/v1/profiles/{}/prompts", engineer.id)))
        .await;

    assert_eq!(
        prompts.iter().map(|p| p.kind).collect::<Vec<_>>(),
        PromptKind::for_role(Role::Engineer).to_vec()
    );
    for prompt in &prompts {
        assert_eq!(
            prompt.content,
            default_prompt(Role::Engineer, prompt.kind).unwrap()
        );
        assert!(!prompt.updated_at.is_empty());
    }
}

/// A planner has one prompt, and it is not the engineer's.
#[tokio::test]
async fn a_planner_lists_only_its_own_prompt() {
    let h = harness().await;
    let planner = h.profile("plan", Role::Planner).await;

    let prompts: Vec<ProfilePromptDto> = h
        .json(get(&format!("/v1/profiles/{}/prompts", planner.id)))
        .await;

    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].kind, PromptKind::PlannerBriefing);
}

/// Any text is accepted: breaking a `{placeholder}` is the editor's business,
/// not the API's.
#[tokio::test]
async fn a_prompt_update_takes_any_content_and_a_reset_restores_the_default() {
    let h = harness().await;
    let reviewer = h.profile("rev", Role::Reviewer).await;
    let uri = format!("/v1/profiles/{}/prompts/reviewer_briefing", reviewer.id);

    let updated: ProfilePromptDto = h
        .json(put_json(
            &uri,
            serde_json::json!({ "content": "Review {nothing_that_exists}." }),
        ))
        .await;
    assert_eq!(updated.kind, PromptKind::ReviewerBriefing);
    assert_eq!(updated.content, "Review {nothing_that_exists}.");

    // The edit is what a later read sees.
    let prompts: Vec<ProfilePromptDto> = h
        .json(get(&format!("/v1/profiles/{}/prompts", reviewer.id)))
        .await;
    assert_eq!(prompts[0].content, "Review {nothing_that_exists}.");

    let reset: ProfilePromptDto = h.json(post(&format!("{uri}/reset"))).await;
    assert_eq!(
        reset.content,
        default_prompt(Role::Reviewer, PromptKind::ReviewerBriefing).unwrap()
    );
}

/// Editing a prompt kind of another role is a 400 naming the roles involved,
/// on both the update and the reset route.
#[tokio::test]
async fn a_kind_of_another_role_is_refused_with_a_sentence() {
    let h = harness().await;
    let planner = h.profile("plan", Role::Planner).await;
    let uri = format!("/v1/profiles/{}/prompts/merge_instructions", planner.id);

    for request in [
        put_json(&uri, serde_json::json!({ "content": "..." })),
        post(&format!("{uri}/reset")),
    ] {
        let err = h.error(request, StatusCode::BAD_REQUEST).await;
        assert_eq!(err.error.code, "invalid_request");
        assert!(
            err.error.message.contains("merge_instructions")
                && err.error.message.contains("engineer"),
            "unhelpful message: {}",
            err.error.message
        );
    }
}

#[tokio::test]
async fn an_unknown_kind_is_a_400_listing_the_known_ones() {
    let h = harness().await;
    let engineer = h.profile("eng", Role::Engineer).await;

    let err = h
        .error(
            put_json(
                &format!("/v1/profiles/{}/prompts/nope", engineer.id),
                serde_json::json!({ "content": "..." }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(err.error.code, "invalid_request");
    assert!(
        err.error.message.contains("engineer_briefing"),
        "unhelpful message: {}",
        err.error.message
    );
}

#[tokio::test]
async fn an_unknown_profile_is_a_404_on_every_prompt_route() {
    let h = harness().await;

    for request in [
        get("/v1/profiles/nosuchprofile/prompts"),
        put_json(
            "/v1/profiles/nosuchprofile/prompts/engineer_briefing",
            serde_json::json!({ "content": "..." }),
        ),
        post("/v1/profiles/nosuchprofile/prompts/engineer_briefing/reset"),
        post("/v1/profiles/nosuchprofile/system-prompt/reset"),
    ] {
        let err = h.error(request, StatusCode::NOT_FOUND).await;
        assert_eq!(err.error.code, "not_found");
    }
}

#[tokio::test]
async fn resetting_the_system_prompt_returns_the_profile_on_the_role_default() {
    let h = harness().await;
    let engineer = h.profile("eng", Role::Engineer).await;
    assert_eq!(engineer.system_prompt, "You are eng.");

    let profile: ProfileDto = h
        .json(post(&format!(
            "/v1/profiles/{}/system-prompt/reset",
            engineer.id
        )))
        .await;

    assert_eq!(profile.id, engineer.id);
    assert_eq!(profile.system_prompt, default_system_prompt(Role::Engineer));
}

/// Profiles are addressable by unique name everywhere under `/v1/profiles`;
/// the prompt routes are no exception.
#[tokio::test]
async fn prompts_are_reachable_by_profile_name() {
    let h = harness().await;
    h.profile("eng", Role::Engineer).await;

    let prompts: Vec<ProfilePromptDto> = h.json(get("/v1/profiles/eng/prompts")).await;
    assert_eq!(prompts.len(), PromptKind::for_role(Role::Engineer).len());
}
