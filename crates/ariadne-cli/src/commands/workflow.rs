//! `ariadne workflow ...`
//!
//! A workflow is a document, so these are the things one does to a document:
//! list them, show one whole, write one, replace its text, put a shipped one
//! back, and delete one of your own — the same shape as `ariadne skill` —
//! plus `check`, which asks the daemon to read a draft without saving it
//! anywhere.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use serde_json::json;

use ariadne_api::workflows::{
    CreateWorkflowRequest, ParseWorkflowRequest, ParsedWorkflowDto, UpdateWorkflowRequest,
    WorkflowDto, WorkflowStepDto,
};
use ariadne_client::{Client, ClientError};

use super::{Subject, agent_label, confirm, path_segment};
use crate::error::Failure;
use crate::output::{
    Column, Format, UNCAPPED, age, col, dash, empty_state, ok_id_line, pager, print, print_json,
    print_list, print_table, view, yes_no,
};

/// Columns of `workflow ls`. A workflow is named and marked whether it is one
/// Ariadne ships, since that is what says whether it can be reset or
/// deleted; its columns say what it does, and its age says how stale that is.
const LS: &[Column] = &[
    col("name", 24).title(),
    col("builtin", UNCAPPED).rank(1),
    col("columns", UNCAPPED).rank(3),
    col("updated", UNCAPPED).rank(2),
];

/// Columns of `workflow check`: one column's worth of what the document said
/// about it.
const STEPS: &[Column] = &[
    col("id", 16).title(),
    col("title", 20),
    col("skills", UNCAPPED).rank(2),
    col("rank", UNCAPPED).rank(1),
    col("gate", UNCAPPED).rank(1),
];

#[derive(Subcommand)]
pub(crate) enum WorkflowCommand {
    /// List every workflow, shipped and written
    Ls,
    /// Show a workflow's document whole
    Show {
        /// Workflow name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::workflow_names))]
        name: String,
    },
    /// Write a workflow of your own
    ///
    /// The document is read from a file, or from stdin where none is named
    /// or where `-` names it.
    Create {
        /// Workflow name, kebab-case: how a goal or a task names it
        name: String,
        /// Read the document from this file; `-` or no file reads stdin
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Replace a workflow's document
    Update {
        /// Workflow name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::workflow_names))]
        name: String,
        /// Read the new document from this file; `-` or no file reads stdin
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Put a shipped workflow back on the document Ariadne ships
    Reset {
        /// Workflow name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::workflow_names))]
        name: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Delete a workflow of your own
    Rm {
        /// Workflow name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::workflow_names))]
        name: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Parse a document without saving it anywhere
    ///
    /// The document is read from a file, or from stdin where none is named
    /// or where `-` names it.
    Check {
        /// Read the document from this file; `-` or no file reads stdin
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

pub(crate) async fn run(client: &Client, cmd: WorkflowCommand, format: Format) -> Result<()> {
    match cmd {
        WorkflowCommand::Ls => {
            let workflows: Vec<WorkflowDto> = client.get_json("/v1/workflows").await?;
            let now = chrono::Utc::now();
            print_list(
                format,
                &workflows,
                LS,
                |w| {
                    vec![
                        w.name.clone(),
                        yes_no(w.builtin, "no"),
                        columns_label(&w.steps),
                        age(&w.updated_at, now),
                    ]
                },
                empty_state("No workflows are available.", Some("ariadne doctor")),
            )?;
        }
        WorkflowCommand::Show { name } => {
            let w = get_workflow(client, &name).await?;
            match format {
                Format::Json => print_json(&w)?,
                // Table and json differ here: json is the DTO whole, the
                // table is the document alone, through the pager the way a
                // long transcript is.
                Format::Table => pager::page(&w.document)?,
            }
        }
        WorkflowCommand::Create { name, file } => {
            // Read before anything is sent: a line naming an unreadable file
            // asks the daemon for nothing at all.
            let document = read_document(file)?;
            let w: WorkflowDto = client
                .post_json(
                    "/v1/workflows",
                    &CreateWorkflowRequest {
                        name: name.clone(),
                        document,
                    },
                )
                .await?;
            print(format, &w, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "created", &w.name)
                )
            })?;
        }
        WorkflowCommand::Update { name, file } => {
            let document = read_document(file)?;
            let w: WorkflowDto = client
                .put_json(
                    &workflow_path(&name),
                    &UpdateWorkflowRequest {
                        document: Some(document),
                    },
                )
                .await?;
            print(format, &w, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "updated", &w.name)
                )
            })?;
        }
        WorkflowCommand::Reset { name, yes } => {
            let w = get_workflow(client, &name).await?;
            let subject = Subject::new("workflow", &w.name, &w.name);
            confirm("reset", &subject, &reset_question(&w), yes)?;
            let w: WorkflowDto = client
                .post_empty(&format!("{}/reset", workflow_path(&name)))
                .await?;
            print(format, &w, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "reset", &w.name)
                )
            })?;
        }
        WorkflowCommand::Rm { name, yes } => {
            let w = get_workflow(client, &name).await?;
            let subject = Subject::new("workflow", &w.name, &w.name);
            confirm("delete", &subject, &rm_question(&w), yes)?;
            client
                .send_no_content::<()>(http::Method::DELETE, &workflow_path(&name), None)
                .await?;
            print(
                format,
                &json!({ "workflow": name, "deleted": true }),
                || {
                    println!(
                        "{}",
                        ok_id_line(view().color, view().quiet, "deleted", &name)
                    )
                },
            )?;
        }
        WorkflowCommand::Check { file } => {
            let document = read_document(file)?;
            let parsed = check(client, document).await?;
            match format {
                Format::Json => print_json(&parsed)?,
                Format::Table => {
                    let rows: Vec<Vec<String>> = parsed.steps.iter().map(step_row).collect();
                    print_table(STEPS, &rows)?;
                }
            }
        }
    }
    Ok(())
}

