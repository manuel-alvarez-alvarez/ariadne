//! Which integrator a task is created with.
//!
//! The role has no lifecycle yet — nothing spawns an integrator session — so
//! what there is to check is the vocabulary: `POST /v1/goals/{id}/tasks` takes
//! an optional integrator profile, falls back to the built-in one, refuses a
//! profile of another role, and the DTO says which one landed on the task.
//!
//! No tmux, no git, no agent CLI: nothing here launches anything.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

use ariadne_api::error::ErrorBody;
use ariadne_api::goals::GoalDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::Role;
use ariadne_daemon::bus::EventBus;
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::{NewProfile, NewRepository, Store};

/// The id `ariadne_store::defaults` gives the built-in Integrator.
const BUILTIN_INTEGRATOR: &str = "00000000000000000000000004";

struct Harness {
    store: Store,
    router: Router,
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
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
        events: EventBus::new(),
        logs: LogBuffer::new(),
    };
    Harness {
        router: http::router(state),
        store,
        dir,
    }
}

impl Harness {
    async fn send(&self, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    async fn json<T: DeserializeOwned>(&self, request: Request<Body>, expected: StatusCode) -> T {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// A goal on a repository of its own, on the seeded built-in profiles.
    async fn goal(&self) -> GoalDto {
        // Never cloned into a worktree: nothing here reaches git.
        let repo = self
            .store
            .create_repository(NewRepository {
                path: self.dir.path().join("repo").display().to_string(),
                base_branch: "main".into(),
                description: None,
            })
            .await
            .unwrap();
        self.json(
            post_json(
                "/v1/goals",
                serde_json::json!({"title": "Land it", "repository_ids": [repo.id],
                                   "planner_profile": "Planner"}),
            ),
            StatusCode::CREATED,
        )
        .await
    }
}

fn create_task(goal: &GoalDto, body: serde_json::Value) -> Request<Body> {
    post_json(&format!("/v1/goals/{}/tasks", goal.id), body)
}

fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Nothing names an integrator today — not the CLI, not the planner's
/// `create_task` — so the built-in is what every task gets.
#[tokio::test]
async fn a_task_created_without_an_integrator_gets_the_built_in_one() {
    let h = harness().await;
    let goal = h.goal().await;

    let request = create_task(
        &goal,
        serde_json::json!({"title": "Do the thing", "engineer_profile": "Engineer",
                           "reviewer_profiles": ["Reviewer"]}),
    );
    let task: TaskDto = h.json(request, StatusCode::CREATED).await;

    assert_eq!(
        task.integrator_profile_id.as_deref(),
        Some(BUILTIN_INTEGRATOR)
    );
    // And it is on the row, not only in the answer to the create.
    let stored = h.store.get_task(&task.id).await.unwrap();
    assert_eq!(
        stored.integrator_profile_id.as_deref(),
        Some(BUILTIN_INTEGRATOR)
    );
    let read_back: TaskDto = h
        .json(
            Request::get(format!("/v1/tasks/{}", task.id))
                .body(Body::empty())
                .unwrap(),
            StatusCode::OK,
        )
        .await;
    assert_eq!(read_back.integrator_profile_id, task.integrator_profile_id);

    // And the task holds it, so the profile cannot be deleted out from under
    // it: the refusal names what is holding it, like every other reference.
    let err: ErrorBody = h
        .json(
            Request::delete(format!("/v1/profiles/{BUILTIN_INTEGRATOR}"))
                .body(Body::empty())
                .unwrap(),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        err.error.message.contains("1 task as its integrator"),
        "{err:?}"
    );
}

/// Named by name, like every other profile a task is created with.
#[tokio::test]
async fn a_named_integrator_is_the_one_the_task_keeps() {
    let h = harness().await;
    let goal = h.goal().await;
    let mine = h
        .store
        .create_profile(NewProfile {
            name: "Careful integrator".into(),
            role: Role::Integrator,
            agent_kind: None,
            model: None,
            system_prompt: "You land changes carefully.".into(),
            prompts: vec![],
        })
        .await
        .unwrap();

    let request = create_task(
        &goal,
        serde_json::json!({"title": "Do the thing", "engineer_profile": "Engineer",
                               "reviewer_profiles": ["Reviewer"],
                           "integrator_profile": "Careful integrator"}),
    );
    let task: TaskDto = h.json(request, StatusCode::CREATED).await;

    assert_eq!(
        task.integrator_profile_id.as_deref(),
        Some(mine.id.as_str())
    );
}

/// A profile of another role is refused the way the engineer's and the
/// reviewers' are, naming the role it actually has.
#[tokio::test]
async fn a_profile_of_another_role_cannot_integrate() {
    let h = harness().await;
    let goal = h.goal().await;

    let request = create_task(
        &goal,
        serde_json::json!({"title": "Do the thing", "engineer_profile": "Engineer",
                               "reviewer_profiles": ["Reviewer"],
                           "integrator_profile": "Reviewer"}),
    );
    let err: ErrorBody = h.json(request, StatusCode::BAD_REQUEST).await;

    assert!(err.error.message.contains("expected integrator"), "{err:?}");
}
