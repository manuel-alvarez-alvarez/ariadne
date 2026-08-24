//! Goals work in registered repositories, by reference.
//!
//! What a goal is created with is a repository id; what its tasks branch from
//! is whatever that repository says at the time. So the checks here run the
//! whole way through: register a repository over HTTP, create a goal on it,
//! create a task, and see the worktree the launcher makes from the path and
//! base branch the repository holds — including after that base branch moves.
//!
//! No tmux and no agent CLI: `tmux` is a stub that answers "no session" and
//! records what it was told, and the profiles are pinned to an agent kind so
//! that nothing here looks for a coding-agent CLI on `PATH`. `git` is real.

use std::path::{Path as FsPath, PathBuf};
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
use ariadne_api::profiles::ProfileDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::AgentKind;
use ariadne_daemon::bus::EventBus;
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::Store;
use ariadne_store::defaults::BUILTIN_PROFILES;

struct Harness {
    router: Router,
    launcher: Arc<Launcher>,
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = EventBus::new();
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: write_tmux_stub(dir.path()),
        git: GitManager,
    });
    let state = AppState {
        store,
        started_at: Instant::now(),
        launcher: launcher.clone(),
        sched_tx: None,
        events: bus,
        logs: LogBuffer::new(),
    };
    let harness = Harness {
        router: http::router(state),
        launcher,
        dir,
    };
    harness.pin_builtin_profiles().await;
    harness
}

/// A `tmux` with no sessions that swallows every command it is given.
fn write_tmux_stub(dir: &FsPath) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("tmux-stub.sh");
    std::fs::write(
        &bin,
        "#!/bin/sh\ncase \"$1\" in\n  has-session) exit 1 ;;\nesac\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    TmuxManager::new(bin.display().to_string())
}

impl Harness {
    /// A toy repo with `main` at v1 and `next` one commit ahead of it.
    fn repo(&self, name: &str) -> PathBuf {
        let repo = self.dir.path().join(name);
        std::fs::create_dir_all(&repo).unwrap();
        sh(
            &repo,
            "git init -q -b main && echo v1 > file.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm init && \
             git checkout -q -b next && echo v2 > file.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm ahead && \
             git checkout -q main",
        );
        repo
    }

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

