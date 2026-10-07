//! Pull request ledger commands.
use super::{
    follow, query_path,
    resolve::{self, Kind},
};
use crate::output::{Column, Format, UNCAPPED, col, print, print_kv, print_list};
use anyhow::Result;
use ariadne_api::pull_requests::{PullRequestDto, PullRequestListQuery, PullRequestMatchDto};
use ariadne_client::Client;
use clap::Subcommand;
use serde_json::json;

const COLUMNS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("repository", 26),
    col("number", UNCAPPED),
    col("title", 48).title(),
    col("role", 12),
    col("tracked by", 10),
    col("draft", 6),
    col("checks", 10),
    col("review decision", 20),
    col("unanswered comments", 10),
    col("updated", 24),
];
#[derive(Subcommand)]
pub(crate) enum PrCommand {
    /// List tracked pull requests
    Ls {
        #[arg(long)]
        repo: Option<String>,
        #[arg(long, value_parser = ["author", "reviewer"])]
        role: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        watch: bool,
    },
    /// Show every field of a pull request
    Inspect { id: String },
    /// Search open requests of an enabled repository
    Search {
        #[arg(long)]
        repo: String,
        query: String,
    },
    /// Track a pull request by URL
    Add { url: String },
    /// Remove a request tracked by hand
    Rm { id: String },
    /// Fetch requests of one repository or all enabled repositories
    Refresh {
        #[arg(long)]
        repo: Option<String>,
    },
}

