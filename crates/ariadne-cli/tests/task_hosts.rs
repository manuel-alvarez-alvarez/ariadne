//! `task ls` shows the host of each task of a goal (spec 014, rules 58 to 61).
use ariadne_api::tasks::{TaskDto, TaskUsageDto};
use ariadne_core::TaskStatus;
use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

const NOW: &str = "2026-01-01T00:00:00Z";

/// A task on `host` in progress, whose title is `title` with no space in it.
fn task(id: &str, title: &str, host: &str, host_unreachable: bool) -> Value {
    serde_json::to_value(TaskDto {
        step: None,
        operation: None,
        id: id.into(),
        goal_id: "01GOAL".into(),
        repo_id: "01REPO".into(),
        title: title.into(),
        description: String::new(),
        status: TaskStatus::InProgress,
        agents: Vec::new(),
        depends_on: Vec::new(),
        branch: format!("branch-{id}"),
        worktree_path: None,
        stalled: false,
        merge_commit: None,
        pr_url: None,
        reason: None,
        usage: TaskUsageDto::default(),
        created_at: NOW.into(),
        updated_at: NOW.into(),
        host: host.into(),
        host_unreachable,
    })
    .unwrap()
}

/// Serve `GET /v1/tasks` as `tasks`, and `GET /v1/hosts` as `hosts`, on a
/// free port. Returns the endpoint and the server task.
async fn daemon(tasks: Value, hosts: Value) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/v1/tasks", get(move || async move { Json(tasks) }))
        .route("/v1/hosts", get(move || async move { Json(hosts) }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (endpoint, server)
}

fn registry(build_endpoint: &str) -> Value {
    let sync =
        json!({"state":"in_sync","error":null,"missing_repositories":[],"skipped_permissions":0});
    json!([
        {"name":"local","kind":"local","status":"ready","endpoint":"","error":null,"remote":null,"sync":sync},
        {"name":"build","kind":"direct","status":"ready","endpoint":build_endpoint,"error":null,"remote":null,"sync":sync},
    ])
}

/// Run `ariadne` against `endpoint` with the given host and extra arguments,
/// and return its exit code, stdout and stderr.
async fn ariadne(endpoint: &str, host: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .args(["--endpoint", endpoint, "--host", host])
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

/// The header words of a table, uppercase, in the order printed.
fn header(stdout: &str) -> Vec<&str> {
    stdout.lines().next().unwrap().split_whitespace().collect()
}

/// The line of the table that names `title`.
fn row<'a>(stdout: &'a str, title: &str) -> &'a str {
    stdout
        .lines()
        .find(|line| line.contains(title))
        .unwrap_or_else(|| panic!("no row names {title} in:\n{stdout}"))
}

#[tokio::test]
async fn a_list_without_a_far_task_keeps_its_columns() {
    let (endpoint, server) = daemon(
        json!([task("01A", "local-only", "local", false)]),
        registry("http://127.0.0.1:1"),
    )
    .await;
    let (code, stdout, _) = ariadne(&endpoint, "local", &["task", "ls"]).await;
    server.abort();
    assert_eq!(code, Some(0));
    assert_eq!(
        header(&stdout),
        [
            "ID", "TITLE", "STATUS", "STEP", "AGE", "STALLED", "PR", "TOKENS", "BRANCH"
        ],
    );
}

#[tokio::test]
async fn a_far_task_adds_a_host_column_after_the_title() {
    let (endpoint, server) = daemon(
        json!([
            task("01A", "local-task", "local", false),
            task("01B", "far-task", "build", false),
        ]),
        registry("http://127.0.0.1:1"),
    )
    .await;
    let (code, stdout, _) = ariadne(&endpoint, "local", &["task", "ls"]).await;
    server.abort();
    assert_eq!(code, Some(0));
    assert_eq!(
        header(&stdout),
        [
            "ID", "TITLE", "HOST", "STATUS", "STEP", "AGE", "STALLED", "PR", "TOKENS", "BRANCH"
        ],
    );
    assert!(row(&stdout, "far-task").contains("build"));
    assert!(row(&stdout, "local-task").contains("local"));
}

#[tokio::test]
async fn a_far_task_on_an_unreachable_host_is_marked() {
    let (endpoint, server) = daemon(
        json!([task("01C", "cold-task", "cold", true)]),
        registry("http://127.0.0.1:1"),
    )
    .await;
    let (code, stdout, _) = ariadne(&endpoint, "local", &["task", "ls"]).await;
    server.abort();
    assert_eq!(code, Some(0));
    assert!(row(&stdout, "cold-task").contains("cold (unreachable)"));
}

#[tokio::test]
async fn json_carries_the_host_and_the_unreachable_flag_of_each_task() {
    let (endpoint, server) = daemon(
        json!([
            task("01A", "local-task", "local", false),
            task("01C", "cold-task", "cold", true),
        ]),
        registry("http://127.0.0.1:1"),
    )
    .await;
    let (code, stdout, _) = ariadne(&endpoint, "local", &["--format", "json", "task", "ls"]).await;
    server.abort();
    assert_eq!(code, Some(0));
    let tasks: Value = serde_json::from_str(&stdout).unwrap();
    let by_id = |id: &str| {
        tasks
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == id)
            .unwrap()
            .clone()
    };
    let local = by_id("01A");
    assert_eq!(local["host"], "local");
    assert_eq!(local["host_unreachable"], false);
    let cold = by_id("01C");
    assert_eq!(cold["host"], "cold");
    assert_eq!(cold["host_unreachable"], true);
}

#[tokio::test]
async fn all_hosts_keep_the_daemon_host_first_and_name_each_task_host_second() {
    let (build_endpoint, build_server) = daemon(
        json!([task("01R", "remote-task", "local", false)]),
        json!([]),
    )
    .await;
    let (endpoint, server) = daemon(
        json!([task("01B", "far-task", "build", false)]),
        registry(&build_endpoint),
    )
    .await;
    let (code, stdout, _) = ariadne(&endpoint, "all", &["task", "ls"]).await;
    server.abort();
    build_server.abort();
    assert_eq!(code, Some(0));
    let words = header(&stdout);
    assert_eq!(words[0], "HOST");
    assert_eq!(words[1], "ID");
    assert!(words.windows(2).any(|pair| pair == ["TASK", "HOST"]));
    assert!(row(&stdout, "far-task").contains("build"));
    assert!(row(&stdout, "remote-task").contains("local"));
}
