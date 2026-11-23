//! CLI output follows the shared attention list, even when that list is partial.

use std::process::Output;

use ariadne_api::attention::{
    AttentionCause, AttentionItemDto, AttentionListDto, AttentionProducer, AttentionSubjectDto,
    AttentionSubjectKind, AttentionTarget,
};
use ariadne_api::sessions::{SessionEntryDto, SessionPageDto};
use ariadne_api::tasks::TaskDto;
use ariadne_core::TaskStatus;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

fn attention(complete: bool) -> AttentionListDto {
    AttentionListDto {
        complete,
        items: vec![AttentionItemDto {
            id: "visible-item".into(),
            occurrence: "visible-occurrence".into(),
            producer: AttentionProducer::Recovery,
            reason: AttentionCause::Configuration,
            summary: "Configure an available model".into(),
            required_action: "Enable a model in agent settings.".into(),
            since: "2026-01-01T00:00:00Z".into(),
            affected: Vec::new(),
            target: AttentionTarget::Settings {
                section: "agents".into(),
            },
        }],
    }
}

async fn listed(args: &[&str], attention: AttentionListDto) -> Output {
    let failed = TaskDto {
        id: "dismissed-task".into(),
        goal_id: "goal".into(),
        repo_id: "repo".into(),
        title: "Dismissed failure".into(),
        description: String::new(),
        status: TaskStatus::Failed,
        step: None,
        operation: None,
        agents: Vec::new(),
        depends_on: Vec::new(),
        branch: "task-branch".into(),
        worktree_path: None,
        stalled: false,
        merge_commit: None,
        pr_url: None,
        reason: None,
        usage: Default::default(),
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        host: "local".into(),
        host_unreachable: false,
    };
    let mut sessions: Vec<SessionEntryDto> = [
        ("dismissed-permission", "running"),
        ("dead-permission", "exited"),
    ]
    .into_iter()
    .map(|(id, status)| {
        serde_json::from_value(json!({
            "id": id,
            "agent_id": "stub",
            "goal_id": "goal",
            "status": status,
            "attention_reason": "waiting_permission",
            "attention_since": "2026-01-01T00:00:00Z",
        }))
        .unwrap()
    })
    .collect();
    sessions.push(
        serde_json::from_value(json!({
            "id": "unlisted-pull-request-session",
            "agent_id": "stub",
            "pull_request_id": "pull-request",
            "status": "exited",
            "attention_reason": "waiting_user",
        }))
        .unwrap(),
    );
    let app = Router::new()
        .route("/v1/goals", get(async || Json(json!([]))))
        .route("/v1/tasks", get(async move || Json(vec![failed])))
        .route(
            "/v1/sessions",
            get(async move || {
                Json(SessionPageDto {
                    total: sessions.len(),
                    sessions,
                    next_cursor: None,
                })
            }),
        )
        .route("/v1/attention", get(async move || Json(attention)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ariadne"))
        .env("ARIADNE_TELEMETRY", "off")
        .args([
            "--endpoint",
            &format!("http://{address}"),
            "--color",
            "never",
        ])
        .args(["--no-pager", "attention"])
        .args(args)
        .kill_on_drop(true)
        .output()
        .await;
    server.abort();
    output.expect("the CLI exits")
}

#[tokio::test]
async fn incomplete_json_contains_only_daemon_items_and_their_count() {
    let output = listed(&["--format", "json"], attention(false)).await;
    assert!(output.status.success(), "{:?}", output);
    let doc: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(doc["count"], 1);
    assert_eq!(doc["goals"], json!([]));
    assert_eq!(doc["recovery"], json!(attention(false).items));
    assert_eq!(doc["complete"], false);
}

#[tokio::test]
async fn incomplete_table_omits_dismissed_and_dead_rows_but_keeps_the_warning() {
    let output = listed(&[], attention(false)).await;
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Configure an available model"), "{stdout}");
    assert!(
        stdout.contains("ariadne attention dismiss visible-item"),
        "{stdout}"
    );
    assert!(!stdout.contains("dismissed-task"), "{stdout}");
    assert!(!stdout.contains("dismissed-permission"), "{stdout}");
    assert!(!stdout.contains("dead-permission"), "{stdout}");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("could not be read")
    );
}

#[tokio::test]
async fn incomplete_quiet_output_names_only_daemon_items_and_still_fails() {
    let output = listed(&["--quiet"], attention(false)).await;
    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "visible-item\n");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("could not be read")
    );
}

