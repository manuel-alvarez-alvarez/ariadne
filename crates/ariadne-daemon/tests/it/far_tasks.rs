//! Far tasks (037): the orchestrator creates a task on a named host, and the
//! goal's daemon sends it to that host and mirrors its state.
//!
//! Two harness daemons: the far one on a loopback TCP port behind a
//! [`Front`], and the local one with the `direct` hosts `far` (the far
//! daemon) and `dead` (nothing listens).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::response::IntoResponse;

use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio::sync::broadcast::Receiver;
use tokio::sync::watch;

use ariadne_api::goals::GoalDto;
use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_api::models::ModelDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::stream::DomainEvent;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{Actor, GoalStatus, TaskStatus};
use ariadne_daemon::bus::BusEvent;
use ariadne_daemon::timeouts::Timeouts;

use crate::common::acp::{registry_home, script, stub_acp_agent};
use crate::common::{
    Harness, QUIET, RUNS_OUT, TIMEOUT, as_session, eventually, eventually_some, harness,
    patch_json, post, post_json, sh, test_pin,
};

/// What the far daemon's answers go through: its `GET /v1/health` turned
/// off, answers lost after the far daemon made them, and requests held. It
/// records each request it passes, and each it answers, by its [`kind`].
#[derive(Clone)]
struct Front {
    health_off: Arc<AtomicBool>,
    protocol_override: Arc<Mutex<Option<Value>>>,
    stream_cut: Arc<watch::Sender<bool>>,
    /// Frames put into each open stream among the ones the far daemon sends.
    stream_frames: tokio::sync::broadcast::Sender<&'static str>,
    /// The kinds of request whose next answer is lost, once each, and the
    /// kind to hold from then on.
    lose: Arc<Mutex<Vec<Loss>>>,
    /// The kinds of request whose answer's body waits until they are no
    /// longer held.
    held: Arc<watch::Sender<Vec<&'static str>>>,
    /// The kinds of request that wait, headers and all, until they are no
    /// longer held: the far daemon gets them only then.
    held_before: Arc<watch::Sender<Vec<&'static str>>>,
    waiting: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<&'static str>>>,
    answered: Arc<Mutex<Vec<&'static str>>>,
}

/// The kind of request whose next answer is lost, and the kind to hold
/// from then on.
type Loss = (&'static str, Option<&'static str>);

/// The kind of a request to the far daemon, as [`Front`] records it.
fn kind(method: &axum::http::Method, path: &str) -> &'static str {
    match (method.as_str(), path) {
        ("POST", "/v1/goals") => "create goal",
        ("PUT", path) if path.starts_with("/v1/remote-tasks/") => "create remote task",
        ("POST", path) if path.ends_with("/dependency-failure") => "dependency failure",
        ("POST", path) if path.ends_with("/retry-dependency") => "dependency retry",
        ("POST", path) if path.ends_with("/tasks") => "create task",
        ("GET", "/v1/events/stream") => "stream",
        ("GET", "/v1/remote-tasks/protocol") => "protocol",
        ("GET", "/v1/tasks") => "find task",
        ("GET", path) if path.starts_with("/v1/tasks/") => "read task",
        ("GET", "/v1/health") => "health",
        ("GET", path)
            if path.starts_with("/v1/sessions/")
                && (path.ends_with("/console") || path.ends_with("/console/stream")) =>
        {
            "retired console route"
        }
        _ => "other",
    }
}

impl Default for Front {
    fn default() -> Self {
        Self {
            health_off: Arc::default(),
            protocol_override: Arc::default(),
            stream_cut: Arc::new(watch::Sender::new(false)),
            stream_frames: tokio::sync::broadcast::channel(8).0,
            lose: Arc::default(),
            held: Arc::new(watch::Sender::new(Vec::new())),
            held_before: Arc::new(watch::Sender::new(Vec::new())),
            waiting: Arc::default(),
            seen: Arc::default(),
            answered: Arc::default(),
        }
    }
}

impl Front {
    /// Put a `resync` frame into each open stream, as the far daemon does
    /// when a client lags (012).
    fn resync(&self) {
        let _ = self
            .stream_frames
            .send("event: resync\ndata: {\"missed\":1}\n\n");
    }

    fn set_health_off(&self, off: bool) {
        self.health_off.store(off, Ordering::SeqCst);
    }

    fn lose_next(&self, kind: &'static str) {
        self.lose.lock().unwrap().push((kind, None));
    }

    /// Lose the next answer of `kind`, then hold each request of `then`.
    fn lose_next_then_hold(&self, kind: &'static str, then: &'static str) {
        self.lose.lock().unwrap().push((kind, Some(then)));
    }

    fn count(&self, kind: &str) -> usize {
        count_of(&self.seen, kind)
    }

    fn answers(&self, kind: &str) -> usize {
        count_of(&self.answered, kind)
    }

    /// How many requests wait while they are held.
    fn waiting(&self) -> usize {
        self.waiting.load(Ordering::SeqCst)
    }

    fn hold(&self, kind: &'static str) {
        self.held.send_modify(|held| held.push(kind));
    }

    /// Hold each request of `kind` before the far daemon gets it.
    fn hold_before(&self, kind: &'static str) {
        self.held_before.send_modify(|held| held.push(kind));
    }

    fn release(&self) {
        self.held.send_modify(Vec::clear);
        self.held_before.send_modify(Vec::clear);
    }

