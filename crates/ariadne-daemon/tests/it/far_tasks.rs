//! Far tasks (037): the orchestrator creates a task on a named host, and the
//! goal's daemon sends it to that host and mirrors its state.
//!
//! Two harness daemons: the far one on a loopback TCP port behind a
//! [`Health`] switch, and the local one with the `direct` hosts `far` (the
//! far daemon) and `dead` (nothing listens).

use std::path::PathBuf;

use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio::sync::broadcast::Receiver;

use ariadne_api::goals::GoalDto;
use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_api::models::ModelDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::stream::DomainEvent;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{Actor, GoalStatus, TaskStatus};
use ariadne_daemon::bus::BusEvent;

use crate::common::acp::{registry_home, script, stub_acp_agent};
use crate::common::{
    Harness, QUIET, TIMEOUT, eventually, eventually_some, harness, post, post_json, sh, test_pin,
};
use crate::host_health::{Health, serve_on_tcp};

/// The goal's daemon, the far daemon, and what keeps them up.
struct Pair {
    local: Harness,
    far: Harness,
    health: Health,
    /// The model the far daemon lists, and the local one need not.
    model: String,
    _stub_dir: tempfile::TempDir,
}

async fn pair() -> Pair {
    let far = harness().await;
    let health = Health::default();
    let address = serve_on_tcp(&far, &health).await;
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
    let local = harness().home(home).await;
    local.state.hosts.start();
    ariadne_daemon::far::start(&local.state, local.timeouts);
    until_far_is(&local, HostStatus::Ready).await;
    Pair {
        local,
        far,
        health,
        model,
        _stub_dir: stub_dir,
    }
}

async fn host(h: &Harness, name: &str) -> HostDto {
    let hosts: Vec<HostDto> = h.get("/v1/hosts").await;
    hosts.into_iter().find(|host| host.name == name).unwrap()
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
    h.json(
        post_json(
            "/v1/goals",
            json!({
                "title": "Ship the widgets",
                "description": "Across the machines.",
                "repository_ids": [repo.id],
                "model": test_pin().model,
            }),
        ),
        StatusCode::CREATED,
    )
    .await
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

async fn create_task(h: &Harness, goal: &GoalDto, body: Value) -> TaskDto {
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
        assert_eq!(far.goal_id, linked[0].id);
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
    p.health.set_off(true);
    let _: HostDto = p
        .local
        .json(post("/v1/hosts/far/reconnect"), StatusCode::OK)
        .await;
    until_far_is(&p.local, HostStatus::Down).await;
    eventually(TIMEOUT, "the far task marked unreachable", || async {
        task(&p.local, &away.id).await.host_unreachable
    })
    .await;

    // What happens on the host meanwhile does not reach this daemon.
    let mut local_events = p.local.bus.subscribe();
    let started = start_on_far(&p.far, &sent.id).await;
    let quiet = tokio::time::Instant::now() + QUIET;
    while let Ok(Ok(event)) = tokio::time::timeout_at(quiet, local_events.recv()).await {
        if let DomainEvent::TaskUpdated(update) = event.event {
            assert_ne!(update.task.id, away.id, "the far task moved while away");
        }
    }
    let kept = task(&p.local, &away.id).await;
    assert_eq!(kept.status, TaskStatus::Pending);
    assert!(kept.host_unreachable);

    // On the next `ready`, the far task is read again.
    p.health.set_off(false);
    let _: HostDto = p
        .local
        .json(post("/v1/hosts/far/reconnect"), StatusCode::OK)
        .await;
    until_far_is(&p.local, HostStatus::Ready).await;
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
