//! `ariadne goal ...`

use anyhow::{Result, bail};
use clap::Subcommand;
use serde::Serialize;
use serde_json::json;

use ariadne_api::goals::{CreateGoalRequest, GoalDto};
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::{TaskDto, TaskListQuery};
use ariadne_client::Client;
use ariadne_core::GoalStatus;

use super::query_path;
use super::resolve::{self, Kind};
use super::{ProfileNames, Subject, confirm, parse_effort, parse_model, print_thread, recipient};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, UNCAPPED, age, col, moment, ok_id_line, print, print_kv, print_list,
    status_line, usage_block, usage_cell, view,
};

/// Columns of `goal ls`. `tokens` is what every agent of the goal spent
/// between them, in over an up arrow and out over a down one, with the share
/// of the input the prompt cache served; the roles it splits into, and the
/// counts to the digit, are in `goal inspect`.
///
/// What the goal is and where it got to stay whatever the terminal's width;
/// the repositories are the widest cell and the first to go.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 48).title(),
    col("status", UNCAPPED).status(),
    col("age", UNCAPPED).rank(3),
    col("approvals", UNCAPPED).rank(2),
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
  # a goal in one registered repository, planned on the Planner profile's own model
  ariadne goal create --title \"Add rate limiting\" --repo ~/projects/api

  # a planner of your own, on one model of one agent CLI, reasoned deeply
  ariadne goal create --title \"Add rate limiting\" --repo ~/projects/api \\
      --planner Architect --model codex:gpt-5.6-sol --effort xhigh

  # two repositories, and two reviewer approvals before a task may merge
  ariadne goal create --title \"Split the API\" --repo ~/projects/api \\
      --repo ~/projects/ui --approvals 2
";

#[derive(Subcommand)]
pub enum GoalCommand {
    /// Create a goal
    ///
    /// Names what is to be achieved and the registered repositories it is to
    /// be achieved in, and spawns the planner that breaks it into tasks.
    /// Nothing runs until you confirm the plan in the goal's conversation.
    /// Prints the new goal id.
    #[command(after_help = CREATE_EXAMPLES)]
    Create {
        /// Short goal title (what the whole effort is called)
        #[arg(long)]
        title: String,
        /// Goal description (what should be achieved)
        #[arg(short = 'd', long, default_value = "", hide_default_value = true)]
        description: String,
        /// Registered repository, by id or by the path it was added with
        /// (`ariadne repo add`); repeatable
        #[arg(long = "repo", required = true, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repos: Vec<String>,
        /// Planner profile id or name (default: the built-in Planner profile)
        #[arg(long, default_value = "Planner", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::planner_profiles))]
        planner: String,
        /// What the planner runs on: AGENT[:MODEL] — an agent CLI
        /// (claude_code | codex | opencode) on its own default model, or one
        /// model of it after the colon (codex:gpt-5.3-codex). Default: the
        /// planner profile's own
        #[arg(long, value_name = "MODEL", value_parser = parse_model, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// The reasoning effort that model is run at: one of the efforts
        /// `ariadne models ls` lists for it. Default: whatever the agent CLI
        /// runs it at
        #[arg(long, value_name = "EFFORT", value_parser = parse_effort, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::efforts))]
        effort: Option<String>,
        /// Reviewer approvals required to merge a task
        #[arg(long)]
        approvals: Option<i64>,
        /// Maximum number of tasks (default: unbounded)
        #[arg(long)]
        max_tasks: Option<i64>,
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
    },
    /// Show a goal
    Inspect {
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
    /// tmux sessions and worktrees, and `goal cancel` is what tears those
    /// down. What goes takes its tasks and messages with it, for good.
    Rm {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::deletable_goal_ids))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Show the goal-level conversation
    Thread {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
        /// Read this many messages from the start of the thread
        /// (default 200)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..), conflicts_with = "tail")]
        limit: Option<u32>,
        /// Read this many messages from the end of the thread instead
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        tail: Option<u32>,
    },
    /// Post a message into the goal-level conversation
    Msg {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
        /// Message body
        body: String,
        /// Address the message: the goal's planner, by profile id or name, or
        /// "user" to reach the human. An addressed recipient is woken to read
        /// it.
        #[arg(long, value_name = "PROFILE|user", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_message_recipients))]
        to: Option<String>,
    },
    /// Attach to the goal's planner tmux session
    Attach {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
    },
}

