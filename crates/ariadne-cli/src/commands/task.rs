//! `ariadne task ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::reviews::ReviewDto;
use ariadne_api::tasks::{TaskDto, TaskListQuery, TaskTransitionDto};
use ariadne_client::Client;
use ariadne_core::TaskStatus;

use crate::output::{Format, print_json, print_kv, print_table};
use crate::query::query_path;

#[derive(Subcommand)]
pub enum TaskCommand {
    /// List tasks
    Ls {
        /// Filter by goal id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        goal: Option<String>,
        /// Filter by status
        #[arg(long, value_enum)]
        status: Option<TaskStatus>,
    },
    /// Show a task
    Inspect {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Show a task's conversation
    Messages {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Post a message into a task's conversation
    Msg {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        body: String,
    },
    /// Show a task's reviews
    Reviews {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Show a task's transition history
    History {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Cancel a task
    Cancel {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Retry a failed task
    Retry {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Show the diff of the task branch against its base
    Diff {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Attach to the task's agent tmux session
    Attach {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// engineer (default) or reviewer
        #[arg(long, value_parser = crate::commands::parse_role, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::roles))]
        role: Option<ariadne_core::Role>,
    },
    /// Show recent terminal output of the task's agent
    Logs {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        #[arg(long, value_parser = crate::commands::parse_role, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::roles))]
        role: Option<ariadne_core::Role>,
    },
}

pub async fn run(client: &Client, cmd: TaskCommand, format: Format) -> Result<()> {
    match cmd {
        TaskCommand::Ls { goal, status } => {
            let path = query_path("/v1/tasks", &TaskListQuery { goal, status })?;
            let tasks: Vec<TaskDto> = client.get_json(&path).await?;
            match format {
                Format::Json => print_json(&tasks)?,
                Format::Table => print_table(
                    &["id", "title", "status", "round", "stalled", "branch"],
                    &tasks
                        .iter()
                        .map(|t| {
                            vec![
                                t.id.clone(),
                                t.title.clone(),
                                t.status.as_str().into(),
                                t.review_round.to_string(),
                                if t.stalled { "yes".into() } else { "-".into() },
                                t.branch.clone(),
                            ]
                        })
                        .collect::<Vec<_>>(),
                ),
            }
        }
        TaskCommand::Inspect { id } => {
            let t: TaskDto = client.get_json(&format!("/v1/tasks/{id}")).await?;
            match format {
                Format::Json => print_json(&t)?,
                Format::Table => print_kv(&[
                    ("id", t.id),
                    ("goal", t.goal_id),
                    ("title", t.title),
                    ("status", t.status.as_str().into()),
                    ("engineer", t.engineer_profile_id),
                    ("reviewers", t.reviewer_profile_ids.join(", ")),
                    (
                        "depends_on",
                        if t.depends_on.is_empty() {
                            "-".into()
                        } else {
                            t.depends_on.join(", ")
                        },
                    ),
                    ("branch", t.branch),
                    ("worktree", t.worktree_path.unwrap_or_else(|| "-".into())),
                    ("round", t.review_round.to_string()),
                    (
                        "stalled",
                        if t.stalled { "yes".into() } else { "no".into() },
                    ),
                    ("merge", t.merge_commit.unwrap_or_else(|| "-".into())),
                    ("created", t.created_at),
                    ("description", format!("\n---\n{}", t.description)),
                ]),
            }
        }
        TaskCommand::Messages { id } => {
            let msgs: Vec<MessageDto> = client
                .get_json(&format!("/v1/tasks/{id}/messages?limit=200"))
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
        TaskCommand::Msg { id, body } => {
            let m: MessageDto = client
                .post_json(
                    &format!("/v1/tasks/{id}/messages"),
                    &CreateMessageRequest { body },
                )
                .await?;
            println!("posted {}", m.id);
        }
        TaskCommand::Reviews { id } => {
            let reviews: Vec<ReviewDto> =
                client.get_json(&format!("/v1/tasks/{id}/reviews")).await?;
            match format {
                Format::Json => print_json(&reviews)?,
                Format::Table => print_table(
                    &["round", "reviewer", "verdict", "body"],
                    &reviews
                        .iter()
                        .map(|r| {
                            vec![
                                r.round.to_string(),
                                r.reviewer_profile_id.clone(),
                                r.verdict.as_str().into(),
                                r.body.clone().unwrap_or_else(|| "-".into()),
                            ]
                        })
                        .collect::<Vec<_>>(),
                ),
            }
        }
        TaskCommand::History { id } => {
            let rows: Vec<TaskTransitionDto> = client
                .get_json(&format!("/v1/tasks/{id}/transitions"))
                .await?;
            match format {
                Format::Json => print_json(&rows)?,
                Format::Table => {
                    for t in rows {
                        println!(
                            "[{}] {} -> {} by {}{}",
                            t.created_at,
                            t.from_status,
                            t.to_status,
                            t.actor,
                            t.reason.map(|r| format!(" ({r})")).unwrap_or_default()
                        );
                    }
                }
            }
        }
        TaskCommand::Cancel { id } => {
            let t: TaskDto = client.post_empty(&format!("/v1/tasks/{id}/cancel")).await?;
            println!("task {} is now {}", t.id, t.status.as_str());
        }
        TaskCommand::Retry { id } => {
            let t: TaskDto = client.post_empty(&format!("/v1/tasks/{id}/retry")).await?;
            println!("task {} is now {}", t.id, t.status.as_str());
        }
        TaskCommand::Diff { id } => {
            print!(
                "{}",
                client.get_text(&format!("/v1/tasks/{id}/diff")).await?
            );
        }
        TaskCommand::Attach { id, role } => {
            crate::commands::attach::attach(client, &id, role).await?;
        }
        TaskCommand::Logs { id, role } => {
            let session = crate::commands::attach::resolve_tmux(client, &id, role).await?;
            let logs: ariadne_api::sessions::SessionLogsResponse = client
                .get_json(&format!("/v1/sessions/{}/logs", session.id))
                .await?;
            print!("{}", logs.logs);
        }
    }
    Ok(())
}