    /// `far`'s router behind this front, on a loopback TCP port.
    async fn serve(&self, far: &Harness) -> String {
        let front = self.clone();
        let router = far.router.clone().layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let front = front.clone();
                async move {
                    let kind = kind(request.method(), request.uri().path());
                    front.seen.lock().unwrap().push(kind);
                    if kind == "health" && front.health_off.load(Ordering::SeqCst) {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    if kind == "protocol"
                        && let Some(value) = front.protocol_override.lock().unwrap().clone()
                    {
                        return axum::Json(value).into_response();
                    }
                    if kind == "stream" && *front.stream_cut.borrow() {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    let mut before = front.held_before.subscribe();
                    if before.borrow().contains(&kind) {
                        let _waiting = Waiting::new(&front.waiting);
                        let _ = before.wait_for(|held| !held.contains(&kind)).await;
                    }
                    let mut held = front.held.subscribe();
                    if held.borrow().contains(&kind) {
                        // The headers go at once and the body waits: a
                        // client bounds its wait for the headers, not for the
                        // body, so the request waits for as long as it is
                        // held. The count goes down when the wait ends, and
                        // when the caller drops the request while it waits.
                        // A held request answers `200`, as the ones the tests
                        // hold do.
                        let waiting = Waiting::new(&front.waiting);
                        let answered = front.answered.clone();
                        let body = futures_util::stream::once(async move {
                            let _waiting = waiting;
                            let _ = held.wait_for(|held| !held.contains(&kind)).await;
                            let answer = next.run(request).await;
                            answered.lock().unwrap().push(kind);
                            axum::body::to_bytes(answer.into_body(), usize::MAX).await
                        });
                        return (
                            [(axum::http::header::CONTENT_TYPE, "application/json")],
                            axum::body::Body::from_stream(body),
                        )
                            .into_response();
                    }
                    let lost = {
                        let mut lose = front.lose.lock().unwrap();
                        lose.iter()
                            .position(|(k, _)| *k == kind)
                            .map(|at| lose.remove(at))
                    };
                    let answer = next.run(request).await;
                    front.answered.lock().unwrap().push(kind);
                    if let Some((_, then)) = lost {
                        if let Some(then) = then {
                            front.hold(then);
                        }
                        // The far daemon made it; its answer never arrives.
                        return StatusCode::BAD_GATEWAY.into_response();
                    }
                    if kind == "stream" {
                        use futures_util::StreamExt;
                        let cut = front.stream_cut.subscribe();
                        let frames = front.stream_frames.subscribe();
                        let (parts, body) = answer.into_parts();
                        let stream = futures_util::stream::unfold(
                            (body.into_data_stream(), cut, frames),
                            |(mut body, mut cut, mut frames)| async move {
                                let frame = tokio::select! {
                                    frame = body.next() => frame,
                                    Ok(frame) = frames.recv() => Some(Ok(frame.into())),
                                    _ = cut.wait_for(|cut| *cut) => None,
                                };
                                frame.map(|frame| (frame, (body, cut, frames)))
                            },
                        );
                        return axum::response::Response::from_parts(
                            parts,
                            axum::body::Body::from_stream(stream),
                        );
                    }
                    answer
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        address
    }
}

/// One request that waits while it is held, for as long as it waits.
struct Waiting(Arc<AtomicUsize>);

impl Waiting {
    fn new(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count.clone())
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn count_of(kinds: &Mutex<Vec<&'static str>>, kind: &str) -> usize {
    kinds.lock().unwrap().iter().filter(|k| **k == kind).count()
}

/// The goal's daemon, the far daemon, and what keeps them up.
struct Pair {
    local: Harness,
    far: Harness,
    front: Front,
    /// The model the far daemon lists, and the local one need not.
    model: String,
    _stub_dir: tempfile::TempDir,
}

async fn pair() -> Pair {
    pair_on(false).await
}

/// [`pair`], with a scheduler on the goal's daemon where `scheduler`.
async fn pair_on(scheduler: bool) -> Pair {
    let pair = pair_setup(scheduler).await;
    ariadne_daemon::far::start(&pair.local.state, pair.local.timeouts);
    pair
}

async fn pair_setup(scheduler: bool) -> Pair {
    let far = harness().await;
    let front = Front::default();
    let address = front.serve(&far).await;
    let models: Vec<ModelDto> = far.get("/v1/models").await;
    let model = models
        .first()
        .expect("the far daemon lists a model")
        .id
        .clone();

    // The local daemon knows the stub agent, so its goals can name a model.
    let stub_dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(stub_dir.path(), script());
    let home = registry_home(&stub);
    std::fs::write(
        home.join("host-fixture.toml"),
        format!(
            "[hosts.far]\nendpoint = \"http://{address}\"\n\
             [hosts.dead]\nendpoint = \"http://127.0.0.1:1\"\n"
        ),
    )
    .unwrap();
    // A send that the host did not answer is tried again soon.
    let local = harness().home(home).timeouts(Timeouts {
        far_retry: RUNS_OUT / 10,
        // A call to the host that nothing ends outlasts each wait of a test:
        // only the readiness fence ends it.
        far_call: TIMEOUT * 4,
        ..Timeouts::default()
    });
    let local = if scheduler { local.scheduler() } else { local }.await;
    local.state.hosts.start();
    until_far_is(&local, HostStatus::Ready).await;
    Pair {
        local,
        far,
        front,
        model,
        _stub_dir: stub_dir,
    }
}

async fn host(h: &Harness, name: &str) -> HostDto {
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    hosts.into_iter().find(|host| host.name == name).unwrap()
}

/// Make the host `far` leave `ready`.
async fn go_away(p: &Pair) {
    p.front.set_health_off(true);
    let _: HostDto = p
        .local
        .json(post("/v1/hosts/far/reconnect"), StatusCode::OK)
        .await;
    until_far_is(&p.local, HostStatus::Down).await;
}

/// Bring the host `far` back to `ready`, at the same endpoint.
async fn back(p: &Pair) {
    p.front.set_health_off(false);
    let _: HostDto = p
        .local
        .json(post("/v1/hosts/far/reconnect"), StatusCode::OK)
        .await;
    until_far_is(&p.local, HostStatus::Ready).await;
}

async fn until_far_is(h: &Harness, status: HostStatus) {
    eventually(TIMEOUT, "the host far in the status", || async {
        host(h, "far").await.status == status
    })
    .await;
}

/// A checkout under `h` whose `origin` is `owner/name` on GitHub.
async fn forge_repository(h: &Harness, dir: &str, owner_name: &str) -> RepositoryDto {
    let path: PathBuf = h.git_repo(dir);
    sh(
        &path,
        &format!("git remote add origin https://github.com/{owner_name}.git"),
    );
    h.json(
        post_json(
            "/v1/repositories",
            json!({"path": path.display().to_string()}),
        ),
        StatusCode::CREATED,
    )
    .await
}

async fn goal_on(h: &Harness, repo: &RepositoryDto) -> GoalDto {
    goal_on_all(h, &[repo]).await
}

async fn goal_on_all(h: &Harness, repos: &[&RepositoryDto]) -> GoalDto {
    let ids: Vec<&str> = repos.iter().map(|repo| repo.id.as_str()).collect();
    h.json(
        post_json(
            "/v1/goals",
            json!({
                "title": "Ship the widgets",
                "description": "Across the machines.",
                "repository_ids": ids,
                "model": test_pin().model,
            }),
        ),
        StatusCode::CREATED,
    )
    .await
}

/// Wait until the goal's daemon has recorded what the host made of `task`:
/// the far task's branch is the branch here.
async fn until_sent(h: &Harness, task: &TaskDto, far: &TaskDto) {
    eventually(TIMEOUT, "the far task recorded here", || async {
        self::task(h, &task.id).await.branch == far.branch
    })
    .await;
}

/// A task body staffed with `model` on every column of `goal`.
fn task_body(goal: &GoalDto, title: &str, model: &str, extra: Value) -> Value {
    let agents: Vec<Value> = goal
        .steps
        .iter()
        .map(|step| json!({"step": step.id, "model": model}))
        .collect();
    let mut body = json!({"title": title, "agents": agents});
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    body
}

/// Seed an existing mirror. New HTTP creation uses source references instead.
async fn create_task(h: &Harness, goal: &GoalDto, body: Value) -> TaskDto {
    if let Some(host) = body["host"].as_str().filter(|h| *h != "local") {
        let agents: Vec<ariadne_api::tasks::AgentAssignment> =
            serde_json::from_value(body["agents"].clone()).unwrap();
        let task = h
            .store
            .create_far_task(
                ariadne_store::NewTask {
                    goal_id: goal.id.clone(),
                    repo_id: body["repo_id"].as_str().unwrap_or(&goal.repos[0].id).into(),
                    title: body["title"].as_str().unwrap().into(),
                    description: body["description"].as_str().unwrap_or_default().into(),
                    agents: agents
                        .into_iter()
                        .map(|a| ariadne_store::NewTaskAgent {
                            step: a.step.unwrap_or_default(),
                            skills: a.skills,
                            pin: ariadne_store::AgentPin {
                                model: a.model,
                                effort: a.effort,
                            },
                            brief: a.brief,
                        })
                        .collect(),
                    depends_on: body["depends_on"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|v| v.as_str().unwrap().into())
                        .collect(),
                },
                host,
            )
            .await
            .unwrap();
        return h.get(&format!("/v1/tasks/{}", task.id)).await;
    }
    h.json(
        post_json(&format!("/v1/goals/{}/tasks", goal.id), body),
        StatusCode::CREATED,
    )
    .await
}

async fn refusal(h: &Harness, goal: &GoalDto, body: Value, status: StatusCode) -> String {
    h.error(
        post_json(&format!("/v1/goals/{}/tasks", goal.id), body),
        status,
    )
    .await
    .error
    .message
}

async fn activate(h: &Harness, goal: &GoalDto) {
    h.store
        .set_goal_status(&goal.id, GoalStatus::Active)
        .await
        .unwrap();
}

/// The linked goals on the far daemon that `goal` feeds.
async fn linked_goals(far: &Harness, goal: &GoalDto) -> Vec<GoalDto> {
    let goals: Vec<GoalDto> = far.get("/v1/goals").await;
    goals
        .into_iter()
        .filter(|g| {
            g.linked
                .as_ref()
                .is_some_and(|link| link.origin_goal_id == goal.id)
        })
        .collect()
}

/// The far daemon's tasks in the linked goal of `goal`.
async fn far_tasks(far: &Harness, goal: &GoalDto) -> Vec<TaskDto> {
    let mut tasks = Vec::new();
    for linked in linked_goals(far, goal).await {
        let more: Vec<TaskDto> = far.get(&format!("/v1/tasks?goal={}", linked.id)).await;
        tasks.extend(more);
    }
    tasks
}

/// The far task titled `title`, once the far daemon has it.
async fn far_task(far: &Harness, goal: &GoalDto, title: &str) -> TaskDto {
    eventually_some(TIMEOUT, "the far task on the host", async || {
        far_tasks(far, goal)
            .await
            .into_iter()
            .find(|task| task.title == title)
    })
    .await
}

async fn task(h: &Harness, id: &str) -> TaskDto {
    h.get(&format!("/v1/tasks/{id}")).await
}

#[tokio::test]
async fn cancelling_a_sent_far_task_cancels_it_on_its_host() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let local = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Cancel", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let far = far_task(&p.far, &goal, "Cancel").await;
    until_sent(&p.local, &local, &far).await;
    let cancelled: TaskDto = p
        .local
        .json(
            post(&format!("/v1/tasks/{}/cancel", local.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
    eventually(TIMEOUT, "the host cancels the task", || async {
        task(&p.far, &far.id).await.status == TaskStatus::Cancelled
    })
    .await;
}

#[tokio::test]
async fn editing_a_sent_far_task_edits_it_on_its_host() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let local = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Edit", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let far = far_task(&p.far, &goal, "Edit").await;
    until_sent(&p.local, &local, &far).await;
    let edited: TaskDto = p
        .local
        .json(
            patch_json(
                &format!("/v1/tasks/{}", local.id),
                json!({"title": "Changed"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.title, "Changed");
    assert_eq!(task(&p.far, &far.id).await.title, "Changed");
}

#[tokio::test]
async fn retrying_a_sent_far_task_retries_it_on_its_host() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let local = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Retry", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let far = far_task(&p.far, &goal, "Retry").await;
    until_sent(&p.local, &local, &far).await;
    start_on_far(&p.far, &far.id).await;
    p.far
        .store
        .transition_task(
            &far.id,
            TaskStatus::Failed,
            Actor::Agent,
            Some("failed"),
            None,
        )
        .await
        .unwrap();
    eventually(TIMEOUT, "failure mirrored", || async {
        task(&p.local, &local.id).await.status == TaskStatus::Failed
    })
    .await;
    let retried: TaskDto = p
        .local
        .json(
            post(&format!("/v1/tasks/{}/retry", local.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(retried.status, TaskStatus::Ready);
    assert_eq!(task(&p.far, &far.id).await.status, TaskStatus::Ready);
}

#[tokio::test]
async fn cancelling_a_goal_cancels_its_linked_goal() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let local = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Close", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let far = far_task(&p.far, &goal, "Close").await;
    until_sent(&p.local, &local, &far).await;
    let _: GoalDto = p
        .local
        .json(
            post(&format!("/v1/goals/{}/cancel", goal.id)),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the linked goal is cancelled", || async {
        linked_goals(&p.far, &goal).await[0].status == GoalStatus::Cancelled
    })
    .await;
}

#[tokio::test]
async fn completing_a_goal_completes_its_linked_goal() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let local = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Complete", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let far = far_task(&p.far, &goal, "Complete").await;
    until_sent(&p.local, &local, &far).await;
    start_on_far(&p.far, &far.id).await;
    merge_on_far(&p.far, &far.id, "c0ffee").await;
    eventually(TIMEOUT, "the task is finished", || async {
        task(&p.local, &local.id).await.status == TaskStatus::Finished
    })
    .await;
    let _: GoalDto = p
        .local
        .json(
            post_json(&format!("/v1/goals/{}/complete", goal.id), json!({})),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the linked goal is completed", || async {
        linked_goals(&p.far, &goal).await[0].status == GoalStatus::Completed
    })
    .await;
}

#[tokio::test]
async fn a_cancelled_goal_closes_its_linked_goal_when_the_host_returns() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let local = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Away", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let far = far_task(&p.far, &goal, "Away").await;
    until_sent(&p.local, &local, &far).await;
    go_away(&p).await;
    let _: GoalDto = p
        .local
        .json(
            post(&format!("/v1/goals/{}/cancel", goal.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        linked_goals(&p.far, &goal).await[0].status,
        GoalStatus::Active
    );
    back(&p).await;
    eventually(TIMEOUT, "the linked goal is cancelled", || async {
        linked_goals(&p.far, &goal).await[0].status == GoalStatus::Cancelled
    })
    .await;
}

#[tokio::test]
async fn a_goal_closes_a_link_whose_create_answer_was_lost() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    create_task(
        &p.local,
        &goal,
        task_body(&goal, "Lost", &p.model, json!({"host": "far"})),
    )
    .await;
    p.front.lose_next_then_hold("create goal", "find task");
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "the next send waits", || async {
        p.front.waiting() > 0
    })
    .await;
    let _: GoalDto = p
        .local
        .json(
            post(&format!("/v1/goals/{}/cancel", goal.id)),
            StatusCode::OK,
        )
        .await;
    p.front.release();
    eventually(TIMEOUT, "the linked goal is cancelled", || async {
        linked_goals(&p.far, &goal).await[0].status == GoalStatus::Cancelled
    })
    .await;
}

#[tokio::test]
async fn a_write_to_a_sent_far_task_refuses_a_down_host() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let local = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Away", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let far = far_task(&p.far, &goal, "Away").await;
    until_sent(&p.local, &local, &far).await;
    go_away(&p).await;
    let error = p
        .local
        .error(
            post(&format!("/v1/tasks/{}/cancel", local.id)),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(error.error.message.contains("far"));
    assert!(error.error.message.contains("down"));
    assert_eq!(task(&p.far, &far.id).await.status, far.status);
}

#[tokio::test]
async fn a_dependency_held_far_task_can_be_cancelled_while_its_host_is_down() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let dependency = create_task(
        &p.local,
        &goal,
        task_body(&goal, "First", &test_pin().model, json!({})),
    )
    .await;
    let held = create_task(
        &p.local,
        &goal,
        task_body(
            &goal,
            "Held",
            &p.model,
            json!({"host": "far", "depends_on": [dependency.id]}),
        ),
    )
    .await;
    activate(&p.local, &goal).await;
    go_away(&p).await;
    let cancelled: TaskDto = p
        .local
        .json(
            post(&format!("/v1/tasks/{}/cancel", held.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
    assert!(far_tasks(&p.far, &goal).await.is_empty());
}

#[tokio::test]
async fn an_unsent_unheld_far_task_refuses_a_write_while_its_host_is_down() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let task = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Unsent", &p.model, json!({"host": "far"})),
    )
    .await;
    go_away(&p).await;
    let error = p
        .local
        .error(
            post(&format!("/v1/tasks/{}/cancel", task.id)),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(error.error.message.contains("far"));
    assert!(error.error.message.contains("down"));
}

#[tokio::test]
async fn a_far_agent_session_can_be_switched_on_its_host() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    let theirs = forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let local = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Switch", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let far = far_task(&p.far, &goal, "Switch").await;
    until_sent(&p.local, &local, &far).await;
    sh(
        std::path::Path::new(&theirs.path),
        &format!("git remote set-url origin {}", theirs.path),
    );
    start_on_far(&p.far, &far.id).await;
    let far_task = p.far.store.get_task(&far.id).await.unwrap();
    let agent = p
        .far
        .store
        .list_task_agents(&far.id)
        .await
        .unwrap()
        .remove(0);
    let session = p
        .far
        .launcher
        .start_step_agent(&far_task, &agent)
        .await
        .unwrap();
    let here = task(&p.local, &local.id).await;
    let id = here
        .agents
        .iter()
        .find(|a| a.step == agent.step)
        .unwrap()
        .session_id
        .as_ref()
        .unwrap();
    let _: ariadne_api::sessions::SessionDto = p
        .local
        .json(
            post_json(
                &format!("/v1/sessions/{id}/switch"),
                json!({"model": "stub:b"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        p.far.store.get_session(&session.id).await.unwrap().model,
        "stub:b"
    );
    assert_eq!(task(&p.local, &local.id).await.agents[0].model, "stub:b");
}

/// Listen for `QUIET`: no task titled `title` reaches the far daemon in that
/// while.
async fn not_sent(events: &mut Receiver<BusEvent>, title: &str) {
    let quiet = tokio::time::Instant::now() + QUIET;
    while let Ok(Ok(event)) = tokio::time::timeout_at(quiet, events.recv()).await {
        if let DomainEvent::TaskCreated(task) = event.event {
            assert_ne!(task.title, title, "the task reached the far daemon");
        }
    }
}

/// Walk a far daemon's task to its first column, as its scheduler would.
async fn start_on_far(far: &Harness, id: &str) -> TaskDto {
    far.store
        .transition_task(id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    far.store.start_first_step(id).await.unwrap();
    task(far, id).await
}

/// Merge a far daemon's task from the column it is in.
async fn merge_on_far(far: &Harness, id: &str, commit: &str) -> TaskDto {
    let step = task(far, id).await.step.unwrap();
    far.store
        .end_step_by_daemon(id, &step, "merged", commit)
        .await
        .unwrap();
    task(far, id).await
}

#[tokio::test]
async fn a_task_without_a_host_runs_on_the_goals_own_daemon() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                json!({"title": "Here", "repository_ids": [repo.id], "model": test_pin().model}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let model = test_pin().model;
    for extra in [json!({}), json!({"host": "local"})] {
        let created = create_task(&h, &goal, task_body(&goal, "Here", &model, extra)).await;
        assert_eq!(created.host, "local");
        assert!(!created.host_unreachable);
        assert_eq!(created.status, TaskStatus::Pending);
    }
}

#[tokio::test]
async fn a_far_task_is_refused_on_a_host_that_is_unknown_or_not_ready() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;

    let unknown = refusal(
        &p.local,
        &goal,
        task_body(&goal, "Nowhere", &p.model, json!({"host": "nowhere"})),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert!(unknown.contains("unknown host nowhere"), "{unknown}");

    let status = host(&p.local, "dead").await.status;
    assert_ne!(status, HostStatus::Ready);
    let unready = refusal(
        &p.local,
        &goal,
        task_body(&goal, "Dead", &p.model, json!({"host": "dead"})),
        StatusCode::CONFLICT,
    )
    .await;
    let named = format!("host dead is {}", format!("{status:?}").to_lowercase());
    assert!(unready.contains(&named), "{unready}");
}

#[tokio::test]
async fn a_far_task_is_refused_on_a_host_that_lacks_its_repository() {
    let p = pair().await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let lacking = forge_repository(&p.local, "lacking", "acme/gadgets").await;
    let goal = goal_on(&p.local, &lacking).await;

    let message = refusal(
        &p.local,
        &goal,
        task_body(&goal, "Gadgets", &p.model, json!({"host": "far"})),
        StatusCode::CONFLICT,
    )
    .await;
    for part in [
        "host far lacks repository acme/gadgets".to_string(),
        format!("`repository:{}` step", lacking.id),
        "ariadne host setup far".to_string(),
    ] {
        assert!(message.contains(&part), "{part:?} not in {message}");
    }
}

#[tokio::test]
async fn a_far_task_is_refused_a_model_its_host_does_not_list() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;

    let message = refusal(
        &p.local,
        &goal,
        task_body(
            &goal,
            "Unlisted",
            "stub:not-offered",
            json!({"host": "far"}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert!(
        message.contains("host far does not list the model stub:not-offered"),
        "{message}"
    );
}

#[tokio::test]
async fn one_linked_goal_on_a_host_takes_every_far_task_of_a_goal() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    let theirs = forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    for title in ["First far", "Second far"] {
        let created = create_task(
            &p.local,
            &goal,
            task_body(&goal, title, &p.model, json!({"host": "far"})),
        )
        .await;
        assert_eq!(created.host, "far");
        assert_eq!(created.status, TaskStatus::Pending);
    }
    // Nothing goes to the host while the goal plans.
    assert!(linked_goals(&p.far, &goal).await.is_empty());
    activate(&p.local, &goal).await;

    let first = far_task(&p.far, &goal, "First far").await;
    let second = far_task(&p.far, &goal, "Second far").await;
    let linked = linked_goals(&p.far, &goal).await;
    assert_eq!(linked.len(), 1, "{linked:?}");
    assert_eq!(linked[0].title, goal.title);
    assert_eq!(linked[0].status, GoalStatus::Active);
    for far in [&first, &second] {
        assert_eq!(far.goal_id.as_ref(), Some(&linked[0].id));
        assert_eq!(far.repo_id, theirs.id);
        assert_eq!(far.agents[0].model, p.model);
    }
}

#[tokio::test]
async fn a_far_task_waits_until_its_dependencies_merge_on_any_host() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let far_first = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Far first", &p.model, json!({"host": "far"})),
    )
    .await;
    let here = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Here first", &test_pin().model, json!({})),
    )
    .await;
    let after = create_task(
        &p.local,
        &goal,
        task_body(
            &goal,
            "Far after",
            &p.model,
            json!({"host": "far", "depends_on": [far_first.id, here.id]}),
        ),
    )
    .await;
    let mut far_events = p.far.bus.subscribe();
    activate(&p.local, &goal).await;

    // The far dependency goes to the host; the task after it waits.
    let sent = far_task(&p.far, &goal, "Far first").await;
    not_sent(&mut far_events, "Far after").await;
    assert_eq!(task(&p.local, &after.id).await.status, TaskStatus::Pending);

    // Merged on the host: the dependency on this daemon still holds it.
    start_on_far(&p.far, &sent.id).await;
    merge_on_far(&p.far, &sent.id, "f00d").await;
    eventually(TIMEOUT, "the far dependency merged here", || async {
        task(&p.local, &far_first.id).await.status == TaskStatus::Finished
    })
    .await;
    not_sent(&mut far_events, "Far after").await;
    assert_eq!(task(&p.local, &after.id).await.status, TaskStatus::Pending);

    // Merged here too: the task goes to its host.
    p.local
        .store
        .transition_task(&here.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    p.local.store.start_first_step(&here.id).await.unwrap();
    let step = task(&p.local, &here.id).await.step.unwrap();
    p.local
        .store
        .end_step_by_daemon(&here.id, &step, "merged", "beef")
        .await
        .unwrap();
    let sent_after = far_task(&p.far, &goal, "Far after").await;
    assert!(sent_after.depends_on.is_empty());
}

#[tokio::test]
async fn a_far_task_mirrors_the_state_its_host_reports() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let mirrored = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Mirror me", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let sent = far_task(&p.far, &goal, "Mirror me").await;
    assert_ne!(sent.branch, mirrored.branch);

    let started = start_on_far(&p.far, &sent.id).await;
    eventually(TIMEOUT, "the column of the far task", || async {
        let here = task(&p.local, &mirrored.id).await;
        here.status == TaskStatus::InProgress
            && here.step == started.step
            && here.branch == started.branch
    })
    .await;

    let url = "https://github.com/acme/widgets/pull/7";
    p.far
        .store
        .set_task_pull_request(&sent.id, url)
        .await
        .unwrap();
    let merged = merge_on_far(&p.far, &sent.id, "c0ffee").await;
    let here = eventually_some(TIMEOUT, "the far task merged here", async || {
        let here = task(&p.local, &mirrored.id).await;
        (here.status == TaskStatus::Finished).then_some(here)
    })
    .await;
    assert_eq!(here.step, merged.step);
    assert_eq!(here.branch, merged.branch);
    assert_eq!(here.pr_url.as_deref(), Some(url));
    assert_eq!(here.merge_commit.as_deref(), Some("c0ffee"));
    assert_eq!(here.host, "far");
    assert!(!here.host_unreachable);
}

#[tokio::test]
async fn a_far_task_keeps_its_state_while_its_host_is_away() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let away = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Away", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let sent = far_task(&p.far, &goal, "Away").await;
    eventually(TIMEOUT, "the far branch here", || async {
        task(&p.local, &away.id).await.branch == sent.branch
    })
    .await;

    // The host leaves `ready`.
    go_away(&p).await;
    eventually(TIMEOUT, "the far task marked unreachable", || async {
        task(&p.local, &away.id).await.host_unreachable
    })
    .await;

    // What happens on the host meanwhile does not reach this daemon.
    let mut local_events = p.local.bus.subscribe();
    let started = start_on_far(&p.far, &sent.id).await;
    let quiet = tokio::time::Instant::now() + QUIET;
    while let Ok(Ok(event)) = tokio::time::timeout_at(quiet, local_events.recv()).await {
        // The flag's own update may come late; nothing else may.
        if let DomainEvent::TaskUpdated(update) = event.event {
            assert!(
                update.task.id != away.id
                    || (update.task.host_unreachable && update.task.status == TaskStatus::Pending),
                "the far task moved while away: {:?}",
                update.task
            );
        }
    }
    let kept = task(&p.local, &away.id).await;
    assert_eq!(kept.status, TaskStatus::Pending);
    assert!(kept.host_unreachable);

    // On the next `ready`, the far task is read again.
    back(&p).await;
    let back = eventually_some(TIMEOUT, "the far task read again", async || {
        let here = task(&p.local, &away.id).await;
        (!here.host_unreachable).then_some(here)
    })
    .await;
    assert_eq!(back.status, TaskStatus::InProgress);
    assert_eq!(back.step, started.step);
}

#[tokio::test]
async fn the_openapi_document_carries_the_host_of_a_task() {
    let h = harness().await;
    let doc: Value = h.get("/api-docs/openapi.json").await;
    let schemas = &doc["components"]["schemas"];
    let task = &schemas["TaskDto"];
    assert_eq!(task["properties"]["host"]["type"], "string", "{task}");
    assert_eq!(task["properties"]["host_unreachable"]["type"], "boolean");
    let create = &schemas["CreateTaskRequest"]["properties"]["host"];
    assert!(create.is_object(), "{}", schemas["CreateTaskRequest"]);
}

#[tokio::test]
async fn a_model_its_host_turned_off_is_the_users_choice_alone() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    p.far
        .store
        .set_model_enabled(&p.model, false)
        .await
        .unwrap();

    let stored = p.local.store.get_goal(&goal.id).await.unwrap();
    let orchestrator = p.local.orchestrator_session(&stored).await;
    let staffed = p
        .local
        .error(
            as_session(
                &format!("/v1/goals/{}/tasks", goal.id),
                &orchestrator.id,
                task_body(&goal, "Staffed", &p.model, json!({"host": "far"})),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await
        .error
        .message;
    let named = format!("host far has the model {} turned off", p.model);
    assert!(staffed.contains(&named), "{staffed}");

    let chosen = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Chosen", &p.model, json!({"host": "far"})),
    )
    .await;
    assert_eq!(chosen.host, "far");
}

#[tokio::test]
async fn a_lost_answer_of_the_host_makes_no_second_goal_or_task() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let once = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Once", &p.model, json!({"host": "far"})),
    )
    .await;
    // The host makes the goal and the task, and each first answer is lost.
    p.front.lose_next("create goal");
    p.front.lose_next("create task");
    activate(&p.local, &goal).await;

    let sent = far_task(&p.far, &goal, "Once").await;
    until_sent(&p.local, &once, &sent).await;
    // Each send asks first what the host made: the third finds the task.
    assert!(p.front.count("create goal") >= 2);
    assert_eq!(p.front.count("create task"), 1);
    assert!(p.front.count("find task") >= 3);
    assert_eq!(linked_goals(&p.far, &goal).await.len(), 1);
    assert_eq!(far_tasks(&p.far, &goal).await.len(), 1);
}

#[tokio::test]
async fn a_repository_set_up_later_joins_the_same_linked_goal() {
    let p = pair().await;
    let widgets = forge_repository(&p.local, "widgets", "acme/widgets").await;
    let gadgets = forge_repository(&p.local, "gadgets", "acme/gadgets").await;
    forge_repository(&p.far, "far-widgets", "acme/widgets").await;
    let goal = goal_on_all(&p.local, &[&widgets, &gadgets]).await;
    let first = create_task(
        &p.local,
        &goal,
        task_body(
            &goal,
            "Widgets",
            &p.model,
            json!({"host": "far", "repo_id": widgets.id}),
        ),
    )
    .await;
    activate(&p.local, &goal).await;
    let sent = far_task(&p.far, &goal, "Widgets").await;
    until_sent(&p.local, &first, &sent).await;
    let linked = linked_goals(&p.far, &goal).await;
    assert_eq!(linked[0].repos.len(), 1);

    // The host gets the second repository, and a task on it.
    let far_gadgets = forge_repository(&p.far, "far-gadgets", "acme/gadgets").await;
    create_task(
        &p.local,
        &goal,
        task_body(
            &goal,
            "Gadgets",
            &p.model,
            json!({"host": "far", "repo_id": gadgets.id}),
        ),
    )
    .await;
    let second = far_task(&p.far, &goal, "Gadgets").await;
    let linked = linked_goals(&p.far, &goal).await;
    assert_eq!(linked.len(), 1, "{linked:?}");
    assert_eq!(second.goal_id.as_ref(), Some(&linked[0].id));
    assert_eq!(second.repo_id, far_gadgets.id);
    assert!(linked[0].repos.iter().any(|repo| repo.id == far_gadgets.id));
}

#[tokio::test]
async fn a_far_task_cancelled_here_stays_cancelled_whatever_its_host_reports() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let cancelled_goal = goal_on(&p.local, &repo).await;
    let going_goal = goal_on(&p.local, &repo).await;
    let cancelled = create_task(
        &p.local,
        &cancelled_goal,
        task_body(
            &cancelled_goal,
            "Cancelled here",
            &p.model,
            json!({"host": "far"}),
        ),
    )
    .await;
    let going = create_task(
        &p.local,
        &going_goal,
        task_body(&going_goal, "Goes on", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &cancelled_goal).await;
    activate(&p.local, &going_goal).await;
    let far_cancelled = far_task(&p.far, &cancelled_goal, "Cancelled here").await;
    let far_going = far_task(&p.far, &going_goal, "Goes on").await;
    until_sent(&p.local, &cancelled, &far_cancelled).await;
    until_sent(&p.local, &going, &far_going).await;

    // The task is cancelled here before the host reports its next state.
    p.local
        .store
        .transition_task(
            &cancelled.id,
            TaskStatus::Cancelled,
            Actor::User,
            Some("goal cancelled"),
            None,
        )
        .await
        .unwrap();
    // The host goes on with it. Its update comes before the other task's,
    // on the one stream.
    start_on_far(&p.far, &far_cancelled.id).await;
    start_on_far(&p.far, &far_going.id).await;
    eventually(TIMEOUT, "the other far task started here", || async {
        task(&p.local, &going.id).await.status == TaskStatus::InProgress
    })
    .await;
    assert_eq!(
        task(&p.local, &cancelled.id).await.status,
        TaskStatus::Cancelled
    );

    // The host merges it while it is away, and the read on its return
    // keeps the cancellation too.
    go_away(&p).await;
    eventually(TIMEOUT, "the far tasks marked unreachable", || async {
        task(&p.local, &going.id).await.host_unreachable
    })
    .await;
    p.local
        .store
        .end_goal(&cancelled_goal.id, GoalStatus::Cancelled)
        .await
        .unwrap();
    merge_on_far(&p.far, &far_cancelled.id, "dead").await;
    let reads = p.front.answers("read task");
    back(&p).await;
    let kept = eventually_some(TIMEOUT, "the host read again", async || {
        let kept = task(&p.local, &cancelled.id).await;
        (p.front.answers("read task") >= reads + 2
            && !kept.host_unreachable
            && !task(&p.local, &going.id).await.host_unreachable)
            .then_some(kept)
    })
    .await;
    assert_eq!(kept.status, TaskStatus::Cancelled);
    assert_eq!(kept.merge_commit, None);
}

#[tokio::test]
async fn a_far_task_of_a_completed_goal_is_unreachable_while_its_host_is_away() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let done = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Done", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let sent = far_task(&p.far, &goal, "Done").await;
    start_on_far(&p.far, &sent.id).await;
    merge_on_far(&p.far, &sent.id, "d0ne").await;
    eventually(TIMEOUT, "the far task merged here", || async {
        task(&p.local, &done.id).await.status == TaskStatus::Finished
    })
    .await;
    p.local
        .store
        .end_goal(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();

    go_away(&p).await;
    eventually(TIMEOUT, "the far task marked unreachable", || async {
        task(&p.local, &done.id).await.host_unreachable
    })
    .await;
    let reads = p.front.answers("read task");
    back(&p).await;
    eventually(TIMEOUT, "the far task read again", || async {
        p.front.answers("read task") > reads && !task(&p.local, &done.id).await.host_unreachable
    })
    .await;
    let kept = task(&p.local, &done.id).await;
    assert_eq!(kept.status, TaskStatus::Finished);
    assert_eq!(kept.merge_commit.as_deref(), Some("d0ne"));
}

#[tokio::test]
async fn a_host_back_at_the_same_endpoint_is_read_again_while_its_worker_is_busy() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let busy = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Busy", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let sent = far_task(&p.far, &goal, "Busy").await;
    until_sent(&p.local, &busy, &sent).await;

    // The worker comes back to the host and waits on its read of the task.
    p.front.hold("read task");
    let reads = p.front.count("read task");
    go_away(&p).await;
    back(&p).await;
    eventually(TIMEOUT, "the worker waits on its read", || async {
        p.front.count("read task") > reads && p.front.waiting() > 0
    })
    .await;

    // Meanwhile the host leaves `ready` and comes back at the same endpoint.
    let streams = p.front.count("stream");
    go_away(&p).await;
    back(&p).await;
    p.front.release();
    eventually(TIMEOUT, "the stream opened again", || async {
        p.front.count("stream") > streams
    })
    .await;
    eventually(TIMEOUT, "the far task read again", || async {
        !task(&p.local, &busy.id).await.host_unreachable
    })
    .await;
}

#[tokio::test]
async fn a_far_task_fails_when_a_dependency_ends_unmerged() {
    let p = pair_on(true).await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let first = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Fails first", &test_pin().model, json!({})),
    )
    .await;
    let after = create_task(
        &p.local,
        &goal,
        task_body(
            &goal,
            "Never sent",
            &p.model,
            json!({"host": "far", "depends_on": [first.id]}),
        ),
    )
    .await;
    p.local
        .store
        .transition_task(
            &first.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("no"),
            None,
        )
        .await
        .unwrap();
    activate(&p.local, &goal).await;
    p.local.notify(&after.id);
    p.local.flush_scheduler().await;

    let failed = task(&p.local, &after.id).await;
    assert_eq!(failed.status, TaskStatus::Failed);
    let reason = failed.reason.unwrap_or_default();
    assert!(reason.contains("\"Fails first\""), "{reason}");
    assert!(reason.contains("failed"), "{reason}");
    assert!(far_tasks(&p.far, &goal).await.is_empty());
}

#[tokio::test]
async fn a_task_the_host_made_is_found_again_after_its_linked_goal_ended() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let once = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Made once", &p.model, json!({"host": "far"})),
    )
    .await;
    // The host makes the task, its answer is lost, and the next send waits.
    p.front.lose_next_then_hold("create task", "find task");
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "the next send waits", || async {
        p.front.waiting() > 0
    })
    .await;

    // The linked goal ends on the host before the task is sent again.
    let made = far_task(&p.far, &goal, "Made once").await;
    p.far
        .store
        .end_goal(made.goal_id.as_deref().unwrap(), GoalStatus::Completed)
        .await
        .unwrap();
    p.front.release();

    until_sent(&p.local, &once, &made).await;
    let goals = linked_goals(&p.far, &goal).await;
    assert_eq!(goals.len(), 1, "{goals:?}");
    assert_eq!(goals[0].status, GoalStatus::Completed);
    assert_eq!(far_tasks(&p.far, &goal).await.len(), 1);
}

