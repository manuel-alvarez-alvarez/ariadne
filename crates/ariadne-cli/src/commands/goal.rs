//! `ariadne goal ...`

use anyhow::{Result, bail};
use clap::Subcommand;
use serde::Serialize;
use serde_json::json;

use ariadne_api::goals::{CreateGoalRequest, GoalDto};
use ariadne_api::issues::IssueDto;
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::{TaskDto, TaskListQuery};
use ariadne_client::{Client, SseEvent};
use ariadne_core::{GoalStatus, Landing};

use super::follow;
use super::query_path;
use super::resolve::{self, Kind};
use super::{Subject, confirm, parse_effort, parse_model};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, Kv, UNCAPPED, age, col, empty_state, moment, ok_id_line, print, print_kv,
    print_list, status_line, usage_block, usage_cell, view,
};

/// Columns of `goal ls`. `tokens` is what every agent of the goal spent
/// between them, in over an up arrow and out over a down one, with the share
/// of the input the prompt cache served; the seats it splits into, and the
/// counts to the digit, are in `goal inspect`.
///
/// What the goal is and where it got to stay whatever the terminal's width;
/// the repositories are the widest cell and the first to go.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 48).title(),
    col("status", UNCAPPED).status(),
    col("age", UNCAPPED).rank(2),
    col("tokens", UNCAPPED).rank(1),
    col("repos", 40).rank(0),
];

/// Where a continuation line of `goal inspect` starts: [`print_kv`] pads its
/// keys to the longest one — `description` — and then two spaces, and a block
/// that spills over several lines lines them all up under the first.
const INDENT: &str = "\n             ";

/// What `goal create --help` ends with: a first goal, then the two things
/// most often said on the same line.
const CREATE_EXAMPLES: &str = "\
Examples:
  # a goal in one registered repository
  ariadne goal create --title \"Add rate limiting\" --repo ~/projects/api \\
      --model claude-acp:claude-sonnet-5

  # an orchestrator reasoned deeply
  ariadne goal create --title \"Add rate limiting\" --repo ~/projects/api \\
      --model codex-acp:gpt-5.6-sol --effort xhigh

  # a goal that works in two repositories
  ariadne goal create --title \"Split the API\" --repo ~/projects/api \\
      --repo ~/projects/ui --model claude-acp:claude-sonnet-5
";

#[derive(Subcommand)]
pub(crate) enum GoalCommand {
    /// Create a goal
    ///
    /// Names what is to be achieved and the registered repositories it is to
    /// be achieved in, and spawns the orchestrator that breaks it into tasks.
    /// Nothing runs until the orchestrator finalizes the plan. Prints the new
    /// goal id.
    #[command(after_help = CREATE_EXAMPLES)]
    Create {
        /// Short goal title (what the whole effort is called)
        #[arg(long)]
        title: Option<String>,
        /// Goal description (what should be achieved)
        #[arg(short = 'd', long)]
        description: Option<String>,
        /// Create the goal from a GitHub or GitLab issue URL
        #[arg(long)]
        from_issue: Option<String>,
        /// Registered repository, by id or by the path it was added with
        /// (`ariadne repo add`); repeatable
        #[arg(long = "repo", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repos: Vec<String>,
        /// What the orchestrator runs on: AGENT:MODEL — the id of an agent of
        /// the ACP registry and, after the colon, one model of it
        /// (codex-acp:gpt-5.3-codex). Required: a model is required, and no
        /// agent default stands in for one
        #[arg(long, value_name = "MODEL", value_parser = parse_model, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: String,
        /// The reasoning effort that model is run at: one of the efforts
        /// `ariadne models ls` lists for it. Default: whatever the agent runs
        /// it at
        #[arg(long, value_name = "EFFORT", value_parser = parse_effort, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::efforts))]
        effort: Option<String>,
        /// How every task of the goal ends: merge on the base branch,
        /// pull-request opened and pushed, none where there is nothing to
        /// land, or feature-branch. Fixed once the goal is created. Default:
        /// merge
        #[arg(long, value_enum)]
        landing: Option<Landing>,
    },
    /// List goals: the live ones, newest first (--all includes finished)
    Ls {
        /// Filter by status, at the daemon; repeatable and comma-separated,
        /// and a goal in any of the named statuses is listed. Names the
        /// statuses precisely, so it replaces the live/finished split --all
        /// makes
        #[arg(long = "status", value_parser = Spelling::<GoalStatus>::new(), value_delimiter = ',')]
        statuses: Vec<GoalStatus>,
        /// Include finished goals (completed/cancelled), not just live ones;
        /// nothing to add once --status names one
        #[arg(short, long)]
        all: bool,
        /// Redraw the table whenever a goal changes, until Ctrl-C
        #[arg(long)]
        watch: bool,
    },
    /// Show a goal
    Inspect {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
    },
    /// Close a goal whose tasks are all done
    ///
    /// The orchestrator calls this itself once the plan did what the goal
    /// asked for. This is the same call from the terminal, for a goal whose
    /// orchestrator is gone or will not start. Refused while any task is
    /// still going — `ariadne goal cancel` is what ends one of those.
    Complete {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
    },
    /// Cancel a goal and every task under it
    Cancel {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::cancellable_goal_ids))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Delete a finished goal and everything under it
    ///
    /// Only a completed or cancelled goal can go: an active one still owns
    /// agent sessions and worktrees, and `goal cancel` is what tears those
    /// down. What goes takes its tasks with it, for good.
    Rm {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::deletable_goal_ids))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Attach to the console of the goal's orchestrator
    Attach {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
    },
}

