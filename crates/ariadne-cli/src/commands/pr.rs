//! Pull request commands: the open requests of the enabled repositories,
//! read live off the forge and narrowed to the user's own or to the ones
//! that ask for their review (026, 029), as the desktop's Pull requests tab
//! narrows them. Ariadne keeps a request only while it works on it, and
//! nothing here adds or removes one: that is all automatic.
use std::time::{Duration, Instant};

use super::{
    DEFAULT, follow, parse_effort, parse_model, query_path,
    resolve::{self, Kind},
};
use crate::error::Failure;
use crate::output::{
    Column, Format, Kv, UNCAPPED, col, dash, moment, ok_id_line, print, print_kv, print_list, view,
    yes_no,
};
use anyhow::Result;
use ariadne_api::pull_requests::{PullRequestDto, PullRequestListQuery, PullRequestMatchDto};
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::sessions::{SessionPageDto, SessionPageQuery};
use ariadne_client::Client;
use clap::Subcommand;
use serde_json::json;

const COLUMNS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("repository", 26),
    col("number", UNCAPPED),
    col("title", 48).title(),
    col("branches", 32),
    col("author", 20),
    col("ariadne", 10),
    col("draft", 6),
    col("checks", 10),
    col("review decision", 20),
    col("unanswered comments", 10),
    col("updated", 24),
    col("session", UNCAPPED).id(),
];

/// How long `pr review --attach` waits for the scheduler to start the review
/// session it just woke, and how often it looks again.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(30);
const ATTACH_POLL: Duration = Duration::from_millis(500);
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
    /// Ask Ariadne to review a request of your own, or stop its review (029)
    Review {
        /// The repository, by id or path
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
        /// The request's number
        number: i64,
        /// What the review runs on: AGENT:MODEL — default: the repository's
        /// own review pin (`repo update --review-model`)
        #[arg(long, value_name = "MODEL", value_parser = parse_review_model, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// The reasoning effort that model is run at
        #[arg(long, value_name = "EFFORT", value_parser = parse_effort, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::efforts))]
        effort: Option<String>,
        /// A skill the review loads beside `pr-reviewer`; repeatable
        #[arg(long = "skill", value_name = "SKILL", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::skill_names))]
        skills: Vec<String>,
        /// Open the review session's console once it starts
        #[arg(long)]
        attach: bool,
        /// Stop Ariadne's review of the request
        #[arg(long, conflicts_with_all = ["model", "effort", "skills", "attach"])]
        stop: bool,
    },
}

/// `pr review --model`: the same spelling every `--model` takes, with
/// `default` refused — a review pins a model of its own, never "whatever the
/// agent runs".
fn parse_review_model(s: &str) -> std::result::Result<String, String> {
    if s == DEFAULT {
        return Err("`default` is no model — a model is required".into());
    }
    parse_model(s)
}

async fn repository(client: &Client, repo: Option<String>) -> Result<Option<String>> {
    match repo {
        Some(repo) => Ok(Some(resolve::id(client, Kind::Repo, &repo).await?)),
        None => Ok(None),
    }
}
/// "you" on a request of the user's own, the forge login otherwise.
fn author_label(r: &PullRequestDto) -> String {
    match r.role.as_str() {
        "author" => "you".to_string(),
        _ => r.author_login.clone(),
    }
}

/// `head → base`, as `pr ls` and `pr inspect` both name the branches.
fn branches_label(head: &str, base: &str) -> String {
    format!("{head} → {base}")
}

/// The skills a review loads beside `pr-reviewer`, or a dash where it loads
/// none of its own.
fn review_skills_label(skills: &[String]) -> String {
    match skills.is_empty() {
        true => "-".into(),
        false => skills.join(", "),
    }
}