async fn repository(client: &Client, repo: Option<String>) -> Result<Option<String>> {
    match repo {
        Some(repo) => Ok(Some(resolve::id(client, Kind::Repo, &repo).await?)),
        None => Ok(None),
    }
}
fn inspect(row: &PullRequestDto, format: Format) -> Result<()> {
    let value = serde_json::to_value(row)?;
    let rows: Vec<_> = value
        .as_object()
        .expect("a DTO is an object")
        .iter()
        .map(|(key, value)| {
            (
                key.as_str(),
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string()),
            )
        })
        .collect();
    print(format, row, || print_kv(&rows))
}
async fn list(client: &Client, path: &str, format: Format) -> Result<()> {
    let rows: Vec<PullRequestDto> = client.get_json(path).await?;
    print_list(
        format,
        &rows,
        COLUMNS,
        |r| {
            vec![
                r.id.clone(),
                r.repository_id.clone(),
                r.number.to_string(),
                r.title.clone(),
                r.role.clone(),
                r.tracked_by.clone(),
                r.draft.to_string(),
                r.checks.clone(),
                r.review_decision.clone(),
                r.unanswered_comments.to_string(),
                r.updated_at.clone(),
            ]
        },
        "No pull requests",
    )
}
pub(crate) async fn run(client: &Client, command: PrCommand, format: Format) -> Result<()> {
    match command {
        PrCommand::Ls {
            repo,
            role,
            all,
            watch,
        } => {
            let query = PullRequestListQuery {
                repo: repository(client, repo).await?,
                role,
                state: all.then(|| "all".into()),
            };
            let path = query_path("/v1/pull-requests", &query)?;
            if watch {
                follow::watch(
                    client,
                    "/v1/events/stream",
                    |event| event.event.starts_with("pull_request_"),
                    async || list(client, &path, format).await,
                )
                .await
            } else {
                list(client, &path, format).await
            }
        }
        PrCommand::Inspect { id } => {
            let id = resolve::id(client, Kind::PullRequest, &id).await?;
            inspect(
                &client.get_json(&format!("/v1/pull-requests/{id}")).await?,
                format,
            )
        }
        PrCommand::Search { repo, query } => {
            let repo = resolve::id(client, Kind::Repo, &repo).await?;
            let path = query_path(
                &format!("/v1/repositories/{repo}/pull-requests/search"),
                &json!({"q": query}),
            )?;
            let rows: Vec<PullRequestMatchDto> = client.get_json(&path).await?;
            print_list(
                format,
                &rows,
                &[
                    col("number", UNCAPPED).id(),
                    col("title", 48).title(),
                    col("author", 24),
                    col("role", 12),
                    col("tracked", 8),
                    col("url", 60),
                ],
                |r| {
                    vec![
                        r.number.to_string(),
                        r.title.clone(),
                        r.author_login.clone(),
                        r.role.clone(),
                        r.tracked.to_string(),
                        r.url.clone(),
                    ]
                },
                "No matching pull requests",
            )
        }
        PrCommand::Add { url } => {
            let row: PullRequestDto = client
                .post_json("/v1/pull-requests", &json!({"url": url}))
                .await?;
            inspect(&row, format)
        }
        PrCommand::Rm { id } => {
            let id = resolve::id(client, Kind::PullRequest, &id).await?;
            client
                .send_no_content::<()>(
                    http::Method::DELETE,
                    &format!("/v1/pull-requests/{id}"),
                    None,
                )
                .await?;
            print(format, &json!({"id": id}), || println!("removed {id}"))
        }
        PrCommand::Refresh { repo } => {
            let path = query_path(
                "/v1/pull-requests/refresh",
                &json!({"repo": repository(client, repo).await?}),
            )?;
            client
                .send_no_content::<()>(http::Method::POST, &path, None)
                .await?;
            print(format, &json!({"requested": true}), || {
                println!("Refresh requested")
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::to_bytes,
        extract::{Request, State},
        response::IntoResponse,
        routing::{get, post},
    };
    use std::sync::{Arc, Mutex};

    fn row() -> serde_json::Value {
        json!({"id":"pull-42","repository_id":"repo-1","number":42,"url":"https://github.com/acme/widgets/pull/42","title":"Fix widgets","author_login":"me","role":"author","tracked_by":"user","state":"open","draft":false,"head_branch":"fix","head_sha":"abc","head_repo":null,"base_branch":"main","checks":"none","review_decision":"none","unanswered_comments":0,"ready":false,"origin_task_id":null,"opened_at":"2026-10-01","last_seen_at":"2026-10-07","created_at":"2026-10-01","updated_at":"2026-10-07"})
    }
    type Calls = Arc<Mutex<Vec<(String, String, serde_json::Value)>>>;
    async fn answer(State(calls): State<Calls>, request: Request) -> axum::response::Response {
        let method = request.method().to_string();
        let uri = request.uri().to_string();
        let body = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
        let body = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
        calls
            .lock()
            .unwrap()
            .push((method.clone(), uri.clone(), body));
        if method == "DELETE" {
            return http::StatusCode::NO_CONTENT.into_response();
        }
        if uri.starts_with("/v1/pull-requests/refresh") {
            return http::StatusCode::ACCEPTED.into_response();
        }
        let payload = if uri == "/v1/repositories" {
            json!([crate::commands::fixtures::repository(
                "repo-1",
                "/work/widgets",
                "main"
            )])
        } else if uri.contains("/search") {
            json!([{"number":42,"title":"Fix widgets","author_login":"me","role":"author","tracked":true,"url":"https://github.com/acme/widgets/pull/42"}])
        } else if method == "GET" && !uri.starts_with("/v1/pull-requests/pull-42") {
            json!([row()])
        } else {
            row()
        };
        axum::Json(payload).into_response()
    }
    #[tokio::test]
    async fn pr_commands_use_the_ledger_routes_and_preserve_search_text() {
        let calls: Calls = Arc::default();
        let app = Router::new()
            .route("/v1/repositories", get(answer))
            .route("/v1/pull-requests", get(answer).post(answer))
            .route("/v1/pull-requests/{id}", get(answer).delete(answer))
            .route("/v1/pull-requests/refresh", post(answer))
            .route("/v1/repositories/repo-1/pull-requests/search", get(answer))
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::tcp(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for command in [
            PrCommand::Ls {
                repo: Some("/work/widgets".into()),
                role: Some("author".into()),
                all: true,
                watch: false,
            },
            PrCommand::Inspect {
                id: "pull-42".into(),
            },
            PrCommand::Search {
                repo: "repo-1".into(),
                query: "fix & widgets".into(),
            },
            PrCommand::Add {
                url: "https://github.com/acme/widgets/pull/42".into(),
            },
            PrCommand::Rm {
                id: "pull-42".into(),
            },
            PrCommand::Refresh {
                repo: Some("repo-1".into()),
            },
        ] {
            run(&client, command, Format::Json).await.unwrap();
        }
        server.abort();
        let calls = calls.lock().unwrap();
        assert!(calls.iter().any(|(method, path, _)| method == "GET"
            && path == "/v1/pull-requests?repo=repo-1&role=author&state=all"));
        assert!(
            calls
                .iter()
                .any(|(_, path, _)| path == "/v1/pull-requests/pull-42")
        );
        assert!(
            calls.iter().any(|(_, path, _)| path
                == "/v1/repositories/repo-1/pull-requests/search?q=fix+%26+widgets")
        );
        assert!(calls.iter().any(|(method, path, body)| method == "POST"
            && path == "/v1/pull-requests"
            && body["url"] == "https://github.com/acme/widgets/pull/42"));
        assert!(
            calls
                .iter()
                .any(|(method, path, _)| method == "DELETE" && path == "/v1/pull-requests/pull-42")
        );
        assert!(
            calls.iter().any(|(method, path, _)| method == "POST"
                && path == "/v1/pull-requests/refresh?repo=repo-1")
        );
    }
}