#[tokio::test]
async fn a_read_that_answers_after_the_host_left_writes_nothing() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let late = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Late", &p.model, json!({"host": "far"})),
    )
    .await;
    activate(&p.local, &goal).await;
    let sent = far_task(&p.far, &goal, "Late").await;
    until_sent(&p.local, &late, &sent).await;

    // The host comes back, and the read of the task waits.
    p.front.hold("read task");
    go_away(&p).await;
    let started = start_on_far(&p.far, &sent.id).await;
    back(&p).await;
    eventually(TIMEOUT, "the read waits", || async {
        p.front.waiting() > 0
    })
    .await;

    // The host leaves again while the read waits. The read then ends, as
    // an answer or as a dropped call, and writes nothing.
    go_away(&p).await;
    let mut local_events = p.local.bus.subscribe();
    p.front.release();
    eventually(TIMEOUT, "the late read ended", || async {
        p.front.waiting() == 0
    })
    .await;
    let quiet = tokio::time::Instant::now() + QUIET;
    while let Ok(Ok(event)) = tokio::time::timeout_at(quiet, local_events.recv()).await {
        if let DomainEvent::TaskUpdated(update) = event.event {
            assert!(
                update.task.id != late.id || update.task.host_unreachable,
                "the late read reached the far task: {:?}",
                update.task
            );
        }
    }
    let kept = task(&p.local, &late.id).await;
    assert_eq!(kept.status, TaskStatus::Pending);
    assert!(kept.host_unreachable);

    // The next `ready` reads it.
    back(&p).await;
    let read = eventually_some(TIMEOUT, "the far task read again", async || {
        let here = task(&p.local, &late.id).await;
        (!here.host_unreachable).then_some(here)
    })
    .await;
    assert_eq!(read.status, TaskStatus::InProgress);
    assert_eq!(read.step, started.step);
}