pub(crate) async fn run(client: &Client, cmd: GoalCommand, format: Format) -> Result<()> {
    match cmd {
        GoalCommand::Create {
            title,
            description,
            from_issue,
            repos,
            model,
            effort,
            landing,
        } => {
            let (issue, issue_repository) = match from_issue.as_deref() {
                Some(url) => {
                    let (repository, number) = issue_reference(client, url).await?;
                    let issue: IssueDto = client
                        .get_json(&format!("/v1/repositories/{repository}/issues/{number}"))
                        .await?;
                    (Some(issue), Some(repository))
                }
                None => (None, None),
            };
            let title = title
                .or_else(|| issue.as_ref().map(|issue| issue.title.clone()))
                .ok_or_else(|| anyhow::anyhow!("--title or --from-issue is required"))?;
            let description = description
                .or_else(|| issue.as_ref().map(|issue| issue.body.clone()))
                .unwrap_or_default();
            let repository_ids = if repos.is_empty() {
                vec![issue_repository.ok_or_else(|| anyhow::anyhow!("--repo is required"))?]
            } else {
                resolve_repositories(client, &repos).await?
            };
            let goal: GoalDto = client
                .post_json(
                    "/v1/goals",
                    &CreateGoalRequest {
                        workflow: None,
                        title,
                        description,
                        repository_ids,
                        issue_url: issue.map(|issue| issue.url),
                        model,
                        effort,
                        landing,
                    },
                )
                .await?;
            print(format, &goal, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "created", &goal.id)
                )
            })?;
        }
        GoalCommand::Ls {
            statuses,
            all,
            watch,
        } => ls(client, statuses, all, watch, format).await?,
        GoalCommand::Inspect { id } => {
            let id = resolve::id(client, Kind::Goal, &id).await?;
            let g: GoalDto = client.get_json(&goal_path(&id)).await?;
            print(format, &g, || {
                print_kv(&[
                    ("id", Kv::id(g.id.clone())),
                    ("title", Kv::title(g.title.clone())),
                    (
                        "issue",
                        g.issue_url.clone().unwrap_or_else(|| "-".into()).into(),
                    ),
                    ("status", Kv::status(g.status.as_str())),
                    ("landing", landing_row(&g)),
                    (
                        "orchestrator",
                        pin_label(&g.model, g.effort.as_deref()).into(),
                    ),
                    (
                        "repos",
                        g.repos
                            .iter()
                            .map(goal_repo_label)
                            .collect::<Vec<_>>()
                            .join(INDENT)
                            .into(),
                    ),
                    ("tokens", usage_lines(&g).into()),
                    ("created", Kv::meta(moment(&g.created_at))),
                    ("description", format!("\n---\n{}", g.description).into()),
                ])
            })?;
        }
        GoalCommand::Complete { id } => {
            let id = resolve::id(client, Kind::Goal, &id).await?;
            let g: GoalDto = client
                .post_json(&format!("/v1/goals/{id}/complete"), &serde_json::json!({}))
                .await?;
            print_status(&g, format)?;
        }
        GoalCommand::Cancel { id, yes } => {
            let id = resolve::id(client, Kind::Goal, &id).await?;
            let g: GoalDto = client.get_json(&goal_path(&id)).await?;
            let subject = Subject::new("goal", &g.title, &g.id);
            confirm(
                "cancel",
                &subject,
                &cancel_question(client, &g, &subject).await,
                yes,
            )?;
            let g: GoalDto = client.post_empty(&format!("/v1/goals/{id}/cancel")).await?;
            print_status(&g, format)?;
        }
        GoalCommand::Rm { id, yes } => {
            let id = resolve::id(client, Kind::Goal, &id).await?;
            let g: GoalDto = client.get_json(&goal_path(&id)).await?;
            // The daemon decides this too (and answers 409 if the goal moves
            // between these two calls); asking here is what turns the refusal
            // into the command that unblocks it.
            if !g.status.is_terminal() {
                return Err(crate::error::Failure::conflict(format!(
                    "goal {id} is {}",
                    g.status.as_str()
                ))
                .hint(format!("cancel it first: ariadne goal cancel {id}"))
                .err());
            }
            let subject = Subject::new("goal", &g.title, &g.id);
            confirm(
                "delete",
                &subject,
                &rm_question(client, &g, &subject).await,
                yes,
            )?;
            client
                .send_no_content::<()>(http::Method::DELETE, &goal_path(&id), None)
                .await?;
            // Nothing is left to print: what the caller asked about, and that
            // it happened.
            print(format, &json!({"goal": id, "deleted": true}), || {
                println!("{}", ok_id_line(view().color, view().quiet, "deleted", &id))
            })?;
        }
        GoalCommand::Attach { id } => {
            let id = resolve::id(client, Kind::Goal, &id).await?;
            crate::commands::attach::attach(client, &id, None).await?;
        }
    }
    Ok(())
}