/// What `pr inspect` prints: a hand-written block in a reading order, the
/// body last after a `---` line, the way `goal inspect` prints a
/// description.
fn inspect_rows(row: &PullRequestDto) -> Vec<(&'static str, Kv)> {
    vec![
        ("number", row.number.to_string().into()),
        ("title", Kv::title(row.title.clone())),
        ("repository", Kv::id(row.repository_id.clone())),
        ("author", author_label(row).into()),
        ("state", row.state.clone().into()),
        ("draft", yes_no(row.draft, "no").into()),
        (
            "branches",
            branches_label(&row.head_branch, &row.base_branch).into(),
        ),
        ("head", row.head_sha.clone().into()),
        ("checks", row.checks.clone().into()),
        ("review decision", row.review_decision.clone().into()),
        (
            "unanswered comments",
            row.unanswered_comments.to_string().into(),
        ),
        ("ariadne review", yes_no(row.review_asked, "no").into()),
        ("review model", dash(row.review_model.as_deref()).into()),
        (
            "review skills",
            review_skills_label(&row.review_skills).into(),
        ),
        ("session", Kv::id(dash(row.session_id.as_deref()))),
        ("kept by", Kv::id(dash(row.origin_task_id.as_deref()))),
        ("url", row.url.clone().into()),
        ("opened", Kv::meta(moment(&row.opened_at))),
        ("updated", Kv::meta(moment(&row.updated_at))),
        ("body", format!("\n---\n{}", row.body).into()),
    ]
}

fn inspect(row: &PullRequestDto, format: Format) -> Result<()> {
    print(format, row, || print_kv(&inspect_rows(row)))
}
async fn list(client: &Client, path: &str, format: Format) -> Result<()> {
    let rows: Vec<PullRequestDto> = client.get_json(path).await?;
    print_list(format, &rows, COLUMNS, ls_row, "No pull requests")
}

/// One row of `pr ls`.
fn ls_row(r: &PullRequestDto) -> Vec<String> {
    vec![
        r.id.clone().unwrap_or_else(|| "-".into()),
        r.repository_id.clone(),
        r.number.to_string(),
        r.title.clone(),
        branches_label(&r.head_branch, &r.base_branch),
        author_label(r),
        // What Ariadne does with it: keeps it for a task, reviews it, or
        // nothing.
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
        dash(r.session_id.as_deref()),
    ]
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
        PrCommand::Review {
            repo,
            number,
            model,
            effort,
            skills,
            attach,
            stop,
        } => {
            let repo_id = resolve::id(client, Kind::Repo, &repo).await?;
            let request = review_request(client, &repo_id, stop, model, effort, skills).await?;
            let pull: PullRequestDto = client
                .put_json(&review_path(&repo_id, number), &request)
                .await?;
            if attach {
                let pull_request_id = pull.id.clone().ok_or_else(|| {
                    anyhow::anyhow!("the request carries no id to attach a review to")
                })?;
                let session_id = wait_for_review_session(client, &pull_request_id).await?;
                return crate::commands::console::attach(client, &session_id).await;
            }
            // A row of nobody's own has no id to show; the number still
            // names the request a `--stop` on it answered about.
            let id = pull.id.clone().unwrap_or_else(|| pull.number.to_string());
            let verb = if stop { "stopped" } else { "asked" };
            print(format, &pull, || {
                println!("{}", ok_id_line(view().color, view().quiet, verb, &id))
            })
        }
    }
}

/// The body `pr review` sends: `{asked: false}` alone for `--stop` — no
/// pin and no skills travel with a request to stop, as the desktop's own
/// Stop review sends — else `{asked: true, model, effort, skills}` on the
/// pin the caller gave, or the repository's own `review_model` and
/// `review_effort` where they left `--model` out.
async fn review_request(
    client: &Client,
    repo_id: &str,
    stop: bool,
    model: Option<String>,
    effort: Option<String>,
    skills: Vec<String>,
) -> Result<serde_json::Value> {
    if stop {
        return Ok(json!({"asked": false}));
    }
    let (model, effort) = match model {
        Some(model) => (model, effort),
        None => default_review_pin(client, repo_id).await?,
    };
    Ok(json!({"asked": true, "model": model, "effort": effort, "skills": skills}))
}

/// The repository's own review pin (`repo update --review-model`), for a
/// `pr review` with no `--model` of its own.
async fn default_review_pin(client: &Client, repo_id: &str) -> Result<(String, Option<String>)> {
    let repo: RepositoryDto = client
        .get_json(&format!("/v1/repositories/{repo_id}"))
        .await?;
    repo.forge
        .and_then(|forge| forge.review_model.map(|model| (model, forge.review_effort)))
        .ok_or_else(|| {
            Failure::usage(
                "no --model was given, and this repository has no review model of its own \
                 — pass --model, or pin one with `ariadne repo update --review-model`",
            )
            .err()
        })
}