pub async fn run(client: &Client, cmd: GoalCommand, format: Format) -> Result<()> {
    match cmd {
        GoalCommand::Create {
            title,
            description,
            repos,
            planner,
            model,
            effort,
            approvals,
            max_tasks,
        } => {
            let planner = resolve::Profiles::new(client).id(&planner).await?;
            let goal: GoalDto = client
                .post_json(
                    "/v1/goals",
                    &CreateGoalRequest {
                        title,
                        description,
                        repository_ids: resolve_repositories(client, &repos).await?,
                        planner_profile: planner,
                        max_tasks,
                        required_approvals: approvals,
                        model,
                        effort,
                    },
                )
                .await?;
            print(format, &goal, || println!("{}", goal.id))?;
        }
        GoalCommand::Ls { statuses, all } => {
            let goals: Vec<GoalDto> = client.get_json(&goals_path(&statuses)?).await?;
            let goals = visible(goals, all, &statuses);
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
                        g.required_approvals.to_string(),
                        usage_cell(&g.usage.total),
                        g.repos
                            .iter()
                            .map(|r| r.path.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                    ]
                },
                // An empty list under a filter is not an empty system, and
                // telling the reader to create a goal would hide the ones that
                // are right there.
                match (statuses.is_empty(), all) {
                    (false, _) => "no goals match that filter",
                    (true, true) => "no goals yet — create one with: ariadne goal create",
                    (true, false) => "no goals under way — finished ones are behind --all",
                },
            )?;
        }
        GoalCommand::Inspect { id } => {
            let id = resolve::id(client, Kind::Goal, &id).await?;
            let g: GoalDto = client.get_json(&goal_path(&id)).await?;
            let profiles = ProfileNames::for_format(client, format).await;
            print(format, &g, || {
                print_kv(&[
                    ("id", g.id.clone()),
                    ("title", g.title.clone()),
                    ("status", g.status.as_str().into()),
                    (
                        "planner",
                        profiles.pinned_label(
                            &g.planner_profile_id,
                            g.model.as_deref(),
                            g.effort.as_deref(),
                        ),
                    ),
                    ("approvals", g.required_approvals.to_string()),
                    (
                        "max_tasks",
                        g.max_tasks.map_or("unbounded".into(), |m| m.to_string()),
                    ),
                    (
                        "repos",
                        g.repos
                            .iter()
                            .map(|r| format!("{} [{}] ({})", r.path, r.base_branch, r.id))
                            .collect::<Vec<_>>()
                            .join(INDENT),
                    ),
                    ("tokens", usage_lines(&g)),
                    ("created", moment(&g.created_at)),
                    ("description", format!("\n---\n{}", g.description)),
                ])
            })?;
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
                println!("{}", ok_id_line(view().color, "deleted", &id))
            })?;
        }
        GoalCommand::Attach { id } => {
            let id = resolve::id(client, Kind::Goal, &id).await?;
            crate::commands::attach::attach(client, &id, None).await?;
        }
        GoalCommand::Thread { id, limit, tail } => {
            let id = resolve::id(client, Kind::Goal, &id).await?;
            print_thread(
                client,
                &format!("/v1/goals/{id}/messages"),
                limit,
                tail,
                format,
            )
            .await?;
        }
        GoalCommand::Msg { id, body, to } => {
            let id = resolve::id(client, Kind::Goal, &id).await?;
            let to = recipient(client, to).await?;
            let m: MessageDto = client
                .post_json(
                    &format!("/v1/goals/{id}/messages"),
                    &CreateMessageRequest { body, to },
                )
                .await?;
            print(format, &m, || {
                println!("{}", ok_id_line(view().color, "posted", &m.id))
            })?;
        }
    }
    Ok(())
}