/// Match an issue URL to one enabled registered repository and its issue number.
async fn issue_reference(client: &Client, url: &str) -> Result<(String, i64)> {
    let (_, rest) = url
        .split_once("://")
        .ok_or_else(|| anyhow::anyhow!("invalid issue URL"))?;
    let (host, path) = rest
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("invalid issue URL"))?;
    let (repository_path, number) = path
        .rsplit_once("/issues/")
        .ok_or_else(|| anyhow::anyhow!("invalid issue URL"))?;
    let number: i64 = number
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid issue number"))?;
    let repository_path = repository_path.trim_end_matches("/-");
    let repositories: Vec<RepositoryDto> = client.get_json("/v1/repositories").await?;
    let repository = repositories
        .into_iter()
        .find(|repository| {
            repository.forge.as_ref().is_some_and(|forge| {
                forge.enabled
                    && forge.host.eq_ignore_ascii_case(host)
                    && format!("{}/{}", forge.owner, forge.name)
                        .eq_ignore_ascii_case(repository_path)
            })
        })
        .ok_or_else(|| anyhow::anyhow!("no enabled repository matches {url}"))?;
    Ok((repository.id, number))
}

/// What the goal cost, seat by seat: every session of it summed, then the
/// orchestrator, its authors and its reviewers under that.
///
/// By seat rather than by profile, the way [`GoalUsageDto`] groups it: a goal
/// has as many authors as it has tasks, and at this height the question is
/// where the tokens went, not which agent went there. Each of the three lines
/// is always printed, `0` included — a seat a goal has not spent on yet is a
/// figure, not a gap.
fn usage_lines(g: &GoalDto) -> String {
    let seats = [
        ("orchestrator".to_string(), g.usage.orchestrator),
        ("authors".to_string(), g.usage.authors),
        ("reviewers".to_string(), g.usage.reviewers),
    ];
    usage_block(&g.usage.total, &seats, INDENT)
}

