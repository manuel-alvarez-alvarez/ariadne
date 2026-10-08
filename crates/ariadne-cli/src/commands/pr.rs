//! Pull request commands: the open requests of the enabled repositories,
//! read live off the forge and narrowed to the user's own or to the ones
//! that ask for their review (026, 029), as the desktop's Pull requests tab
//! narrows them. Ariadne keeps a request only while it works on it, and
//! nothing here adds or removes one: that is all automatic.
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
    col("author", 20),
    col("ariadne", 10),
    col("draft", 6),
    col("checks", 10),
    col("review decision", 20),
    col("unanswered comments", 10),
    col("updated", 24),
];
#[derive(Subcommand)]
pub(crate) enum PrCommand {
    /// List the open pull requests: every one, yours, or the ones that ask for your review
    Ls {
        #[arg(long)]
        repo: Option<String>,
        /// Only your own requests
        #[arg(long, conflicts_with = "review_requests")]
        mine: bool,
        /// Only the requests that ask for your review
        #[arg(long)]
        review_requests: bool,
        #[arg(long)]
        watch: bool,
    },
    /// Show every field of one open pull request, read off the forge now
    Inspect {
        /// The repository, by id or path
        repo: String,
        number: i64,
    },
    /// Search the open requests of an enabled repository that are not yours
    Search {
        #[arg(long)]
        repo: String,
        query: String,
    },
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
                r.id.clone().unwrap_or_else(|| "-".into()),
                r.repository_id.clone(),
                r.number.to_string(),
                r.title.clone(),
                match r.role.as_str() {
                    "author" => "you".to_string(),
                    _ => r.author_login.clone(),
                },
                // What Ariadne does with it: keeps it for a task, reviews it,
                // or nothing.
                match (&r.origin_task_id, r.id.is_some()) {
                    (Some(_), _) => "keeps".to_string(),
                    (None, true) => "reviews".to_string(),
                    (None, false) => "-".to_string(),
                },
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
            mine,
            review_requests,
            watch,
        } => {
            // Every open request with neither flag, as the desktop's All.
            let (role, requested) = match (mine, review_requests) {
                (true, _) => (Some("author".into()), None),
                (_, true) => (Some("reviewer".into()), Some(true)),
                _ => (None, None),
            };
            let query = PullRequestListQuery {
                repo: repository(client, repo).await?,
                role,
                task: None,
                requested,
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
        PrCommand::Inspect { repo, number } => {
            let repo = resolve::id(client, Kind::Repo, &repo).await?;
            inspect(
                &client
                    .get_json(&format!("/v1/repositories/{repo}/pull-requests/{number}"))
                    .await?,
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
                    col("tracked", 8),
                    col("url", 60),
                ],
                |r| {
                    vec![
                        r.number.to_string(),
                        r.title.clone(),
                        r.author_login.clone(),
                        r.tracked.to_string(),
                        r.url.clone(),
                    ]
                },
                "No matching pull requests",
            )
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
        json!({"id":"pull-42","repository_id":"repo-1","number":42,"url":"https://github.com/acme/widgets/pull/42","title":"Fix widgets","author_login":"me","role":"author","state":"open","draft":false,"head_branch":"fix","head_sha":"abc","head_repo":null,"base_branch":"main","checks":"none","review_decision":"none","unanswered_comments":0,"ready":false,"origin_task_id":null,"opened_at":"2026-10-01","updated_at":"2026-10-07"})
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
        } else if method == "GET" && !uri.ends_with("/pull-requests/42") {
            json!([row()])
        } else {
            row()
        };
        axum::Json(payload).into_response()
    }
    /// `--mine` and `--review-requests` are two narrowings of one list, so
    /// asking for both is refused rather than answered with nothing.
    #[test]
    fn mine_and_review_requests_do_not_combine() {
        #[derive(clap::Parser)]
        struct Pr {
            #[command(subcommand)]
            command: PrCommand,
        }
        let parse = |argv: &[&str]| <Pr as clap::Parser>::try_parse_from(argv);
        assert!(parse(&["pr", "ls", "--mine", "--review-requests"]).is_err());
        assert!(parse(&["pr", "ls", "--mine"]).is_ok());
        assert!(parse(&["pr", "ls", "--review-requests", "--all"]).is_err());
        assert!(parse(&["pr", "add", "https://github.com/acme/widgets/pull/1"]).is_err());
        assert!(parse(&["pr", "rm", "pull-42"]).is_err());
    }

    #[tokio::test]
    async fn pr_commands_use_the_live_routes_and_preserve_search_text() {
        let calls: Calls = Arc::default();
        let app = Router::new()
            .route("/v1/repositories", get(answer))
            .route("/v1/pull-requests", get(answer).post(answer))
            .route("/v1/pull-requests/{id}", get(answer))
            .route("/v1/repositories/repo-1/pull-requests/42", get(answer))
            .route("/v1/pull-requests/refresh", post(answer))
            .route("/v1/repositories/repo-1/pull-requests/search", get(answer))
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::tcp(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for command in [
            PrCommand::Ls {
                repo: Some("/work/widgets".into()),
                mine: false,
                review_requests: true,
                watch: false,
            },
            PrCommand::Ls {
                repo: None,
                mine: true,
                review_requests: false,
                watch: false,
            },
            PrCommand::Ls {
                repo: None,
                mine: false,
                review_requests: false,
                watch: false,
            },
            PrCommand::Inspect {
                repo: "repo-1".into(),
                number: 42,
            },
            PrCommand::Search {
                repo: "repo-1".into(),
                query: "fix & widgets".into(),
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
            && path == "/v1/pull-requests?repo=repo-1&role=reviewer&requested=true"));
        assert!(
            calls
                .iter()
                .any(|(method, path, _)| method == "GET" && path == "/v1/pull-requests?role=author")
        );
        assert!(
            calls
                .iter()
                .any(|(method, path, _)| method == "GET" && path == "/v1/pull-requests"),
            "neither flag lists every open request"
        );
        assert!(
            calls
                .iter()
                .any(|(_, path, _)| path == "/v1/repositories/repo-1/pull-requests/42")
        );
        assert!(
            calls.iter().any(|(_, path, _)| path
                == "/v1/repositories/repo-1/pull-requests/search?q=fix+%26+widgets")
        );
        assert!(
            calls
                .iter()
                .all(|(method, _, _)| method != "DELETE" && method != "PUT"),
            "nothing adds or removes a request by hand"
        );
        assert!(
            calls.iter().any(|(method, path, _)| method == "POST"
                && path == "/v1/pull-requests/refresh?repo=repo-1")
        );
    }
}