/// What the goal cost, role by role: every session of it summed, then the
/// planner, its engineers and its reviewers under that.
///
/// By role rather than by profile, the way [`GoalUsageDto`] groups it: a goal
/// has as many engineers as it has tasks, and at this height the question is
/// where the tokens went, not which agent went there. Each of the three lines
/// is always printed, `0` included — a role a goal has not spent on yet is a
/// figure, not a gap.
fn usage_lines(g: &GoalDto) -> String {
    let roles = [
        ("planner".to_string(), g.usage.planner),
        ("engineers".to_string(), g.usage.engineers),
        ("reviewers".to_string(), g.usage.reviewers),
    ];
    usage_block(&g.usage.total, &roles, INDENT)
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

/// What a goal-level transition prints: the goal it produced, or where it got
/// to.
fn print_status(g: &GoalDto, format: Format) -> Result<()> {
    print(format, g, || {
        println!(
            "{}",
            status_line(view().color, "goal", &g.id, g.status.as_str())
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

/// What `goal rm` asks before it deletes: the goal's tasks, messages and
/// review history go with it and none of it comes back, so the question names
/// how much history is about to be dropped.
async fn rm_question(client: &Client, goal: &GoalDto, subject: &Subject) -> String {
    let tail = match goal_tasks(client, &goal.id).await.len() {
        0 => "no tasks".into(),
        1 => "1 task".into(),
        n => format!("{n} tasks"),
    };
    format!(
        "Delete {} goal {} for good, with {tail} and their messages?",
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
async fn resolve_repositories(client: &Client, specs: &[String]) -> Result<Vec<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_api::goals::GoalUsageDto;
    use ariadne_api::usage::TokenUsageDto;

    use crate::commands::fixtures::{goal, repository};

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

    /// The total first, then where it went: a goal is read by role, since its
    /// engineers are as many as it has tasks.
    #[test]
    fn the_block_splits_the_goal_total_by_role() {
        let g = GoalDto {
            usage: GoalUsageDto {
                total: usage(12_345_000, 11_000_000, 456_000),
                planner: usage(345_000, 300_000, 6_000),
                engineers: usage(10_000_000, 9_000_000, 400_000),
                reviewers: usage(2_000_000, 1_700_000, 50_000),
            },
            ..goal("01GOAL", "Ship the board")
        };
        assert_eq!(
            usage_lines(&g),
            [
                "input    12M  89%",
                "             output  456k",
                "             planner    ↑345k ↓6k",
                "             engineers  ↑10M ↓400k",
                "             reviewers  ↑2M ↓50k",
            ]
            .join("\n")
        );
    }

    /// A goal nobody has run yet spent `0`, and every role says so: a role
    /// left out of the block would read as one the goal does not have.
    #[test]
    fn a_goal_that_has_spent_nothing_says_zero_for_every_role() {
        let g = goal("01GOAL", "Ship the board");
        assert_eq!(
            usage_lines(&g),
            [
                "input   0  0%",
                "             output  0",
                "             planner    ↑0 ↓0",
                "             engineers  ↑0 ↓0",
                "             reviewers  ↑0 ↓0",
            ]
            .join("\n")
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

    /// One checkout, two base branches: the path alone does not say which.
    #[test]
    fn a_path_on_several_base_branches_is_ambiguous() {
        let err = pick_repository(&repos(), "/home/me/ui").unwrap_err();
        assert!(err.to_string().contains("01REPOUINEXT"), "{err}");
        assert!(err.to_string().contains("by id"), "{err}");
    }
}