    async fn error(&self, request: Request<Body>, expected: StatusCode) -> ErrorBody {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// Fix what the seeded profiles run on.
    ///
    /// They are seeded on "auto", which at spawn time means "the first
    /// coding-agent CLI on `PATH`" — and where there is none, as on every CI
    /// runner, spawning fails outright. What is under test here is the
    /// worktree a spawn cuts, not the agent it starts, so the kind is pinned
    /// and never looked up.
    async fn pin_builtin_profiles(&self) {
        for builtin in BUILTIN_PROFILES {
            let _: ProfileDto = self
                .json(
                    put_json(
                        &format!("/v1/profiles/{}", builtin.id),
                        serde_json::json!({"agent_kind": AgentKind::ClaudeCode.as_str()}),
                    ),
                    StatusCode::OK,
                )
                .await;
        }
    }

    async fn register(&self, path: &FsPath, base_branch: &str) -> RepositoryDto {
        self.json(
            post_json(
                "/v1/repositories",
                serde_json::json!({"path": path.display().to_string(),
                                   "base_branch": base_branch}),
            ),
            StatusCode::CREATED,
        )
        .await
    }

    async fn goal_on(&self, repository_ids: Vec<&str>) -> GoalDto {
        self.json(
            post_json(
                "/v1/goals",
                serde_json::json!({"title": "Ship it", "repository_ids": repository_ids,
                                   "planner_profile": "Planner"}),
            ),
            StatusCode::CREATED,
        )
        .await
    }

    async fn task_in(&self, goal: &GoalDto) -> TaskDto {
        self.json(
            post_json(
                &format!("/v1/goals/{}/tasks", goal.id),
                serde_json::json!({"title": "Do the thing", "engineer_profile": "Engineer",
                                   "integrator_profile": "Local Integrator",
                                   "reviewer_profiles": ["Reviewer"]}),
            ),
            StatusCode::CREATED,
        )
        .await
    }
}

fn sh(dir: &FsPath, cmd: &str) -> String {
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "command failed: {cmd}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
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

fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// Register a repository, create a goal on it, create a task: the engineer's
/// worktree is cut from that repository's checkout and base branch.
#[tokio::test]
async fn a_task_branches_from_the_repository_its_goal_references() {
    let h = harness().await;
    let repo = h.repo("repo");
    let registered = h.register(&repo, "next").await;
    let goal = h.goal_on(vec![&registered.id]).await;
    assert_eq!(goal.repos.len(), 1);
    assert_eq!(goal.repos[0].id, registered.id);

    let task = h.task_in(&goal).await;
    assert_eq!(task.repo_id, registered.id, "the goal has one repository");

    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    let worktree = PathBuf::from(session.worktree_path.unwrap());
    assert!(worktree.is_dir(), "the worktree was created");
    assert_eq!(
        sh(&worktree, "git rev-parse --abbrev-ref HEAD"),
        task.branch
    );
    assert_eq!(
        sh(&worktree, "git rev-parse HEAD"),
        sh(&repo, "git rev-parse next"),
        "the branch starts at the repository's base branch"
    );
}

/// The goal reads its repository live: moving the base branch moves what the
/// next task branches from, with nothing to update on the goal.
#[tokio::test]
async fn editing_the_base_branch_moves_what_new_tasks_branch_from() {
    let h = harness().await;
    let repo = h.repo("repo");
    let registered = h.register(&repo, "next").await;
    let goal = h.goal_on(vec![&registered.id]).await;

    let edited: RepositoryDto = h
        .json(
            put_json(
                &format!("/v1/repositories/{}", registered.id),
                serde_json::json!({"base_branch": "main"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.base_branch, "main");
    let goal: GoalDto = h
        .json(
            Request::builder()
                .uri(format!("/v1/goals/{}", goal.id))
                .body(Body::empty())
                .unwrap(),
            StatusCode::OK,
        )
        .await;
    assert_eq!(goal.repos[0].base_branch, "main", "the goal moved with it");

    let task = h.task_in(&goal).await;
    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    let worktree = PathBuf::from(session.worktree_path.unwrap());
    assert_eq!(
        sh(&worktree, "git rev-parse HEAD"),
        sh(&repo, "git rev-parse main"),
        "the new base branch is what the worktree starts from"
    );
}

/// Deleting a repository a goal works in would leave the goal pointing at
/// nothing, so it is refused — and the refusal says who is holding it.
#[tokio::test]
async fn a_repository_in_use_cannot_be_deleted() {
    let h = harness().await;
    let repo = h.repo("repo");
    let registered = h.register(&repo, "main").await;
    let goal = h.goal_on(vec![&registered.id]).await;
    h.task_in(&goal).await;

    let err = h
        .error(
            delete(&format!("/v1/repositories/{}", registered.id)),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        err.error.message.contains("1 goal"),
        "{}",
        err.error.message
    );
    assert!(
        err.error.message.contains("1 task"),
        "{}",
        err.error.message
    );

    // An unheld one still goes.
    let spare = h.register(&h.repo("spare"), "main").await;
    let (status, _) = h
        .send(delete(&format!("/v1/repositories/{}", spare.id)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// A goal is created on repositories that are registered, and nothing else:
/// an id nobody registered is a 404, not a repository invented on the spot.
#[tokio::test]
async fn a_goal_cannot_be_created_on_an_unknown_repository() {
    let h = harness().await;
    let registered = h.register(&h.repo("repo"), "main").await;

    let err = h
        .error(
            post_json(
                "/v1/goals",
                serde_json::json!({"title": "Ship it", "planner_profile": "Planner",
                                   "repository_ids": [registered.id, "01nosuchrepository"]}),
            ),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert!(
        err.error.message.contains("01nosuchrepository"),
        "the refusal names the id: {}",
        err.error.message
    );

    // And no repository at all is a bad request, as it always was.
    h.error(
        post_json(
            "/v1/goals",
            serde_json::json!({"title": "Ship it", "planner_profile": "Planner",
                               "repository_ids": []}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;

    let goals: Vec<GoalDto> = h
        .json(
            Request::builder()
                .uri("/v1/goals")
                .body(Body::empty())
                .unwrap(),
            StatusCode::OK,
        )
        .await;
    assert!(goals.is_empty(), "neither attempt left a goal behind");
}