#[tokio::test]
async fn a_linked_task_takes_no_host_beside_its_origin_task() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let linked: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                json!({
                    "title": "Fed",
                    "repository_ids": [repo.id],
                    "model": test_pin().model,
                    "linked": {"origin_goal_id": "01arz3ndektsv4rrffq69g5fav"},
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    let model = test_pin().model;
    let message = refusal(
        &h,
        &linked,
        task_body(
            &linked,
            "Both",
            &model,
            json!({"host": "far", "origin_task_id": "01arz3ndektsv4rrffq69g5fat"}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert!(
        message.contains("origin_task_id and a host other than local"),
        "{message}"
    );
}

/// The body of a linked goal create for the origin goal `origin`.
fn linked_body(repo: &ariadne_store::Repository, origin: &str) -> Value {
    json!({
        "title": "Fed",
        "repository_ids": [repo.id],
        "model": test_pin().model,
        "linked": {"origin_goal_id": origin},
    })
}

#[tokio::test]
async fn linked_goal_creates_at_once_make_one_goal() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let body = linked_body(&repo, "01arz3ndektsv4rrffq69g5fav");
    let answers = futures_util::future::join_all(
        (0..8).map(|_| h.send(post_json("/v1/goals", body.clone()))),
    )
    .await;
    let mut ids = Vec::new();
    let mut made = 0;
    for (status, body) in answers {
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "{status}: {}",
            String::from_utf8_lossy(&body)
        );
        made += usize::from(status == StatusCode::CREATED);
        let goal: GoalDto = serde_json::from_slice(&body).unwrap();
        ids.push(goal.id);
    }
    assert_eq!(made, 1);
    ids.dedup();
    assert_eq!(ids.len(), 1, "{ids:?}");
}

#[tokio::test]
async fn a_linked_goal_that_ended_takes_no_more_tasks() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let body = linked_body(&repo, "01arz3ndektsv4rrffq69g5fav");
    let goal: GoalDto = h
        .json(post_json("/v1/goals", body.clone()), StatusCode::CREATED)
        .await;
    h.store
        .end_goal(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();
    let message = h
        .error(post_json("/v1/goals", body), StatusCode::CONFLICT)
        .await
        .error
        .message;
    assert!(message.contains(&goal.id), "{message}");
    assert!(message.contains("takes no more tasks"), "{message}");
}

#[tokio::test]
async fn a_lost_goal_answer_after_its_goal_ended_fails_the_far_task() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let lost = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Goal lost", &p.model, json!({"host": "far"})),
    )
    .await;
    // The host makes the linked goal, its answer is lost, and the next send
    // waits.
    p.front.lose_next_then_hold("create goal", "find task");
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "the next send waits", || async {
        p.front.waiting() > 0
    })
    .await;
    let made = linked_goals(&p.far, &goal).await;
    assert_eq!(made.len(), 1);
    p.far
        .store
        .end_goal(&made[0].id, GoalStatus::Cancelled)
        .await
        .unwrap();
    p.front.release();

    let failed = eventually_some(TIMEOUT, "the far task failed", async || {
        let here = task(&p.local, &lost.id).await;
        (here.status == TaskStatus::Failed).then_some(here)
    })
    .await;
    let reason = failed.reason.unwrap_or_default();
    assert!(reason.contains("host far refused the task"), "{reason}");
    assert!(reason.contains("takes no more tasks"), "{reason}");
    assert_eq!(linked_goals(&p.far, &goal).await.len(), 1);
    assert!(far_tasks(&p.far, &goal).await.is_empty());
}

