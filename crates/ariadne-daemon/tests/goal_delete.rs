//! Deleting a goal: `DELETE /v1/goals/{id}`.
//!
//! The contract is that only a finished goal can go — an active one still owns
//! tmux sessions and worktrees that cancelling is what tears down — that what
//! goes takes its tasks and messages with it, and that the deletion reaches the
//! domain-event stream so clients stop showing what no longer exists.

use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tokio::sync::broadcast::Receiver;
use tower::ServiceExt;

use ariadne_api::error::ErrorBody;
use ariadne_api::goals::GoalDto;
use ariadne_api::messages::MessageDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::stream::DomainEvent;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{AgentKind, Role};
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{AgentSession, NewProfile, NewSession, Store};

/// How long a test waits for an event before giving up.
const TIMEOUT: Duration = Duration::from_secs(5);

struct Harness {
    bus: EventBus,
    router: Router,
    store: Store,
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    // Installed before anything writes, exactly as the daemon does at startup.
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        // A stub rather than the real thing: this file is the only one that
        // asks the launcher to kill a pane, and what it asserts is that the
        // kill was issued before the rows went.
        tmux: write_tmux_stub(dir.path()),
        git: GitManager,
    });
    let state = AppState {
        store: store.clone(),
        started_at: Instant::now(),
        launcher,
        sched_tx: None,
        events: bus.clone(),
        logs: LogBuffer::new(),
    };
    Harness {
        router: http::router(state),
        bus,
        store,
        dir,
    }
}

/// A `tmux` whose sessions are the names a test wrote into `alive`, and which
/// writes down every `kill-session` it is handed.
fn write_tmux_stub(dir: &FsPath) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(dir.join("alive"), "").unwrap();
    let bin = dir.join("tmux-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         alive='{alive}'\n\
         killed='{killed}'\n\
         target=''\n\
         prev=''\n\
         for a in \"$@\"; do\n\
        \x20 if [ \"$prev\" = \"-t\" ]; then target=\"$a\"; fi\n\
        \x20 prev=\"$a\"\n\
         done\n\
         case \"$1\" in\n\
        \x20 has-session) grep -qx \"$target\" \"$alive\" || exit 1 ;;\n\
        \x20 kill-session) echo \"$target\" >> \"$killed\" ;;\n\
         esac\n\
         exit 0\n",
        alive = dir.join("alive").display(),
        killed = dir.join("kill-session.log").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    TmuxManager::new(bin.display().to_string())
}

