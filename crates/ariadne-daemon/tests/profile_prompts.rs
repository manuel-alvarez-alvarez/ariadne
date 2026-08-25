//! Integration tests for the per-profile prompt endpoints.
//!
//! The contract is that every prompt a profile owns is listable — the ones it
//! was given and the defaults it was not, each saying which it is — editable
//! with any text whose `{placeholder}`s its kind can fill in, and resettable to
//! the default by dropping what was set; and that a kind belonging to another
//! role, or a placeholder nothing would substitute, is refused with a sentence,
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
                system_prompt: Some(format!("You are {name}.")),
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

/// How many prompts the database is actually holding: what the endpoints
/// answer is the effective prompt, which says nothing about whether a row is
/// behind it.
async fn prompt_rows(h: &Harness) -> i64 {
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        h.dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let rows = sqlx::query_scalar("SELECT COUNT(*) FROM profile_prompts")
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    rows
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

fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
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
    // Nothing was set on it, so every prompt is the default of its kind and
    // there is no date on any of them.
    for prompt in &prompts {
        assert_eq!(
            prompt.content,
            default_prompt(Role::Engineer, prompt.kind).unwrap()
        );
        assert!(prompt.is_default, "{} was stored", prompt.kind.as_str());
        assert!(prompt.updated_at.is_none());
    }
    assert_eq!(prompt_rows(&h).await, 0);
}

/// A planner lists the prompts a planner owns, and none of the engineer's.
#[tokio::test]
async fn a_planner_lists_only_its_own_prompts() {
    let h = harness().await;
    let planner = h.profile("plan", Role::Planner).await;

    let prompts: Vec<ProfilePromptDto> = h
        .json(get(&format!("/v1/profiles/{}/prompts", planner.id)))
        .await;

    assert_eq!(
        prompts.iter().map(|p| p.kind).collect::<Vec<_>>(),
        PromptKind::for_role(Role::Planner).to_vec()
    );
    assert!(
        !prompts.iter().any(|p| p.kind == PromptKind::EngineerResume),
        "a planner was given the engineer's resume"
    );
}

/// Dropping placeholders, keeping literal braces, saying almost nothing: what
/// a briefing says is the editor's business, not the API's.
///
/// And the rows follow the edit: setting one writes exactly one, resetting it
/// takes that one away again.
#[tokio::test]
async fn a_prompt_update_takes_any_content_and_a_reset_deletes_it() {
    let h = harness().await;
    let reviewer = h.profile("rev", Role::Reviewer).await;
    let uri = format!("/v1/profiles/{}/prompts/reviewer_briefing", reviewer.id);
    let content = "Review {task_title} and answer {\"verdict\": \"approve\"}.";

    let updated: ProfilePromptDto = h
        .json(put_json(&uri, serde_json::json!({ "content": content })))
        .await;
    assert_eq!(updated.kind, PromptKind::ReviewerBriefing);
    assert_eq!(updated.content, content);
    assert!(!updated.is_default);
    assert!(updated.updated_at.is_some());
    assert_eq!(prompt_rows(&h).await, 1);

    // The edit is what a later read sees, and the only prompt of the profile
    // that is not its default.
    let prompts: Vec<ProfilePromptDto> = h
        .json(get(&format!("/v1/profiles/{}/prompts", reviewer.id)))
        .await;
    assert_eq!(prompts[0].content, content);
    assert!(!prompts[0].is_default);
    assert!(prompts[1..].iter().all(|p| p.is_default));

    let reset: ProfilePromptDto = h.json(post(&format!("{uri}/reset"))).await;
    assert_eq!(
        reset.content,
        default_prompt(Role::Reviewer, PromptKind::ReviewerBriefing).unwrap()
    );
    assert!(reset.is_default);
    assert!(reset.updated_at.is_none());
    // The row is gone with it: a default is stored nowhere.
    assert_eq!(prompt_rows(&h).await, 0);
}

