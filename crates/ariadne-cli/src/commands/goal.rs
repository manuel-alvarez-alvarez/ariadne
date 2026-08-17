//! `ariadne goal ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::goals::{CreateGoalRequest, GoalDto, RepoSpec};
use ariadne_api::messages::MessageDto;
use ariadne_client::Client;

use crate::output::{Format, print_json, print_kv, print_table};

#[derive(Subcommand)]
pub enum GoalCommand {
    /// Create a goal
    Create {
        #[arg(long)]
        title: String,
        /// Goal description (what should be achieved)
        #[arg(short = 'd', long, default_value = "")]
        description: String,
        /// Repo path, optionally with base branch as path:branch (repeatable)
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
    Ls,
    /// Show a goal
    Inspect {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
    },
    /// Cancel a goal
    Cancel {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
    },
    /// Show the goal-level conversation
    Messages {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        id: String,
    },
    /// Attach to the goal's planner tmux session
    Attach {
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
            let repos = repos
                .into_iter()
                .map(|spec| {
                    // Split on the last ':' only when the suffix is not a path
                    // (absolute paths contain '/' after any ':').
                    match spec.rsplit_once(':') {
                        Some((path, branch)) if !branch.contains('/') && !branch.is_empty() => {
                            RepoSpec {
                                path: path.to_string(),
                                base_branch: Some(branch.to_string()),
                            }
                        }
                        _ => RepoSpec {
                            path: spec,
                            base_branch: None,
                        },
                    }
                })
                .collect();
            let goal: GoalDto = client
                .post_json(
                    "/v1/goals",
                    &CreateGoalRequest {
                        title,
                        description,
                        repos,
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
        GoalCommand::Ls => {
            let goals: Vec<GoalDto> = client.get_json("/v1/goals").await?;
            match format {
                Format::Json => print_json(&goals)?,
                Format::Table => print_table(
                    &["id", "title", "status", "approvals", "repos"],
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
                ),
            }
        }
        GoalCommand::Inspect { id } => {
            let g: GoalDto = client.get_json(&format!("/v1/goals/{id}")).await?;
            match format {
                Format::Json => print_json(&g)?,
                Format::Table => print_kv(&[
                    ("id", g.id),
                    ("title", g.title),
                    ("status", g.status.as_str().into()),
                    ("planner", g.planner_profile_id),
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
                    ("created", g.created_at),
                    ("description", format!("\n---\n{}", g.description)),
                ]),
            }
        }
        GoalCommand::Cancel { id } => {
            let g: GoalDto = client.post_empty(&format!("/v1/goals/{id}/cancel")).await?;
            println!("goal {} is now {}", g.id, g.status.as_str());
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
                    for m in msgs {
                        println!("[{}] {}: {}", m.created_at, m.author_role.as_str(), m.body);
                    }
                }
            }
        }
    }
    Ok(())
}
