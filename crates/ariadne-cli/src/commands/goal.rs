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

use super::{ProfileNames, confirm, parse_model, print_messages};
use crate::output::{
    Column, Format, UNCAPPED, local_time, print, print_kv, print_list, usage_block, usage_cell,
};
use super::query_path;

/// Columns of `goal ls`. `tokens` is what every agent of the goal spent
/// between them, in over an up arrow and out over a down one, with the share
/// of the input the prompt cache served; the roles it splits into, and the
/// counts to the digit, are in `goal inspect`.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("title", 48),
    ("status", UNCAPPED),
    ("approvals", UNCAPPED),
    ("tokens", UNCAPPED),
    ("repos", 40),
];

/// Where a continuation line of `goal inspect` starts: [`print_kv`] pads its
/// keys to the longest one — `description` — and then two spaces, and a block
/// that spills over several lines lines them all up under the first.
const INDENT: &str = "\n             ";

#[derive(Subcommand)]
pub enum GoalCommand {
    /// Create a goal
    Create {
        /// Short goal title (what the whole effort is called)
        #[arg(long)]
        title: String,
        /// Goal description (what should be achieved)
        #[arg(short = 'd', long, default_value = "")]
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
        /// Reviewer approvals required to merge a task
        #[arg(long)]
        approvals: Option<i64>,
        /// Maximum number of tasks (default: unbounded)
        #[arg(long)]
        max_tasks: Option<i64>,
    },
    /// List goals
    Ls {
        /// Filter by status, at the daemon; repeatable, and a goal in any of
        /// the named statuses is listed
        #[arg(long = "status", value_enum)]
        statuses: Vec<GoalStatus>,
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
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
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
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
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Show the goal-level conversation
    Messages {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
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
            approvals,
            max_tasks,
        } => {
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
                    },
                )
                .await?;
            print(format, &goal, || println!("{}", goal.id))?;
        }
        GoalCommand::Ls { statuses, no_trunc } => {
            let goals: Vec<GoalDto> = client.get_json(&goals_path(&statuses)?).await?;
            print_list(
                format,
                &goals,
                LS,
                no_trunc,
                |g| {
                    vec![
                        g.id.clone(),
                        g.title.clone(),
                        g.status.as_str().into(),
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
                match statuses.is_empty() {
                    true => "no goals yet — create one with: ariadne goal create",
                    false => "no goals match that filter",
                },
            )?;
        }
        GoalCommand::Inspect { id } => {
            let g: GoalDto = client.get_json(&goal_path(&id)).await?;
            let profiles = ProfileNames::for_format(client, format).await;
            print(format, &g, || {
                print_kv(&[
                    ("id", g.id.clone()),
                    ("title", g.title.clone()),
                    ("status", g.status.as_str().into()),
                    (
                        "planner",
                        profiles.pinned_label(&g.planner_profile_id, g.model.as_deref()),
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
                    ("created", local_time(&g.created_at)),
                    ("description", format!("\n---\n{}", g.description)),
                ])
            })?;
        }
        GoalCommand::Cancel { id, yes } => {
            let g: GoalDto = client.get_json(&goal_path(&id)).await?;
            confirm(&cancel_question(client, &g).await, yes)?;
            let g: GoalDto = client.post_empty(&format!("/v1/goals/{id}/cancel")).await?;
            print_status(&g, format)?;
        }
        GoalCommand::Rm { id, yes } => {
            let g: GoalDto = client.get_json(&goal_path(&id)).await?;
            // The daemon decides this too (and answers 409 if the goal moves
            // between these two calls); asking here is what turns the refusal
            // into the command that unblocks it.
            if !g.status.is_terminal() {
                bail!(
                    "goal {id} is {} — cancel it first: ariadne goal cancel {id}",
                    g.status.as_str()
                );
            }
            confirm(&rm_question(client, &g).await, yes)?;
            client
                .send_no_content::<()>(http::Method::DELETE, &goal_path(&id), None)
                .await?;
            // Nothing is left to print: what the caller asked about, and that
            // it happened.
            print(format, &json!({"goal": id, "deleted": true}), || {
                println!("deleted {id}")
            })?;
        }
        GoalCommand::Attach { id } => {
            crate::commands::attach::attach(client, &id, None).await?;
        }
        GoalCommand::Messages { id } => {
            let msgs: Vec<MessageDto> = client
                .get_json(&format!("/v1/goals/{id}/messages?limit=200"))
                .await?;
            print_messages(&msgs, format)?;
        }
        GoalCommand::Msg { id, body, to } => {
            let m: MessageDto = client
                .post_json(
                    &format!("/v1/goals/{id}/messages"),
                    &CreateMessageRequest { body, to },
                )
                .await?;
            print(format, &m, || println!("posted {}", m.id))?;
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

fn goal_path(id: &str) -> String {
    format!("/v1/goals/{id}")
}

/// What a goal-level transition prints: the goal it produced, or where it got
/// to.
fn print_status(g: &GoalDto, format: Format) -> Result<()> {
    print(format, g, || {
        println!("goal {} is now {}", g.id, g.status.as_str())
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
async fn cancel_question(client: &Client, goal: &GoalDto) -> String {
    let tasks = goal_tasks(client, &goal.id).await;
    let tail = match tasks.iter().filter(|t| !t.status.is_terminal()).count() {
        0 => "no task is still running".into(),
        1 => "1 live task will be cancelled too".into(),
        n => format!("{n} live tasks will be cancelled too"),
    };
    format!("Cancel goal \"{}\" ({tail})?", goal.title)
}

/// What `goal rm` asks before it deletes: the goal's tasks, messages and
/// review history go with it and none of it comes back, so the question names
/// how much history is about to be dropped.
async fn rm_question(client: &Client, goal: &GoalDto) -> String {
    let tail = match goal_tasks(client, &goal.id).await.len() {
        0 => "no tasks".into(),
        1 => "1 task".into(),
        n => format!("{n} tasks"),
    };
    format!(
        "Delete goal \"{}\" ({}) for good, with {tail} and their messages?",
        goal.title,
        goal.status.as_str()
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
        [] => {
            bail!("unknown repository \"{spec}\" — register it first with: ariadne repo add {spec}")
        }
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
                "input   12,345,000",
                "             cached  11,000,000  89%",
                "             output     456,000",
                "             planner    ↑345,000 ↓6,000",
                "             engineers  ↑10,000,000 ↓400,000",
                "             reviewers  ↑2,000,000 ↓50,000",
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
                "input   0",
                "             cached  0  0%",
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
    /// registering happens.
    #[test]
    fn an_unknown_repository_points_at_repo_add() {
        let err = pick_repository(&repos(), "/home/me/other").unwrap_err();
        assert!(
            err.to_string().contains("ariadne repo add /home/me/other"),
            "{err}"
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