fn review_path(repo_id: &str, number: i64) -> String {
    format!("/v1/repositories/{repo_id}/pull-requests/{number}/ariadne-review")
}

/// `pr review --attach`: polls `GET /v1/sessions?pull_request=` until the
/// scheduler has started, or resumed, the review session it just woke, the
/// way `session new --attach` waits for none at all — a review session
/// comes from a later scheduler tick, never the route's own answer.
///
/// The query asks for live sessions alone: it carries no `all`, since a
/// session a `--stop` just before this took down still answers that filter
/// for a moment, and `all` would hand it back as though it were the new
/// one — this poll would then attach to a console already gone rather than
/// waiting for the one the ask on this line started.
async fn wait_for_review_session(client: &Client, pull_request_id: &str) -> Result<String> {
    poll_review_session(client, pull_request_id, ATTACH_TIMEOUT, ATTACH_POLL).await
}

async fn poll_review_session(
    client: &Client,
    pull_request_id: &str,
    timeout: Duration,
    poll: Duration,
) -> Result<String> {
    let deadline = Instant::now() + timeout;
    loop {
        let page: SessionPageDto = client
            .get_json(&query_path(
                "/v1/sessions",
                &SessionPageQuery {
                    pull_request: Some(pull_request_id.to_string()),
                    ..SessionPageQuery::default()
                },
            )?)
            .await?;
        if let Some(session) = page.sessions.into_iter().next() {
            return Ok(session.id);
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "no review session appeared for pull request {pull_request_id} within {timeout:?}"
            );
        }
        tokio::time::sleep(poll).await;
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
        routing::{get, post, put},
    };
    use std::sync::{Arc, Mutex};

    use ariadne_api::repositories::{ForgeDto, WebhookDto};

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

    /// `--stop` is a request of its own — `{asked: false}` alone — and
    /// conflicts with every flag that asks: a reader cannot send both in one
    /// line, and neither can the parser.
    #[test]
    fn pr_review_refuses_stop_combined_with_an_asking_flag_and_a_bare_default_model() {
        #[derive(clap::Parser)]
        struct Pr {
            #[command(subcommand)]
            command: PrCommand,
        }
        let parse = |argv: &[&str]| <Pr as clap::Parser>::try_parse_from(argv);
        assert!(parse(&["pr", "review", "repo-1", "42", "--model", "stub:m"]).is_ok());
        assert!(parse(&["pr", "review", "repo-1", "42", "--stop"]).is_ok());
        assert!(
            parse(&[
                "pr", "review", "repo-1", "42", "--stop", "--model", "stub:m"
            ])
            .is_err()
        );
        assert!(parse(&["pr", "review", "repo-1", "42", "--stop", "--effort", "high"]).is_err());
        assert!(
            parse(&[
                "pr",
                "review",
                "repo-1",
                "42",
                "--stop",
                "--skill",
                "code-review"
            ])
            .is_err()
        );
        assert!(parse(&["pr", "review", "repo-1", "42", "--stop", "--attach"]).is_err());
        assert!(
            parse(&["pr", "review", "repo-1", "42", "--model", "default"]).is_err(),
            "a review pins a model of its own, never \"whatever the agent runs\""
        );
    }

    /// `pr review --model a:m --effort high --skill code-review` sends the
    /// pin whole, and `--stop` sends `{asked: false}` with no pin and no
    /// skills: two different requests on the one route.
    #[tokio::test]
    async fn pr_review_sends_the_ask_body_with_its_pin_and_the_stop_body_alone() {
        let calls: Calls = Arc::default();
        let app = Router::new()
            .route("/v1/repositories", get(answer))
            .route(
                "/v1/repositories/repo-1/pull-requests/42/ariadne-review",
                put(answer),
            )
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::tcp(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        run(
            &client,
            PrCommand::Review {
                repo: "repo-1".into(),
                number: 42,
                model: Some("stub:test-model".into()),
                effort: Some("high".into()),
                skills: vec!["code-review".into()],
                attach: false,
                stop: false,
            },
            Format::Json,
        )
        .await
        .unwrap();
        run(
            &client,
            PrCommand::Review {
                repo: "repo-1".into(),
                number: 42,
                model: None,
                effort: None,
                skills: Vec::new(),
                attach: false,
                stop: true,
            },
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        let bodies: Vec<_> = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, path, _)| method == "PUT" && path.ends_with("/ariadne-review"))
            .map(|(_, _, body)| body.clone())
            .collect();
        assert_eq!(
            bodies,
            [
                json!({
                    "asked": true, "model": "stub:test-model", "effort": "high",
                    "skills": ["code-review"],
                }),
                json!({"asked": false}),
            ]
        );
    }

    /// A repository registered with its own review pin (`repo update
    /// --review-model`) supplies it to `pr review` run with no `--model` of
    /// its own.
    #[tokio::test]
    async fn pr_review_with_no_model_uses_the_repositorys_own_review_pin() {
        let repo = RepositoryDto {
            forge: Some(ForgeDto {
                webhook: WebhookDto {
                    state: "live".into(),
                    url: None,
                    error: None,
                    last_delivery_at: None,
                    fetch_error: None,
                },
                kind: ariadne_core::ForgeKind::Github,
                host: "github.com".into(),
                owner: "acme".into(),
                name: "widgets".into(),
                remote: "origin".into(),
                enabled: true,
                login: Some("octocat".into()),
                review_model: Some("stub:review-model".into()),
                review_effort: Some("balanced".into()),
            }),
            ..crate::commands::fixtures::repository("repo-1", "/work/widgets", "main")
        };
        let calls: Calls = Arc::default();
        let app = repo_router(repo).merge(
            Router::new()
                .route(
                    "/v1/repositories/repo-1/pull-requests/42/ariadne-review",
                    put(answer),
                )
                .with_state(calls.clone()),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::tcp(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        run(
            &client,
            PrCommand::Review {
                repo: "repo-1".into(),
                number: 42,
                model: None,
                effort: None,
                skills: Vec::new(),
                attach: false,
                stop: false,
            },
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        let bodies: Vec<_> = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, path, _)| method == "PUT" && path.ends_with("/ariadne-review"))
            .map(|(_, _, body)| body.clone())
            .collect();
        assert_eq!(
            bodies,
            [json!({
                "asked": true, "model": "stub:review-model", "effort": "balanced",
                "skills": [],
            })]
        );
    }

    /// A repository with no review pin of its own, and no `--model` on the
    /// line, is refused — naming the flag — rather than asking the daemon
    /// with nothing to pin a session on.
    #[tokio::test]
    async fn pr_review_with_no_model_and_no_repository_pin_is_refused() {
        let repo = crate::commands::fixtures::repository("repo-1", "/work/widgets", "main");
        let app = repo_router(repo);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::tcp(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let err = run(
            &client,
            PrCommand::Review {
                repo: "repo-1".into(),
                number: 42,
                model: None,
                effort: None,
                skills: Vec::new(),
                attach: false,
                stop: false,
            },
            Format::Json,
        )
        .await
        .unwrap_err();
        server.abort();

        assert!(err.to_string().contains("--model"), "{err}");
    }

    /// A minimal row of `GET /v1/sessions`'s page: the poll reads only the
    /// id off it.
    fn session_row(id: &str) -> ariadne_api::sessions::SessionEntryDto {
        ariadne_api::sessions::SessionEntryDto {
            kind: ariadne_api::sessions::SessionKind::Ariadne,
            id: id.into(),
            agent_id: "stub".into(),
            title: None,
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: None,
            effort: None,
            internal_session_id: None,
            working_directory: None,
            status: None,
            attention_reason: None,
            attention_since: None,
            last_activity_at: None,
            usage: None,
            context_used: None,
            context_size: None,
            created_at: None,
            ended_at: None,
            pull_request_id: None,
        }
    }

    #[derive(Clone)]
    struct SessionsStub {
        calls: Calls,
        /// How many pages were answered so far: the first answers empty, as
        /// a daemon whose only row is the session `--stop` just ended would
        /// — once the query asks for live sessions alone, never `all` — and
        /// a later one answers the row the scheduler started once it caught
        /// up.
        answered: Arc<Mutex<u32>>,
    }

    async fn stubbed_sessions(
        State(stub): State<SessionsStub>,
        request: Request,
    ) -> axum::response::Response {
        let uri = request.uri().to_string();
        stub.calls
            .lock()
            .unwrap()
            .push(("GET".into(), uri.clone(), serde_json::Value::Null));
        assert!(
            !uri.contains("all="),
            "the attach poll must ask for live sessions alone, never every one: {uri}"
        );
        let mut answered = stub.answered.lock().unwrap();
        *answered += 1;
        let sessions = if *answered < 2 {
            Vec::new()
        } else {
            vec![session_row("01LIVE")]
        };
        let total = sessions.len();
        axum::Json(SessionPageDto {
            sessions,
            next_cursor: None,
            total,
            snapshot_at: "2026-10-10T00:00:00Z".into(),
        })
        .into_response()
    }

    /// `--attach` polls past an empty page — the ended session a `--stop`
    /// just took down must not answer it, which the query's own shape
    /// (rather than this stub) is what keeps out — and attaches to the live
    /// one the scheduler starts once it catches up.
    #[tokio::test]
    async fn pr_review_attach_waits_past_an_empty_page_for_the_live_session() {
        let calls: Calls = Arc::default();
        let stub = SessionsStub {
            calls: calls.clone(),
            answered: Arc::default(),
        };
        let app = Router::new()
            .route("/v1/sessions", get(stubbed_sessions))
            .with_state(stub);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::tcp(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let id = poll_review_session(
            &client,
            "01PR",
            Duration::from_millis(500),
            Duration::from_millis(5),
        )
        .await
        .unwrap();
        server.abort();

        assert_eq!(id, "01LIVE");
        assert!(
            calls.lock().unwrap().len() >= 2,
            "the empty page must be polled past rather than answered as the live session"
        );
    }

    #[derive(Clone)]
    struct RepoState(Arc<RepositoryDto>);

    async fn repo_list(State(RepoState(repo)): State<RepoState>) -> axum::Json<Vec<RepositoryDto>> {
        axum::Json(vec![(*repo).clone()])
    }

    async fn repo_detail(State(RepoState(repo)): State<RepoState>) -> axum::Json<RepositoryDto> {
        axum::Json((*repo).clone())
    }

    /// `GET /v1/repositories` for `resolve::id`, and `GET
    /// /v1/repositories/repo-1` for the pin `pr review` falls back to.
    fn repo_router(repo: RepositoryDto) -> Router {
        Router::new()
            .route("/v1/repositories", get(repo_list))
            .route("/v1/repositories/repo-1", get(repo_detail))
            .with_state(RepoState(Arc::new(repo)))
    }

    /// `pr inspect` prints its fields in a fixed reading order, each key
    /// lowercase space-separated words, the body last after a `---` line —
    /// the way `goal inspect` prints its description.
    #[test]
    fn pr_inspect_prints_readable_keys_in_order_with_the_body_last() {
        let mut pull: PullRequestDto = serde_json::from_value(row()).unwrap();
        pull.session_id = Some("01SESS".into());
        pull.review_asked = true;
        pull.review_model = Some("stub:review-model".into());
        pull.review_skills = vec!["code-review".into()];
        pull.body = "What this changes.".into();

        let rows = inspect_rows(&pull);
        let keys: Vec<&str> = rows.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            keys,
            [
                "number",
                "title",
                "repository",
                "author",
                "state",
                "draft",
                "branches",
                "head",
                "checks",
                "review decision",
                "unanswered comments",
                "ariadne review",
                "review model",
                "review skills",
                "session",
                "kept by",
                "url",
                "opened",
                "updated",
                "body",
            ]
        );
        for key in &keys {
            assert!(!key.contains('_'), "{key} is not lowercase words");
        }

        let block = crate::output::kv_block(&rows, &crate::output::View::plain());
        assert!(block.contains("---"), "{block}");
        assert!(block.trim_end().ends_with("What this changes."), "{block}");
    }

    /// `pr ls` keeps its existing columns and adds the branches (`head →
    /// base`) and the request's session.
    #[test]
    fn pr_ls_shows_the_branches_and_session_columns() {
        let mut pull: PullRequestDto = serde_json::from_value(row()).unwrap();
        pull.session_id = Some("01SESS".into());

        let cells = ls_row(&pull);
        let at = |header: &str| COLUMNS.iter().position(|c| c.header == header).unwrap();
        assert_eq!(cells[at("branches")], "fix → main");
        assert_eq!(cells[at("session")], "01SESS");
    }
}