#[tokio::test]
async fn an_empty_incomplete_read_warns_without_restoring_raw_rows() {
    let output = listed(
        &[],
        AttentionListDto {
            items: Vec::new(),
            complete: false,
        },
    )
    .await;
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stdout.is_empty(), "{:?}", output);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("could not be read"), "{stderr}");
    assert!(!stderr.contains("Nothing needs attention"), "{stderr}");
}

#[tokio::test]
async fn complete_quiet_output_still_succeeds_with_daemon_item_ids() {
    let output = listed(&["--quiet"], attention(true)).await;
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "visible-item\n");
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}

#[tokio::test]
async fn empty_incomplete_quiet_output_fails_without_printing_raw_ids() {
    let output = listed(
        &["--quiet"],
        AttentionListDto {
            items: Vec::new(),
            complete: false,
        },
    )
    .await;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "{:?}", output);
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("could not be read")
    );
}

#[tokio::test]
async fn an_empty_complete_read_still_reports_nothing_needs_attention() {
    let output = listed(
        &[],
        AttentionListDto {
            items: Vec::new(),
            complete: true,
        },
    )
    .await;
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stdout.is_empty(), "{:?}", output);
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Nothing needs attention")
    );
}

#[tokio::test]
async fn selecting_only_ids_omits_attention_details_and_keeps_partial_warnings() {
    let mut list = attention(false);
    list.items[0].affected = vec![AttentionSubjectDto {
        kind: AttentionSubjectKind::Task,
        id: "affected-task".into(),
        label: "Affected task title".into(),
    }];
    let output = listed(&["--columns", "id"], list).await;
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "ID\nvisible-item\n"
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("could not be read")
    );
}

#[tokio::test]
async fn selected_attention_columns_follow_the_requested_order() {
    let mut list = attention(true);
    list.items[0].affected = ["First task", "Second task"]
        .into_iter()
        .map(|label| AttentionSubjectDto {
            kind: AttentionSubjectKind::Task,
            id: label.into(),
            label: label.into(),
        })
        .collect();
    let output = listed(&["--columns", "task,reason,title,id"], list).await;
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert_eq!(
        lines[0].split_whitespace().collect::<Vec<_>>(),
        ["TASK", "REASON", "TITLE", "ID"]
    );
    let cells = [
        "First task, Second task",
        "configuration",
        "Configure an available model",
        "visible-item",
    ];
    let positions: Vec<_> = cells
        .iter()
        .map(|cell| lines[1].find(cell).unwrap())
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{stdout}"
    );
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}

#[tokio::test]
async fn quiet_and_json_output_still_ignore_column_selection() {
    let quiet = listed(&["--quiet", "--columns", "colour"], attention(true)).await;
    assert!(quiet.status.success(), "{:?}", quiet);
    assert_eq!(String::from_utf8(quiet.stdout).unwrap(), "visible-item\n");

    let json = listed(
        &["--format", "json", "--columns", "colour"],
        attention(true),
    )
    .await;
    assert!(json.status.success(), "{:?}", json);
    let doc: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(doc["recovery"], json!(attention(true).items));
    assert_eq!(doc["count"], 1);
}

#[tokio::test]
async fn an_unknown_table_column_still_fails_without_output() {
    let output = listed(&["--columns", "colour"], attention(true)).await;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "{:?}", output);
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("no column \"colour\"")
    );
}