/// Which of the goals the daemon answered with `goal ls` shows: the ones
/// still under way, newest first, with everything behind --all.
///
/// The same default as `session ls`, and for the same reason: a list of every
/// goal there has ever been is a history, and what one asks a list for is
/// what is happening now. A named --status is that choice made precisely, so
/// it takes over — `--status completed` that then dropped every finished goal
/// would answer nothing.
///
/// Newest first because ids are ULIDs: the order they sort in is the order
/// they were created in, so the goal one is working on is the row at the top
/// rather than the row after the fiftieth.
fn visible(goals: Vec<GoalDto>, all: bool, statuses: &[GoalStatus]) -> Vec<GoalDto> {
    let mut goals: Vec<GoalDto> = goals
        .into_iter()
        .filter(|g| all || !statuses.is_empty() || !g.status.is_terminal())
        .collect();
    goals.sort_by(|a, b| b.id.cmp(&a.id));
    goals
}

fn goal_path(id: &str) -> String {
    format!("/v1/goals/{id}")
}

/// The events that change what `goal ls` shows.
fn relevant(frame: &SseEvent) -> bool {
    matches!(
        frame.event.as_str(),
        "goal_created" | "goal_updated" | "goal_deleted"
    )
}

/// `goal ls [--watch]`: the table, and with `--watch` the table again every
/// time a goal changes.
async fn ls(
    client: &Client,
    statuses: Vec<GoalStatus>,
    all: bool,
    watch: bool,
    format: Format,
) -> Result<()> {
    if !watch {
        return render(client, &statuses, all, format).await;
    }
    follow::watch(client, "/v1/events/stream", relevant, async || {
        render(client, &statuses, all, format).await
    })
    .await
}