#[tokio::test]
async fn a_send_under_way_when_the_host_leaves_writes_nothing() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let held = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Held send", &p.model, json!({"host": "far"})),
    )
    .await;
    p.front.hold("find task");
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "the send waits", || async {
        p.front.waiting() > 0
    })
    .await;

    // The host leaves while the send waits: the flag does not wait on it.
    go_away(&p).await;
    eventually(TIMEOUT, "the far task marked unreachable", || async {
        task(&p.local, &held.id).await.host_unreachable
    })
    .await;

    // The question ends late, as an empty answer or as a dropped call, and
    // the send goes no further.
    let mut far_events = p.far.bus.subscribe();
    p.front.release();
    eventually(TIMEOUT, "the late question ended", || async {
        p.front.waiting() == 0
    })
    .await;
    let quiet = tokio::time::Instant::now() + QUIET;
    while let Ok(Ok(event)) = tokio::time::timeout_at(quiet, far_events.recv()).await {
        assert!(
            !matches!(
                event.event,
                DomainEvent::GoalCreated(_) | DomainEvent::TaskCreated(_)
            ),
            "the send went on after the host left: {:?}",
            event.event
        );
    }
    assert_eq!(p.front.count("create goal"), 0);
    let kept = task(&p.local, &held.id).await;
    assert_eq!(kept.status, TaskStatus::Pending);
    assert_eq!(kept.branch, held.branch);

    // The next `ready` sends it.
    back(&p).await;
    let sent = far_task(&p.far, &goal, "Held send").await;
    until_sent(&p.local, &held, &sent).await;
}

