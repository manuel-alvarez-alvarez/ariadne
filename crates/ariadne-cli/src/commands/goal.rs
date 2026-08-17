//! `ariadne goal ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::goals::{CreateGoalRequest, GoalDto, RepoSpec};
use ariadne_api::messages::MessageDto;
use ariadne_api::tasks::{TaskDto, TaskListQuery};
use ariadne_client::Client;

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
        /// Repo path, optionally with the base branch as path@branch
        /// (path:branch also works when the branch has no '/'); repeatable
        #[arg(long = "repo", required = true)]
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
    /// Show the goal-level conversation
    Messages {
        /// Goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
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
                        repos: repos.iter().map(|s| parse_repo(s)).collect(),
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
        GoalCommand::Ls { no_trunc } => {
            let goals: Vec<GoalDto> = client.get_json("/v1/goals").await?;
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
                        note("no goals yet — create one with: ariadne goal create");
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
        GoalCommand::Cancel { id, yes } => {
            let g: GoalDto = client.get_json(&format!("/v1/goals/{id}")).await?;
            confirm(&cancel_question(client, &g).await, yes)?;
            let g: GoalDto = client.post_empty(&format!("/v1/goals/{id}/cancel")).await?;
            match format {
                Format::Json => print_json(&g)?,
                Format::Table => println!("goal {} is now {}", g.id, g.status.as_str()),
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
    }
    Ok(())
}

/// What `goal cancel` asks before it fans out: cancelling is irreversible and
/// takes every task that has not finished with it, so the question names both.
async fn cancel_question(client: &Client, goal: &GoalDto) -> String {
    let query = TaskListQuery {
        goal: Some(goal.id.clone()),
        status: None,
    };
    // The count is context for the question, not the answer to it: a daemon
    // that will not list the tasks still gets asked about the goal.
    let tasks: Vec<TaskDto> = match query_path("/v1/tasks", &query) {
        Ok(path) => client.get_json(&path).await.unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let live = tasks.iter().filter(|t| !t.status.is_terminal()).count();
    let tail = match live {
        0 => "no task is still running".into(),
        1 => "1 live task will be cancelled too".into(),
        n => format!("{n} live tasks will be cancelled too"),
    };
    format!("Cancel goal \"{}\" ({})?", goal.title, tail)
}

/// A `--repo` argument: a path, optionally carrying the base branch.
///
/// `path@branch` is the spelling that always works. `path:branch` came first
/// and stays for the branches it can express — a branch with a `/` in it is
/// indistinguishable from a path there, so `:` only splits when the suffix
/// has none.
fn parse_repo(spec: &str) -> RepoSpec {
    let split = spec
        .rsplit_once('@')
        .filter(|(path, branch)| !path.is_empty() && !branch.is_empty())
        .or_else(|| {
            spec.rsplit_once(':').filter(|(path, branch)| {
                !path.is_empty() && !branch.is_empty() && !branch.contains('/')
            })
        });
    match split {
        Some((path, branch)) => RepoSpec {
            path: path.to_string(),
            base_branch: Some(branch.to_string()),
        },
        None => RepoSpec {
            path: spec.to_string(),
            base_branch: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(spec: &str) -> (String, Option<String>) {
        let r = parse_repo(spec);
        (r.path, r.base_branch)
    }

    #[test]
    fn a_bare_path_has_no_branch() {
        assert_eq!(parsed("/home/me/api"), ("/home/me/api".into(), None));
    }

    #[test]
    fn a_colon_still_names_a_branch() {
        assert_eq!(
            parsed("/home/me/api:main"),
            ("/home/me/api".into(), Some("main".into()))
        );
    }

    /// The reason `@` exists: `:` cannot tell this branch from a path.
    #[test]
    fn an_at_sign_names_a_branch_with_a_slash_in_it() {
        assert_eq!(
            parsed("/home/me/api@feature/x"),
            ("/home/me/api".into(), Some("feature/x".into()))
        );
        assert_eq!(
            parsed("/home/me/api:feature/x"),
            ("/home/me/api:feature/x".into(), None)
        );
    }

    /// A path that itself contains a colon keeps it, as before.
    #[test]
    fn a_colon_inside_a_path_is_not_a_branch_separator() {
        assert_eq!(
            parsed("/home/me/a:b/api"),
            ("/home/me/a:b/api".into(), None)
        );
    }

    /// `@` wins: with both, the colon is part of the path.
    #[test]
    fn an_at_sign_beats_a_colon() {
        assert_eq!(
            parsed("/home/me/a:b@release/1.2"),
            ("/home/me/a:b".into(), Some("release/1.2".into()))
        );
    }

    /// A trailing separator names no branch; the path is what was typed.
    #[test]
    fn a_dangling_separator_is_part_of_the_path() {
        assert_eq!(parsed("/home/me/api@"), ("/home/me/api@".into(), None));
        assert_eq!(parsed("/home/me/api:"), ("/home/me/api:".into(), None));
    }
}
