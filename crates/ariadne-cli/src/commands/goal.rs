//! `ariadne goal ...`

use anyhow::{Result, bail};
use clap::Subcommand;
use serde::Serialize;
use serde_json::json;

use ariadne_api::goals::{CreateGoalRequest, FinalizePlanRequest, GoalDto};
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::tasks::{TaskDto, TaskListQuery};
use ariadne_client::Client;
use ariadne_core::GoalStatus;

use super::{ProfileNames, confirm};
use crate::output::{
    Column, Format, UNCAPPED, local_time, note, print_json, print_kv, print_table,
};
use crate::query::query_path;

/// Columns of `goal ls`.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("title", 48),
    ("status", UNCAPPED),
    ("approvals", UNCAPPED),
    ("repos", 40),
];

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
    /// Finalize a goal's plan: planning ends and its tasks start running
    ///
    /// What the planner's `finalize_plan` does, from the terminal: the goal
    /// leaves `planning` for `active`, and every task whose dependencies are
    /// met is handed to an engineer. The goal needs at least one task.
    Finalize {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
        /// The plan, in a sentence: recorded in the goal thread, which is
        /// what the agents read as the brief they were finalized on
        #[arg(short, long, default_value = "approved by the user")]
        summary: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
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
                    },
                )
                .await?;
            match format {
                Format::Json => print_json(&goal)?,
                Format::Table => println!("{}", goal.id),
            }
        }
        GoalCommand::Ls { statuses, no_trunc } => {
            let goals: Vec<GoalDto> = client.get_json(&goals_path(&statuses)?).await?;
            match format {
                Format::Json => print_json(&goals)?,
                Format::Table => {
                    print_table(
                        LS,
                        &goals
                            .iter()
                            .map(|g| {
                                vec![
                                    g.id.clone(),
                                    g.title.clone(),
                                    g.status.as_str().into(),
                                    g.required_approvals.to_string(),
                                    g.repos
                                        .iter()
                                        .map(|r| r.path.as_str())
                                        .collect::<Vec<_>>()
                                        .join(","),
                                ]
                            })
                            .collect::<Vec<_>>(),
                        no_trunc,
                    );
                    if goals.is_empty() {
                        // An empty list under a filter is not an empty
                        // system, and telling the reader to create a goal
                        // would hide the ones that are right there.
                        note(if statuses.is_empty() {
                            "no goals yet — create one with: ariadne goal create"
                        } else {
                            "no goals match that filter"
                        });
                    }
                }
            }
        }
        GoalCommand::Inspect { id } => {
            let g: GoalDto = client.get_json(&format!("/v1/goals/{id}")).await?;
            match format {
                Format::Json => print_json(&g)?,
                Format::Table => {
                    let profiles = ProfileNames::fetch(client).await;
                    print_kv(&[
                        ("id", g.id),
                        ("title", g.title),
                        ("status", g.status.as_str().into()),
                        ("planner", profiles.label(&g.planner_profile_id)),
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
                                .join("\n           "),
                        ),
                        ("created", local_time(&g.created_at)),
                        ("description", format!("\n---\n{}", g.description)),
                    ]);
                }
            }
        }
        GoalCommand::Finalize { id, summary, yes } => {
            let g: GoalDto = client.get_json(&format!("/v1/goals/{id}")).await?;
            confirm(&finalize_question(client, &g).await, yes)?;
            let g: GoalDto = client
                .post_json(
                    &format!("/v1/goals/{id}/finalize"),
                    &FinalizePlanRequest { summary },
                )
                .await?;
            match format {
                Format::Json => print_json(&g)?,
                Format::Table => println!("goal {} is now {}", g.id, g.status.as_str()),
            }
        }
        GoalCommand::Cancel { id, yes } => {
            let g: GoalDto = client.get_json(&format!("/v1/goals/{id}")).await?;
            confirm(&cancel_question(client, &g).await, yes)?;
            let g: GoalDto = client.post_empty(&format!("/v1/goals/{id}/cancel")).await?;
            match format {
                Format::Json => print_json(&g)?,
                Format::Table => println!("goal {} is now {}", g.id, g.status.as_str()),
            }
        }
        GoalCommand::Rm { id, yes } => {
            let g: GoalDto = client.get_json(&format!("/v1/goals/{id}")).await?;
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
                .send_no_content::<()>(http::Method::DELETE, &format!("/v1/goals/{id}"), None)
                .await?;
            match format {
                // Nothing is left to print: what the caller asked about, and
                // that it happened.
                Format::Json => print_json(&json!({"goal": id, "deleted": true}))?,
                Format::Table => println!("deleted {id}"),
            }
        }
        GoalCommand::Attach { id } => {
            crate::commands::attach::attach(client, &id, None).await?;
        }
        GoalCommand::Messages { id } => {
            let msgs: Vec<MessageDto> = client
                .get_json(&format!("/v1/goals/{id}/messages?limit=200"))
                .await?;
            match format {
                Format::Json => print_json(&msgs)?,
                Format::Table => {
                    for m in &msgs {
                        println!(
                            "[{}] {}: {}",
                            local_time(&m.created_at),
                            m.author_role.as_str(),
                            m.body
                        );
                    }
                    if msgs.is_empty() {
                        note("no messages yet");
                    }
                }
            }
        }
        GoalCommand::Msg { id, body } => {
            let m: MessageDto = client
                .post_json(
                    &format!("/v1/goals/{id}/messages"),
                    &CreateMessageRequest { body },
                )
                .await?;
            match format {
                Format::Json => print_json(&m)?,
                Format::Table => println!("posted {}", m.id),
            }
        }
    }
    Ok(())
}