#[tokio::test]
async fn a_stream_that_opens_when_the_host_leaves_does_not_hold_the_flag() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    activate(&p.local, &goal).await;

    // The host comes back, and its event stream waits to open.
    go_away(&p).await;
    p.front.hold_before("stream");
    back(&p).await;
    eventually(TIMEOUT, "the stream waits to open", || async {
        p.front.waiting() > 0
    })
    .await;
    // A task the host takes meanwhile is reachable.
    let taken = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Taken", &p.model, json!({"host": "far"})),
    )
    .await;
    assert!(!taken.host_unreachable);

    // The host leaves before the stream opens: the flag does not wait on it.
    go_away(&p).await;
    eventually(TIMEOUT, "the far task marked unreachable", || async {
        task(&p.local, &taken.id).await.host_unreachable
    })
    .await;

    p.front.release();
    back(&p).await;
    let sent = far_task(&p.far, &goal, "Taken").await;
    until_sent(&p.local, &taken, &sent).await;
}

#[tokio::test]
async fn a_linked_goal_that_ended_takes_no_new_task_and_keeps_its_own() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                linked_body(&repo, "01arz3ndektsv4rrffq69g5fav"),
            ),
            StatusCode::CREATED,
        )
        .await;
    let model = test_pin().model;
    let body = |origin: &str| task_body(&goal, "Held", &model, json!({"origin_task_id": origin}));
    let first = create_task(&h, &goal, body("01arz3ndektsv4rrffq69g5fa1")).await;
    h.store
        .end_goal(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();

    // The task the goal holds answers a repeated create; a new one is refused.
    let again: TaskDto = h
        .json(
            post_json(
                &format!("/v1/goals/{}/tasks", goal.id),
                body("01arz3ndektsv4rrffq69g5fa1"),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(again.id, first.id);
    let message = refusal(
        &h,
        &goal,
        body("01arz3ndektsv4rrffq69g5fa2"),
        StatusCode::CONFLICT,
    )
    .await;
    assert!(message.contains("takes no more tasks"), "{message}");
}

#[tokio::test]
async fn a_linked_goal_that_ends_between_the_two_creates_fails_the_far_task() {
    let p = pair().await;
    let repo = forge_repository(&p.local, "mine", "acme/widgets").await;
    forge_repository(&p.far, "theirs", "acme/widgets").await;
    let goal = goal_on(&p.local, &repo).await;
    let between = create_task(
        &p.local,
        &goal,
        task_body(&goal, "Between", &p.model, json!({"host": "far"})),
    )
    .await;
    // The goal create goes through, and the task create waits.
    p.front.hold_before("create task");
    activate(&p.local, &goal).await;
    eventually(TIMEOUT, "the task create waits", || async {
        p.front.waiting() > 0
    })
    .await;
    let made = linked_goals(&p.far, &goal).await;
    assert_eq!(made.len(), 1);
    p.far
        .store
        .end_goal(&made[0].id, GoalStatus::Completed)
        .await
        .unwrap();
    p.front.release();

    let failed = eventually_some(TIMEOUT, "the far task failed", async || {
        let here = task(&p.local, &between.id).await;
        (here.status == TaskStatus::Failed).then_some(here)
    })
    .await;
    let reason = failed.reason.unwrap_or_default();
    assert!(reason.contains("host far refused the task"), "{reason}");
    assert!(reason.contains("takes no more tasks"), "{reason}");
    assert!(far_tasks(&p.far, &goal).await.is_empty());
    assert_eq!(p.front.count("create task"), 1);
}

#[tokio::test]
async fn a_linked_goal_with_an_admitted_task_cannot_complete() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    let goal: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                linked_body(&repo, "01arz3ndektsv4rrffq69g5fav"),
            ),
            StatusCode::CREATED,
        )
        .await;
    let model = test_pin().model;
    create_task(
        &h,
        &goal,
        task_body(
            &goal,
            "Admitted",
            &model,
            json!({"origin_task_id": "01arz3ndektsv4rrffq69g5fa1"}),
        ),
    )
    .await;

    let message = h
        .error(
            post_json(&format!("/v1/goals/{}/complete", goal.id), json!({})),
            StatusCode::CONFLICT,
        )
        .await
        .error
        .message;
    assert!(message.contains("Admitted (pending)"), "{message}");
    let kept: GoalDto = h.get(&format!("/v1/goals/{}", goal.id)).await;
    assert_eq!(kept.status, GoalStatus::Active);
}

mod posts;
mod source_references;
