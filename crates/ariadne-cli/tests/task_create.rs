//! `task create` with no goal makes a task of its own (spec 014, rules 62 to 66).
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

const REPO: &str = "01JREPO0000000000000000000";
const GOAL: &str = "01jg0a10000000000000000000";

/// What the stub daemon was asked: `(method path, body or query)`.
type Seen = Arc<Mutex<Vec<(String, Value)>>>;

fn task() -> Value {
    json!({
        "id": "01JTASK0000000000000000000", "goal_id": null, "repo_id": REPO,
        "title": "t", "description": "", "status": "pending", "agents": [],
        "depends_on": [], "branch": "b", "worktree_path": null, "stalled": false,
        "merge_commit": null, "pr_url": null, "reason": null, "workflow": null,
        "steps": [], "usage": serde_json::to_value(ariadne_api::tasks::TaskUsageDto::default()).unwrap(), "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z", "host": "local", "host_unreachable": false
    })
}

async fn daemon() -> (String, Seen, tokio::task::JoinHandle<()>) {
    let seen: Seen = Arc::default();
    let hosts = json!([{"name":"local","kind":"local","status":"ready","endpoint":"","error":null,
        "remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}},
        {"name":"build","kind":"direct","status":"ready","endpoint":"http://127.0.0.1:1","error":null,
        "remote":null,"sync":{"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0}}]);
    let app = Router::new()
        .route(
            "/v1/tasks",
            post(
                |State(seen): State<Seen>, Json(body): Json<Value>| async move {
                    seen.lock().unwrap().push(("POST /v1/tasks".into(), body));
                    Json(task())
                },
            )
            .get(
                |State(seen): State<Seen>, Query(query): Query<Value>| async move {
                    seen.lock().unwrap().push(("GET /v1/tasks".into(), query));
                    Json(json!([]))
                },
            ),
        )
        .route(
            &format!("/v1/goals/{GOAL}/tasks"),
            post(
                |State(seen): State<Seen>, Json(body): Json<Value>| async move {
                    seen.lock().unwrap().push(("POST goal tasks".into(), body));
                    Json(task())
                },
            ),
        )
        .route(
            "/v1/repositories",
            get(|| async {
                Json(
                    json!([{"id": REPO, "path": "/work/api", "base_branch": "main",
                    "default_workflow": "w", "description": null, "permission_mode": "auto",
                    "forge": null, "created_at": "x", "updated_at": "x"}]),
                )
            }),
        )
        .route("/v1/hosts", get(move || async move { Json(hosts) }))
        .with_state(Arc::clone(&seen));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (endpoint, seen, server)
}

async fn ariadne(endpoint: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args(["--endpoint", endpoint])
        .args(args)
        .env("ARIADNE_TELEMETRY", "off")
        .env("NO_COLOR", "1")
        .output()
        .await
        .unwrap();
    (
        output.status.code(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

fn body(seen: &Seen, key: &str) -> Value {
    seen.lock()
        .unwrap()
        .iter()
        .find(|(k, _)| k == key)
        .unwrap_or_else(|| panic!("no {key} was sent"))
        .1
        .clone()
}

#[tokio::test]
async fn a_task_with_no_goal_and_no_workflow_is_staffed_by_one_stepless_agent() {
    let (endpoint, seen, server) = daemon().await;
    let (code, stdout, stderr) = ariadne(
        &endpoint,
        &[
            "task",
            "create",
            "--title",
            "Bump",
            "--repo",
            "/work/api",
            "--agent",
            ":coding=claude-agent-acp:claude-opus-5@high",
        ],
    )
    .await;
    server.abort();
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("01JTASK"), "{stdout}");
    let sent = body(&seen, "POST /v1/tasks");
    assert_eq!(sent["repo_id"], REPO);
    assert!(sent.get("workflow").is_none_or(Value::is_null), "{sent}");
    assert!(sent.get("host").is_none_or(Value::is_null), "{sent}");
    assert_eq!(
        sent["agents"],
        json!([{"skills": ["coding"], "model": "claude-agent-acp:claude-opus-5", "effort": "high", "brief": null}])
    );
}

#[tokio::test]
async fn a_workflow_staffs_each_column_and_a_host_goes_in_the_body() {
    let (endpoint, seen, server) = daemon().await;
    let (code, _, stderr) = ariadne(
        &endpoint,
        &[
            "--host",
            "build",
            "task",
            "create",
            "--title",
            "Fix",
            "--repo",
            REPO,
            "--workflow",
            "develop-review-merge",
            "--agent",
            "develop=codex-acp:o3",
            "--agent",
            "merge=codex-acp:o3",
        ],
    )
    .await;
    server.abort();
    assert_eq!(code, Some(0), "{stderr}");
    let sent = body(&seen, "POST /v1/tasks");
    assert_eq!(sent["workflow"], "develop-review-merge");
    assert_eq!(sent["host"], "build");
    assert_eq!(sent["agents"][0]["step"], "develop");
    assert_eq!(sent["agents"][1]["step"], "merge");
}

#[tokio::test]
async fn a_task_with_no_goal_refuses_what_it_cannot_take() {
    let (endpoint, seen, server) = daemon().await;
    let cases: [(&[&str], &str); 6] = [
        (
            &["--title", "x", "--agent", "=codex-acp:o3"],
            "needs --repo",
        ),
        (
            &[
                "--title",
                "x",
                "--repo",
                REPO,
                "--agent",
                "=codex-acp:o3",
                "--depends-on",
                GOAL,
            ],
            "--depends-on needs a goal",
        ),
        (
            &[
                "--title",
                "x",
                "--repo",
                REPO,
                "--agent",
                "=codex-acp:o3",
                "--agent",
                "=codex-acp:o3",
            ],
            "exactly one --agent",
        ),
        (
            &[
                "--title",
                "x",
                "--repo",
                REPO,
                "--agent",
                "develop=codex-acp:o3",
            ],
            "exactly one --agent",
        ),
        (
            &[
                "--title",
                "x",
                "--repo",
                REPO,
                "--workflow",
                "w",
                "--agent",
                "=codex-acp:o3",
            ],
            "names its column",
        ),
        (
            &[
                "--title",
                "x",
                "--repo",
                REPO,
                "--workflow",
                "w",
                "--agent",
                "develop=codex-acp:o3",
                "--agent",
                ":coding=codex-acp:o3",
            ],
            "names its column",
        ),
    ];
    for (args, reason) in cases {
        let mut line = vec!["task", "create"];
        line.extend(args);
        let (code, _, stderr) = ariadne(&endpoint, &line).await;
        assert_eq!(code, Some(2), "{line:?}: {stderr}");
        assert!(stderr.contains(reason), "{line:?}: {stderr}");
    }
    server.abort();
    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing reached the daemon"
    );
}

#[tokio::test]
async fn the_local_host_sends_no_host_in_the_body() {
    let (endpoint, seen, server) = daemon().await;
    let (code, _, stderr) = ariadne(
        &endpoint,
        &[
            "--host",
            "local",
            "task",
            "create",
            "--title",
            "x",
            "--repo",
            REPO,
            "--agent",
            "=codex-acp:o3",
        ],
    )
    .await;
    server.abort();
    assert_eq!(code, Some(0), "{stderr}");
    let sent = body(&seen, "POST /v1/tasks");
    assert!(sent.get("host").is_none_or(Value::is_null), "{sent}");
}

#[tokio::test]
async fn ls_refuses_a_goal_together_with_no_goal() {
    let (endpoint, seen, server) = daemon().await;
    let (code, _, stderr) = ariadne(&endpoint, &["task", "ls", "--goal", GOAL, "--no-goal"]).await;
    server.abort();
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stderr.contains("--no-goal"), "{stderr}");
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_task_in_a_goal_is_created_as_before() {
    let (endpoint, seen, server) = daemon().await;
    let (code, _, stderr) = ariadne(
        &endpoint,
        &[
            "task",
            "create",
            GOAL,
            "--title",
            "x",
            "--agent",
            "develop=codex-acp:o3",
        ],
    )
    .await;
    server.abort();
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        body(&seen, "POST goal tasks")["agents"][0]["step"],
        "develop"
    );
}

#[tokio::test]
async fn ls_no_goal_asks_for_the_goal_less_tasks() {
    let (endpoint, seen, server) = daemon().await;
    let (code, _, stderr) = ariadne(&endpoint, &["task", "ls", "--no-goal"]).await;
    server.abort();
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(body(&seen, "GET /v1/tasks")["no_goal"], "true");
}