/// A `{token}` the kind has no value for is a 400 naming it and the ones it
/// could have used — the typo is caught here rather than shipped to an agent
/// as literal text.
#[tokio::test]
async fn a_placeholder_the_kind_cannot_fill_in_is_a_400_naming_it() {
    let h = harness().await;
    let engineer = h.profile("eng", Role::Engineer).await;
    let uri = format!("/v1/profiles/{}/prompts/engineer_briefing", engineer.id);

    let err = h
        .error(
            put_json(
                &uri,
                serde_json::json!({ "content": "# {task_titel}\n\n{task_description}" }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;

    assert_eq!(err.error.code, "invalid_request");
    assert!(
        err.error.message.contains("{task_titel}")
            && err.error.message.contains("{task_title}")
            && err.error.message.contains("{dependencies}"),
        "unhelpful message: {}",
        err.error.message
    );

    // The stored prompt is untouched.
    let prompts: Vec<ProfilePromptDto> = h
        .json(get(&format!("/v1/profiles/{}/prompts", engineer.id)))
        .await;
    assert_eq!(
        prompts[0].content,
        default_prompt(Role::Engineer, PromptKind::EngineerBriefing).unwrap()
    );
}

/// Editing a prompt kind of another role is a 400 naming the roles involved,
/// on both the update and the reset route.
#[tokio::test]
async fn a_kind_of_another_role_is_refused_with_a_sentence() {
    let h = harness().await;
    let planner = h.profile("plan", Role::Planner).await;
    let uri = format!("/v1/profiles/{}/prompts/landing_instructions", planner.id);

    for request in [
        put_json(&uri, serde_json::json!({ "content": "..." })),
        post(&format!("{uri}/reset")),
    ] {
        let err = h.error(request, StatusCode::BAD_REQUEST).await;
        assert_eq!(err.error.code, "invalid_request");
        assert!(
            err.error.message.contains("landing_instructions")
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
    assert_eq!(engineer.system_prompt.as_deref(), Some("You are eng."));

    let profile: ProfileDto = h
        .json(post(&format!(
            "/v1/profiles/{}/system-prompt/reset",
            engineer.id
        )))
        .await;

    assert_eq!(profile.id, engineer.id);
    assert_eq!(profile.system_prompt, default_system_prompt(Role::Engineer));
    assert!(profile.system_prompt_is_default);
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

/// A profile is created on the defaults of its role and stores none of them:
/// what it is briefed with is the code's until somebody sets a prompt.
#[tokio::test]
async fn a_created_profile_starts_on_every_default_and_stores_none() {
    let h = harness().await;

    let (status, body) = h
        .send(post_json(
            "/v1/profiles",
            serde_json::json!({ "name": "rev", "role": "reviewer" }),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let profile: ProfileDto = serde_json::from_slice(&body).unwrap();
    // No system prompt was given either, so the role's own is what it runs on.
    assert_eq!(profile.system_prompt, default_system_prompt(Role::Reviewer));
    assert!(profile.system_prompt_is_default);

    let prompts: Vec<ProfilePromptDto> = h
        .json(get(&format!("/v1/profiles/{}/prompts", profile.id)))
        .await;
    assert_eq!(
        prompts.iter().map(|p| p.kind).collect::<Vec<_>>(),
        PromptKind::for_role(Role::Reviewer).to_vec()
    );
    for prompt in &prompts {
        assert_eq!(
            prompt.content,
            default_prompt(Role::Reviewer, prompt.kind).unwrap()
        );
        assert!(prompt.is_default);
    }
    assert_eq!(prompt_rows(&h).await, 0);
}

/// A system prompt given at creation is the profile's own, and the reset takes
/// it back off — the same two states a briefing has.
#[tokio::test]
async fn a_system_prompt_is_the_profile_s_own_until_it_is_reset() {
    let h = harness().await;

    let (status, body) = h
        .send(post_json(
            "/v1/profiles",
            serde_json::json!({
                "name": "eng",
                "role": "engineer",
                "system_prompt": "You are eng.",
            }),
        ))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let profile: ProfileDto = serde_json::from_slice(&body).unwrap();
    assert_eq!(profile.system_prompt, "You are eng.");
    assert!(!profile.system_prompt_is_default);

    let reset: ProfileDto = h
        .json(post(&format!(
            "/v1/profiles/{}/system-prompt/reset",
            profile.id
        )))
        .await;
    assert_eq!(reset.system_prompt, default_system_prompt(Role::Engineer));
    assert!(reset.system_prompt_is_default);
}