impl Harness {
    /// A toy repo with an initial commit on `main`. Nothing here spawns an
    /// agent; the repository exists because a goal needs one.
    fn repo(&self, name: &str) -> PathBuf {
        let repo = self.dir.path().join(name);
        std::fs::create_dir_all(&repo).unwrap();
        sh(
            &repo,
            "git init -q -b main && echo v1 > file.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm init",
        );
        repo
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    /// Send a request expected to answer `expected` and decode its JSON body.
    async fn json<T: DeserializeOwned>(&self, request: Request<Body>, expected: StatusCode) -> T {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// Send a request expected to fail and decode the error envelope.
    async fn error(&self, request: Request<Body>, expected: StatusCode) -> ErrorBody {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// A goal in `planning` on a freshly registered repository.
    async fn goal(&self, name: &str) -> GoalDto {
        let repo = self.repo(name);
        let registered: RepositoryDto = self
            .json(
                post_json(
                    "/v1/repositories",
                    serde_json::json!({"path": repo.display().to_string(),
                                       "base_branch": "main"}),
                ),
                StatusCode::CREATED,
            )
            .await;
        self.json(
            post_json(
                "/v1/goals",
                serde_json::json!({"title": "Ship it", "repository_ids": [registered.id],
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
                                   "integrator_profile": "Integrator",
                                   "reviewer_profiles": ["Reviewer"]}),
            ),
            StatusCode::CREATED,
        )
        .await
    }

    /// A live session on a goal, with a pane the stub tmux answers for.
    async fn live_session(&self, goal: &GoalDto) -> AgentSession {
        let planner = self
            .store
            .create_profile(NewProfile {
                name: "leftover planner".into(),
                role: Role::Planner,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: "You plan.".into(),
                prompts: vec![],
            })
            .await
            .unwrap();
        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: None,
                role: Role::Planner,
                profile_id: planner.id,
                agent_kind: AgentKind::ClaudeCode,
                model: None,
                tmux_session: session_name(&goal.id, None, "pla", None),
                worktree_path: None,
                review_round: None,
            })
            .await
            .unwrap();
        std::fs::write(
            self.dir.path().join("alive"),
            format!("{}\n", session.tmux_session),
        )
        .unwrap();
        session
    }

    /// The panes the launcher asked tmux to kill.
    fn killed_panes(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("kill-session.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    async fn cancel(&self, goal: &GoalDto) -> GoalDto {
        self.json(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/goals/{}/cancel", goal.id))
                .body(Body::empty())
                .unwrap(),
            StatusCode::OK,
        )
        .await
    }
}

fn sh(dir: &FsPath, cmd: &str) {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "command failed: {cmd}");
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(uri)
        .body(Body::empty())
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

/// Wait for the first event matching `pred`, skipping unrelated ones.
async fn next_event(rx: &mut Receiver<BusEvent>, pred: impl Fn(&BusEvent) -> bool) -> BusEvent {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let event = rx.recv().await.expect("event bus closed");
            if pred(&event) {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for a matching domain event")
}

/// A cancelled goal deletes, tasks and messages with it, and the stream says so.
#[tokio::test]
async fn deleting_a_finished_goal_takes_its_children_and_reaches_the_stream() {
    let h = harness().await;
    let goal = h.goal("repo").await;
    let task = h.task_in(&goal).await;
    let _: MessageDto = h
        .json(
            post_json(
                &format!("/v1/goals/{}/messages", goal.id),
                serde_json::json!({"body": "how is it going?"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let goal = h.cancel(&goal).await;

    let mut rx = h.bus.subscribe();
    let (status, body) = h.send(delete(&format!("/v1/goals/{}", goal.id))).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "{}",
        String::from_utf8_lossy(&body)
    );

    // Nothing is left to refetch, so the event carries the id alone — scoped
    // to the goal, for a stream that is following just this one.
    let event = next_event(&mut rx, |e| e.event.kind() == "goal_deleted").await;
    assert_eq!(event.goal_id.as_deref(), Some(goal.id.as_str()));
    assert_eq!(event.task_id, None);
    let DomainEvent::GoalDeleted(gone) = event.event else {
        unreachable!("matched on kind above");
    };
    assert_eq!(gone.id, goal.id);

    h.error(
        get(&format!("/v1/goals/{}", goal.id)),
        StatusCode::NOT_FOUND,
    )
    .await;
    let goals: Vec<GoalDto> = h.json(get("/v1/goals"), StatusCode::OK).await;
    assert!(goals.is_empty(), "the goal is gone from the list too");

    // ON DELETE CASCADE: the task went with it, and so did the thread.
    h.error(
        get(&format!("/v1/tasks/{}", task.id)),
        StatusCode::NOT_FOUND,
    )
    .await;
    let tasks: Vec<TaskDto> = h.json(get("/v1/tasks"), StatusCode::OK).await;
    assert!(tasks.is_empty(), "no task outlives its goal");
    h.error(
        get(&format!("/v1/goals/{}/messages", goal.id)),
        StatusCode::NOT_FOUND,
    )
    .await;

    // The repository the goal referenced is untouched, and free again.
    let repos: Vec<RepositoryDto> = h.json(get("/v1/repositories"), StatusCode::OK).await;
    assert_eq!(repos.len(), 1);
    let (status, _) = h
        .send(delete(&format!("/v1/repositories/{}", repos[0].id)))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "nothing holds it any more");
}

/// A goal that has not finished owns sessions and worktrees only cancelling
/// tears down, so deleting it is refused — and nothing is dropped on the way.
#[tokio::test]
async fn an_unfinished_goal_is_refused_and_keeps_everything() {
    let h = harness().await;
    let goal = h.goal("repo").await;
    let task = h.task_in(&goal).await;

    let err = h
        .error(
            delete(&format!("/v1/goals/{}", goal.id)),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(err.error.code, "conflict");
    assert!(
        err.error.message.contains("planning") && err.error.message.contains("cancel"),
        "the refusal says what the goal is and what to do about it: {}",
        err.error.message
    );

    let still_there: GoalDto = h
        .json(get(&format!("/v1/goals/{}", goal.id)), StatusCode::OK)
        .await;
    assert_eq!(still_there.id, goal.id);
    let _: TaskDto = h
        .json(get(&format!("/v1/tasks/{}", task.id)), StatusCode::OK)
        .await;

    // Cancelling is the way through, and then it goes.
    h.cancel(&goal).await;
    let (status, _) = h.send(delete(&format!("/v1/goals/{}", goal.id))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// An id nobody has is a 404, as it is on every other goal endpoint.
#[tokio::test]
async fn deleting_an_unknown_goal_is_a_404() {
    let h = harness().await;
    let err = h
        .error(delete("/v1/goals/01nosuchgoal"), StatusCode::NOT_FOUND)
        .await;
    assert!(
        err.error.message.contains("01nosuchgoal"),
        "the refusal names the id: {}",
        err.error.message
    );
}

/// The delete is what makes an orphan permanent: the rows cascade away, and a
/// pane that outlived them is no longer anything the daemon can name, let
/// alone reap. A finished goal is not supposed to own one — but if it does,
/// the pane goes before the rows do.
#[tokio::test]
async fn deleting_a_goal_takes_down_a_session_that_outlived_it() {
    let h = harness().await;
    let goal = h.goal("repo").await;
    let goal = h.cancel(&goal).await;
    let session = h.live_session(&goal).await;

    let (status, body) = h.send(delete(&format!("/v1/goals/{}", goal.id))).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(
        h.killed_panes(),
        vec![session.tmux_session],
        "the pane was killed before its row was deleted"
    );
    h.error(
        get(&format!("/v1/sessions/{}", session.id)),
        StatusCode::NOT_FOUND,
    )
    .await;
}