/// The path of one workflow, with the name escaped as a path segment.
fn workflow_path(name: &str) -> String {
    format!("/v1/workflows/{}", path_segment(name))
}

async fn get_workflow(client: &Client, name: &str) -> Result<WorkflowDto> {
    Ok(client.get_json(&workflow_path(name)).await?)
}

/// Parse a document at the daemon without saving it anywhere: its columns, or
/// a usage failure naming the line a syntax rule broke on.
///
/// `workflow_invalid` is the one daemon refusal this side reads apart rather
/// than printing whole: the line is in the envelope's `details`, not the
/// sentence beside it, and a person reading the refusal wants both together.
async fn check(client: &Client, document: String) -> Result<ParsedWorkflowDto> {
    match client
        .post_json::<ParsedWorkflowDto, _>(
            "/v1/workflows/parse",
            &ParseWorkflowRequest { document },
        )
        .await
    {
        Ok(parsed) => Ok(parsed),
        Err(ClientError::Api {
            code,
            message,
            details,
            ..
        }) if code == "workflow_invalid" => {
            let line = details
                .as_deref()
                .and_then(|d| d.get("line"))
                .and_then(serde_json::Value::as_u64);
            Err(match line {
                Some(line) => Failure::usage(format!("line {line}: {message}")).err(),
                None => Failure::usage(message).err(),
            })
        }
        Err(e) => Err(e.into()),
    }
}

/// One column of `workflow check`, as a table row.
fn step_row(s: &WorkflowStepDto) -> Vec<String> {
    vec![
        s.id.clone(),
        s.title.clone(),
        agent_label(&s.skills),
        dash(s.rank.map(|r| r.as_str())),
        dash(s.gate.map(|g| g.as_str())),
    ]
}

/// A workflow's columns as `workflow ls` shows them: the title of each, in
/// order, joined the way a path is.
fn columns_label(steps: &[WorkflowStepDto]) -> String {
    steps
        .iter()
        .map(|s| s.title.as_str())
        .collect::<Vec<_>>()
        .join(" → ")
}

/// The document a `create`, an `update` or a `check` reads: a file where one
/// was named, and stdin where none was or where `-` named it — but never a
/// terminal nobody piped anything into, which would hang with no sign of
/// why.
fn read_document(file: Option<PathBuf>) -> Result<String> {
    match file {
        Some(path) if path != std::path::Path::new("-") => {
            std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
        }
        _ => {
            if std::io::stdin().is_terminal() {
                bail!("nothing to read: pass --file <path>, or pipe the document in");
            }
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading the document from stdin")?;
            Ok(buf)
        }
    }
}

/// What `workflow reset` asks: the name alone does not say what is about to
/// be thrown away, so the question says it is the edited text.
fn reset_question(w: &WorkflowDto) -> String {
    format!(
        "Throw away the text written over the {} workflow and go back to the one \
         Ariadne ships?",
        w.name
    )
}