/// The table as it stands, read afresh.
async fn render(client: &Client, statuses: &[GoalStatus], all: bool, format: Format) -> Result<()> {
    let goals: Vec<GoalDto> = client.get_json(&goals_path(statuses)?).await?;
    let goals = visible(goals, all, statuses);
    let now = chrono::Utc::now();
    print_list(
        format,
        &goals,
        LS,
        |g| {
            vec![
                g.id.clone(),
                g.title.clone(),
                g.status.as_str().into(),
                age(&g.created_at, now),
                usage_cell(&g.usage.total),
                g.repos
                    .iter()
                    .map(|r| r.path.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            ]
        },
        // An empty list under a filter is not an empty system, and telling the
        // reader to create a goal would hide the ones that are right there.
        match (statuses.is_empty(), all) {
            (false, _) => empty_state("No goals match that filter.", Some("ariadne goal ls")),
            (true, true) => empty_state("No goals yet.", Some("ariadne goal create --help")),
            (true, false) => empty_state("No goals are under way.", Some("ariadne goal ls --all")),
        },
    )
}

/// What a goal-level transition prints: the goal it produced, or where it got
/// to.
fn print_status(g: &GoalDto, format: Format) -> Result<()> {
    print(format, g, || {
        println!(
            "{}",
            status_line(view().color, view().quiet, "goal", &g.id, g.status.as_str())
        )
    })
}

/// The one filter `GET /v1/goals` takes: several statuses in a single
/// comma-separated `status=`, the way the goals board asks for them.
#[derive(Serialize)]
struct GoalListQuery {
    status: Option<String>,
}

/// `/v1/goals` untouched when no status was named, so a plain `ls` asks
/// exactly what it always did.
fn goals_path(statuses: &[GoalStatus]) -> Result<String> {
    let status = (!statuses.is_empty()).then(|| {
        statuses
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(",")
    });
    query_path("/v1/goals", &GoalListQuery { status })
}

/// What `goal cancel` asks before it fans out: cancelling is irreversible and
/// takes every task that has not finished with it, so the question names both.
async fn cancel_question(client: &Client, goal: &GoalDto, subject: &Subject) -> String {
    let tasks = goal_tasks(client, &goal.id).await;
    let tail = match tasks.iter().filter(|t| !t.status.is_terminal()).count() {
        0 => "no task is still running".into(),
        1 => "1 live task will be cancelled too".into(),
        n => format!("{n} live tasks will be cancelled too"),
    };
    format!("Cancel goal {} — {tail}?", subject.named())
}

/// What `goal rm` asks before it deletes: the goal's tasks and their review
/// history go with it and none of it comes back, so the question names how
/// much history is about to be dropped.
async fn rm_question(client: &Client, goal: &GoalDto, subject: &Subject) -> String {
    let tail = match goal_tasks(client, &goal.id).await.len() {
        0 => "no tasks".into(),
        1 => "1 task".into(),
        n => format!("{n} tasks"),
    };
    format!(
        "Delete {} goal {} for good, with {tail} and their reviews?",
        goal.status.as_str(),
        subject.named()
    )
}

/// The goal's tasks, as context for a question rather than as its answer: a
/// daemon that will not list them still gets asked about the goal.
async fn goal_tasks(client: &Client, goal_id: &str) -> Vec<TaskDto> {
    let query = TaskListQuery {
        goal: Some(goal_id.to_string()),
        status: None,
    };
    match query_path("/v1/tasks", &query) {
        Ok(path) => client.get_json(&path).await.unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// The ids the `--repo` arguments name, among the registered repositories:
/// nothing here registers one on the fly.
pub(super) async fn resolve_repositories(client: &Client, specs: &[String]) -> Result<Vec<String>> {
    let registered: Vec<RepositoryDto> = client.get_json("/v1/repositories").await?;
    specs
        .iter()
        .map(|spec| pick_repository(&registered, spec))
        .collect()
}

/// The repository a `--repo` argument names: by id, or by the absolute path it
/// was registered with. The same checkout can be registered once per base
/// branch, so a path that names several says so instead of picking one.
fn pick_repository(repos: &[RepositoryDto], spec: &str) -> Result<String> {
    if let Some(repo) = repos.iter().find(|r| r.id == spec) {
        return Ok(repo.id.clone());
    }
    let by_path: Vec<&RepositoryDto> = repos.iter().filter(|r| r.path == spec).collect();
    match by_path.as_slice() {
        [repo] => Ok(repo.id.clone()),
        [] => by_short_id(repos, spec),
        several => bail!(
            "{spec} is registered on several base branches ({}) — name the one you mean by id",
            several
                .iter()
                .map(|r| format!("{} = {}", r.base_branch, r.id))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The registered repository a short id names: the `…last8` every table and
/// the UI show, or the head of one. Only ids — a path that matches none of
/// them is not a typo to disambiguate but a repository nobody registered.
fn by_short_id(repos: &[RepositoryDto], spec: &str) -> Result<String> {
    let catalog = resolve::among(
        Kind::Repo,
        repos
            .iter()
            .map(|r| resolve::row(&r.id, format!("{} [{}]", r.path, r.base_branch))),
    );
    match catalog.pick(spec) {
        Ok(row) => Ok(row.id.clone()),
        Err(e) if crate::error::exit(&e) == crate::error::Exit::NotFound => Err(
            crate::error::Failure::not_found(format!("unknown repository \"{spec}\""))
                .hint(format!("register it first with: ariadne repo add {spec}"))
                .err(),
        ),
        Err(e) => Err(e),
    }
}

/// What a goal's orchestrator runs on: the model, and the effort where one was
/// pinned.
///
/// An effort that was never pinned says nothing at all: the model is run at
/// whatever its agent runs it at, and a `@` with a guess after it would
/// read as a choice somebody made.
fn pin_label(model: &str, effort: Option<&str>) -> String {
    match effort {
        Some(effort) => format!("{model} @ {effort}"),
        None => model.to_string(),
    }
}

fn goal_repo_label(repo: &ariadne_api::goals::GoalRepositoryDto) -> String {
    match repo.goal_branch.as_deref() {
        Some(branch) => format!(
            "{} [{} → {}] ({})",
            repo.path, repo.base_branch, branch, repo.id
        ),
        None => format!("{} [{}] ({})", repo.path, repo.base_branch, repo.id),
    }
}

fn landing_row(goal: &GoalDto) -> Kv {
    goal.landing.as_str().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_api::goals::GoalUsageDto;
    use ariadne_api::usage::TokenUsageDto;

    use crate::commands::fixtures::{goal, repository};

    #[tokio::test]
    async fn create_from_issue_reads_its_title_and_body_through_the_route() {
        use axum::{
            Json, Router,
            routing::{get, post},
        };
        use std::sync::{Arc, Mutex};

        let request = Arc::new(Mutex::new(None::<CreateGoalRequest>));
        let saved = request.clone();
        let repositories = get(|| async {
            let mut repository = repository("repo-1", "/work/widgets", "main");
            repository.forge = Some(ariadne_api::repositories::ForgeDto {
                webhook: ariadne_api::repositories::WebhookDto {
                    state: "polling".into(),
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
                review_model: None,
                review_effort: None,
            });
            Json(vec![repository])
        });
        let issue = get(|| async {
            Json(IssueDto {
                number: 12,
                title: "Fix widget".into(),
                body: "The widget fails.".into(),
                url: "https://github.com/acme/widgets/issues/12".into(),
                labels: vec![],
                assignees: vec![],
                updated_at: "2026-10-01T00:00:00Z".into(),
            })
        });
        let app = Router::new()
            .route("/v1/repositories", repositories)
            .route("/v1/repositories/repo-1/issues/12", issue)
            .route(
                "/v1/goals",
                post(move |Json(body): Json<CreateGoalRequest>| {
                    let saved = saved.clone();
                    async move {
                        *saved.lock().unwrap() = Some(body);
                        Json(goal("goal-1", "Fix widget"))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = run(
            &Client::tcp(format!("http://{address}")),
            GoalCommand::Create {
                title: None,
                description: None,
                from_issue: Some("https://github.com/acme/widgets/issues/12".into()),
                repos: vec![],
                model: "stub:test-model".into(),
                effort: None,
                landing: None,
            },
            Format::Json,
        )
        .await;
        server.abort();
        result.unwrap();
        let body = request.lock().unwrap().take().unwrap();
        assert_eq!(body.title, "Fix widget");
        assert_eq!(body.description, "The widget fails.");
        assert_eq!(
            body.issue_url.as_deref(),
            Some("https://github.com/acme/widgets/issues/12")
        );
        assert_eq!(body.repository_ids, ["repo-1"]);
    }

    fn repos() -> Vec<RepositoryDto> {
        vec![
            repository("01REPOAPI", "/home/me/api", "main"),
            repository("01REPOUI", "/home/me/ui", "main"),
            repository("01REPOUINEXT", "/home/me/ui", "next"),
        ]
    }

    fn usage(input: u64, cached: u64, output: u64) -> TokenUsageDto {
        TokenUsageDto {
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
        }
    }

    /// The total first, then where it went: a goal is read by seat, since its
    /// authors are as many as it has tasks.
    #[test]
    fn the_block_splits_the_goal_total_by_seat() {
        let g = GoalDto {
            issue_url: None,
            usage: GoalUsageDto {
                agents: Vec::new(),
                total: usage(12_345_000, 11_000_000, 456_000),
                orchestrator: usage(345_000, 300_000, 6_000),
                authors: usage(10_000_000, 9_000_000, 400_000),
                reviewers: usage(2_000_000, 1_700_000, 50_000),
            },
            ..goal("01GOAL", "Ship the board")
        };
        assert_eq!(
            usage_lines(&g),
            [
                "input    12M  89.1%",
                "             output  456k",
                "             orchestrator  ↑345k ↓6k",
                "             authors       ↑10M ↓400k",
                "             reviewers     ↑2M ↓50k",
            ]
            .join("\n")
        );
    }

    /// A goal nobody has run yet spent `0`, and every seat says so: a seat
    /// left out of the block would read as one the goal does not have.
    #[test]
    fn a_goal_that_has_spent_nothing_says_zero_for_every_seat() {
        let g = goal("01GOAL", "Ship the board");
        assert_eq!(
            usage_lines(&g),
            [
                "input   0  0.0%",
                "             output  0",
                "             orchestrator  ↑0 ↓0",
                "             authors       ↑0 ↓0",
                "             reviewers     ↑0 ↓0",
            ]
            .join("\n")
        );
    }

    #[test]
    fn goal_inspect_shows_goal_branches_only_when_set() {
        use ariadne_api::goals::GoalRepositoryDto;

        let base = repository("01REPO", "/home/me/api", "main");
        let feature = GoalRepositoryDto {
            repository: base.clone(),
            goal_branch: Some("goal/ship-board".into()),
        };
        let regular = GoalRepositoryDto {
            repository: base,
            goal_branch: None,
        };

        assert_eq!(
            goal_repo_label(&feature),
            "/home/me/api [main → goal/ship-board] (01REPO)"
        );
        assert_eq!(goal_repo_label(&regular), "/home/me/api [main] (01REPO)");
    }

    #[test]
    fn goal_inspect_prints_its_landing() {
        let g = GoalDto {
            issue_url: None,
            landing: Landing::FeatureBranch,
            ..goal("01GOAL", "Ship the board")
        };

        assert_eq!(
            crate::output::kv_block(
                &[("landing", landing_row(&g))],
                &crate::output::View::plain()
            ),
            "landing  feature_branch"
        );
    }

    #[test]
    fn a_repository_is_named_by_id_or_by_path() {
        assert_eq!(pick_repository(&repos(), "01REPOAPI").unwrap(), "01REPOAPI");
        assert_eq!(
            pick_repository(&repos(), "/home/me/api").unwrap(),
            "01REPOAPI"
        );
    }

    /// Nothing is registered on the fly any more, so the refusal says where
    /// registering happens — and it is a missing thing, which exits 4.
    #[test]
    fn an_unknown_repository_points_at_repo_add() {
        let err = pick_repository(&repos(), "/home/me/other").unwrap_err();
        assert!(
            crate::error::human_line(&err).contains("ariadne repo add /home/me/other"),
            "{err}"
        );
        assert_eq!(crate::error::exit(&err), crate::error::Exit::NotFound);
    }

    /// A repository is named by the same short spellings as everything else:
    /// the `…last8` the tables print, or the head of an id.
    #[test]
    fn a_repository_is_named_by_a_short_id_too() {
        let repos = [repository(
            "01m0repo00000000000000abcd",
            "/home/me/api",
            "main",
        )];
        assert_eq!(
            pick_repository(&repos, "000abcd").unwrap(),
            "01m0repo00000000000000abcd"
        );
        assert_eq!(
            pick_repository(&repos, "01M0REPO").unwrap(),
            "01m0repo00000000000000abcd"
        );
    }

    /// No `--status` asks what `goal ls` always asked: the plain list, with
    /// no stray query on it.
    #[test]
    fn no_status_leaves_the_goals_path_alone() {
        assert_eq!(goals_path(&[]).unwrap(), "/v1/goals");
    }

    #[test]
    fn a_status_is_asked_for_in_its_wire_spelling() {
        assert_eq!(
            goals_path(&[GoalStatus::Planning]).unwrap(),
            "/v1/goals?status=planning"
        );
    }

    /// Several statuses ride in the one comma-separated `status=` the daemon
    /// takes — the same request the goals board makes.
    #[test]
    fn several_statuses_ride_in_one_comma_separated_parameter() {
        assert_eq!(
            goals_path(&[GoalStatus::Active, GoalStatus::Completed]).unwrap(),
            "/v1/goals?status=active%2Ccompleted"
        );
    }

    /// What `goal rm` asks about is everything that goes with the goal: its
    /// tasks and their reviews, which is the whole of a goal's history now.
    /// The question is the last thing a person reads before something
    /// irreversible, so its words are pinned rather than left to drift.
    #[tokio::test]
    async fn the_delete_question_names_what_goes_with_the_goal() {
        // Nothing answers, so the task count falls back to none: what is
        // pinned here is the sentence, not the listing behind it.
        let client = Client::resolve(Some("http://127.0.0.1:1"), None);
        let g = GoalDto {
            issue_url: None,
            status: GoalStatus::Cancelled,
            ..goal("01m15hg1d4j6de91a4amkhsfgt", "Ship the board")
        };
        let subject = Subject::new("goal", &g.title, &g.id);
        assert_eq!(
            rm_question(&client, &g, &subject).await,
            "Delete cancelled goal \"Ship the board\" (…amkhsfgt) for good, \
             with no tasks and their reviews?"
        );
    }

    /// One checkout, two base branches: the path alone does not say which.
    #[test]
    fn a_path_on_several_base_branches_is_ambiguous() {
        let err = pick_repository(&repos(), "/home/me/ui").unwrap_err();
        assert!(err.to_string().contains("01REPOUINEXT"), "{err}");
        assert!(err.to_string().contains("by id"), "{err}");
    }
}