/// The one filter `GET /v1/goals` takes: the statuses to match, as the daemon
/// spells them — several of them in a single comma-separated `status=`, the
/// way the goals board asks for them.
#[derive(Serialize)]
struct GoalListQuery {
    status: Option<String>,
}

/// The path `goal ls` reads: `/v1/goals` untouched when no status was named,
/// so a plain `ls` asks exactly what it always did.
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

/// What `goal finalize` asks before execution starts: agents are spawned and
/// start writing code, so the question names how much of it is about to run.
async fn finalize_question(client: &Client, goal: &GoalDto) -> String {
    let tasks = goal_tasks(client, &goal.id).await.len();
    let tail = match tasks {
        1 => "1 task starts running".into(),
        n => format!("{n} tasks start running"),
    };
    format!("Finalize the plan of goal \"{}\" ({tail})?", goal.title)
}

/// What `goal cancel` asks before it fans out: cancelling is irreversible and
/// takes every task that has not finished with it, so the question names both.
async fn cancel_question(client: &Client, goal: &GoalDto) -> String {
    let tasks = goal_tasks(client, &goal.id).await;
    let live = tasks.iter().filter(|t| !t.status.is_terminal()).count();
    let tail = match live {
        0 => "no task is still running".into(),
        1 => "1 live task will be cancelled too".into(),
        n => format!("{n} live tasks will be cancelled too"),
    };
    format!("Cancel goal \"{}\" ({})?", goal.title, tail)
}

/// What `goal rm` asks before it deletes: the goal's tasks, messages and
/// review history go with it and none of it comes back, so the question names
/// how much history is about to be dropped.
async fn rm_question(client: &Client, goal: &GoalDto) -> String {
    let tasks = goal_tasks(client, &goal.id).await.len();
    let tail = match tasks {
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

/// The ids the `--repo` arguments name, looked up among the registered
/// repositories: a goal references repositories now, so nothing here can
/// register one on the fly.
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

    fn repo(id: &str, path: &str, base_branch: &str) -> RepositoryDto {
        RepositoryDto {
            id: id.into(),
            path: path.into(),
            base_branch: base_branch.into(),
            description: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn repos() -> Vec<RepositoryDto> {
        vec![
            repo("01REPOAPI", "/home/me/api", "main"),
            repo("01REPOUI", "/home/me/ui", "main"),
            repo("01REPOUINEXT", "/home/me/ui", "next"),
        ]
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