/// What `workflow rm` asks before it deletes.
fn rm_question(w: &WorkflowDto) -> String {
    format!("Delete the {} workflow?", w.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    use axum::Json as AxumJson;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;

    fn workflow(name: &str) -> WorkflowDto {
        WorkflowDto {
            steps: vec![
                WorkflowStepDto {
                    id: "develop".into(),
                    title: "Develop".into(),
                    description: String::new(),
                    skills: vec!["coding".into()],
                    rank: None,
                    gate: None,
                },
                WorkflowStepDto {
                    id: "review".into(),
                    title: "Review".into(),
                    description: String::new(),
                    skills: vec!["code-review".into()],
                    rank: None,
                    gate: None,
                },
            ],
            ..crate::commands::fixtures::workflow(name)
        }
    }

    #[test]
    fn the_workflow_subject_column_is_title() {
        let table = crate::output::render_table(
            LS,
            &[vec![String::new(); LS.len()]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.contains("NAME"), "{table}");
    }

    /// A workflow's columns read as the titles of its steps, joined the way a
    /// path reads.
    #[test]
    fn the_columns_label_joins_their_titles() {
        assert_eq!(
            columns_label(&workflow("develop-review-merge").steps),
            "Develop → Review"
        );
        assert_eq!(columns_label(&[]), "");
    }

    /// A built-in is marked the shared way every other boolean column is.
    #[test]
    fn a_listing_marks_a_built_in_with_the_shared_boolean_wording() {
        assert_eq!(yes_no(workflow("mine").builtin, "no"), "yes");
        let mut mine = workflow("mine");
        mine.builtin = false;
        assert_eq!(yes_no(mine.builtin, "no"), "no");
    }

    /// Both questions name the workflow: the last thing between the caller
    /// and a document that is about to go says which one it is.
    #[test]
    fn the_questions_name_the_workflow() {
        let w = workflow("develop-review-merge");
        assert_eq!(
            reset_question(&w),
            "Throw away the text written over the develop-review-merge workflow and go back \
             to the one Ariadne ships?"
        );
        assert_eq!(rm_question(&w), "Delete the develop-review-merge workflow?");
    }

    /// One column of `workflow check`, with a dash for the rank and the gate
    /// a column did not name.
    #[test]
    fn a_step_row_dashes_what_the_document_did_not_name() {
        let step = WorkflowStepDto {
            id: "develop".into(),
            title: "Develop".into(),
            description: String::new(),
            skills: vec!["coding".into()],
            rank: None,
            gate: None,
        };
        assert_eq!(step_row(&step), ["develop", "Develop", "coding", "-", "-"]);
    }

    /// A named file is read whole; `-` and no file both fall through to
    /// stdin rather than opening a file literally called `-`. Cargo runs
    /// tests with stdin closed, which `a_pipe_is_refused_rather_than_taken_for_a_yes`
    /// in `commands/mod.rs` relies on too — a closed stdin reads as empty
    /// rather than blocking.
    #[test]
    fn a_dash_reads_stdin_the_same_as_no_file_and_a_path_reads_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("w.md");
        std::fs::write(&path, "workflow mine\n  a[A]\n").expect("write");
        assert_eq!(
            read_document(Some(path)).expect("a file"),
            "workflow mine\n  a[A]\n"
        );

        assert_eq!(read_document(Some(PathBuf::from("-"))).expect("stdin"), "");
        assert_eq!(read_document(None).expect("stdin"), "");
    }

    /// A refusal's line lives in the envelope's `details`, not in the
    /// sentence beside it, so `check` reads both back together and exits as a
    /// usage failure rather than a daemon conflict.
    #[tokio::test]
    async fn check_prints_the_line_of_a_refusal_as_a_usage_error() {
        let captured: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
        let (client, _server) =
            serve(captured.clone(), Err((3, "unknown rank: urgent".into()))).await;

        let err = check(&client, "workflow x\n  a[A]\n    rank: urgent\n".into())
            .await
            .expect_err("an invalid document");
        assert_eq!(err.to_string(), "line 3: unknown rank: urgent");
        assert_eq!(crate::error::exit(&err), crate::error::Exit::Usage);
        assert_eq!(
            captured
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|v| v.get("document")),
            Some(&serde_json::json!("workflow x\n  a[A]\n    rank: urgent\n"))
        );
    }

    /// A good document comes back with its columns, read off the parse route
    /// rather than saved anywhere.
    #[tokio::test]
    async fn check_reads_back_the_columns_of_a_good_document() {
        let captured: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
        let (client, _server) = serve(captured.clone(), Ok(())).await;

        let parsed = check(&client, "workflow x\n  a[A]\n".into())
            .await
            .expect("a valid document");
        assert_eq!(parsed.name, "x");
        assert_eq!(parsed.steps[0].id, "a");
    }

    /// `check --file -` reads stdin end to end through `run`, the same as
    /// naming no file at all: it reaches the parse route whatever stdin held,
    /// rather than opening a file literally called `-`.
    #[tokio::test]
    async fn check_dash_reads_stdin_through_run() {
        let captured: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
        let (client, _server) = serve(captured.clone(), Ok(())).await;

        crate::output::init(crate::output::View::plain());
        run(
            &client,
            WorkflowCommand::Check {
                file: Some(PathBuf::from("-")),
            },
            Format::Json,
        )
        .await
        .expect("check");
        assert_eq!(
            captured
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|v| v.get("document")),
            Some(&serde_json::json!(""))
        );
    }

    /// `create` sends the document under a file, read before anything is
    /// sent; `update` sends it alone.
    #[tokio::test]
    async fn create_and_update_send_the_document_whole_and_nothing_else() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("w.md");
        std::fs::write(&path, "workflow mine\n  a[A]\n").expect("write");

        let captured: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
        let (client, _server) = serve_workflow(captured.clone()).await;

        crate::output::init(crate::output::View::plain());
        run(
            &client,
            WorkflowCommand::Create {
                name: "mine".into(),
                file: Some(path.clone()),
            },
            Format::Json,
        )
        .await
        .expect("create");
        let sent = captured.lock().unwrap().take().expect("a body was sent");
        assert_eq!(sent["name"], "mine");
        assert_eq!(sent["document"], "workflow mine\n  a[A]\n");

        run(
            &client,
            WorkflowCommand::Update {
                name: "mine".into(),
                file: Some(path),
            },
            Format::Json,
        )
        .await
        .expect("update");
        let sent = captured.lock().unwrap().take().expect("a body was sent");
        assert_eq!(
            sent,
            serde_json::json!({ "document": "workflow mine\n  a[A]\n" })
        );
    }

    /// A stub `/v1/workflows/parse`: either the parsed document's columns, or
    /// `workflow_invalid` naming the line the caller asked it to refuse at.
    async fn serve(
        captured: Arc<Mutex<Option<serde_json::Value>>>,
        answer: Result<(), (usize, String)>,
    ) -> (Client, tokio::task::JoinHandle<()>) {
        #[derive(Clone)]
        struct Stub {
            captured: Arc<Mutex<Option<serde_json::Value>>>,
            answer: Arc<Result<(), (usize, String)>>,
        }

        async fn parse(
            State(stub): State<Stub>,
            AxumJson(body): AxumJson<serde_json::Value>,
        ) -> (StatusCode, AxumJson<serde_json::Value>) {
            *stub.captured.lock().unwrap() = Some(body);
            match &*stub.answer {
                Ok(()) => (
                    StatusCode::OK,
                    AxumJson(serde_json::json!({"name": "x", "steps": [
                        {"id": "a", "title": "A", "description": "", "skills": [], "rank": null, "gate": null}
                    ]})),
                ),
                Err((line, message)) => (
                    StatusCode::BAD_REQUEST,
                    AxumJson(serde_json::json!({"error": {
                        "code": "workflow_invalid",
                        "message": message,
                        "details": {"line": line},
                    }})),
                ),
            }
        }

        let app = axum::Router::new()
            .route("/v1/workflows/parse", post(parse))
            .with_state(Stub {
                captured,
                answer: Arc::new(answer),
            });
        spawn(app).await
    }

    /// A stub for `POST /v1/workflows` and `PUT /v1/workflows/{name}`,
    /// capturing whatever body was last sent.
    async fn serve_workflow(
        captured: Arc<Mutex<Option<serde_json::Value>>>,
    ) -> (Client, tokio::task::JoinHandle<()>) {
        async fn create(
            State(captured): State<Arc<Mutex<Option<serde_json::Value>>>>,
            AxumJson(body): AxumJson<serde_json::Value>,
        ) -> (StatusCode, AxumJson<serde_json::Value>) {
            *captured.lock().unwrap() = Some(body.clone());
            (StatusCode::CREATED, AxumJson(dto(&body)))
        }

        async fn update(
            State(captured): State<Arc<Mutex<Option<serde_json::Value>>>>,
            AxumJson(body): AxumJson<serde_json::Value>,
        ) -> AxumJson<serde_json::Value> {
            *captured.lock().unwrap() = Some(body.clone());
            AxumJson(dto(&json!({"name": "mine", "document": body["document"]})))
        }

        fn dto(body: &serde_json::Value) -> serde_json::Value {
            serde_json::json!({
                "name": body["name"].as_str().unwrap_or("mine"),
                "document": body["document"],
                "builtin": false,
                "steps": [],
                "created_at": "2026-08-17T08:00:00Z",
                "updated_at": "2026-08-17T09:00:00Z",
            })
        }

        let app = axum::Router::new()
            .route("/v1/workflows", post(create))
            .route("/v1/workflows/mine", axum::routing::put(update))
            .with_state(captured);
        spawn(app).await
    }

    async fn spawn(app: axum::Router) -> (Client, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (Client::tcp(format!("http://{address}")), server)
    }
}
