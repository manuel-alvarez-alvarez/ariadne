//! `ariadne task ...`

use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::reviews::ReviewDto;
use ariadne_api::tasks::{TaskDto, TaskListQuery, TaskTransitionDto};
use ariadne_client::Client;
use ariadne_core::TaskStatus;

use super::ProfileNames;
use crate::output::{
    Column, Format, UNCAPPED, local_time, note, print_json, print_kv, print_table,
};
use crate::query::query_path;

/// Columns of `task ls`. Titles and branches are the long ones: a task whose
/// title runs to a paragraph would otherwise push status and round off-screen.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("title", 48),
    ("status", UNCAPPED),
    ("round", UNCAPPED),
    ("stalled", UNCAPPED),
    ("branch", 40),
];

/// Columns of `task reviews`. A review body is prose, and only its opening
/// belongs in a table — `task reviews --format json` has all of it.
const REVIEWS: &[Column] = &[
    ("round", UNCAPPED),
    ("reviewer", 24),
    ("verdict", UNCAPPED),
    ("body", 60),
];

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
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
    },
    /// Show a task
    Inspect {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Show a task's conversation
    Messages {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Post a message into a task's conversation
    Msg {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// Message body
        body: String,
    },
    /// Show a task's reviews
    Reviews {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
    },
    /// Show a task's transition history
    History {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Cancel a task
    Cancel {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Retry a failed task
    Retry {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Show the diff of the task branch against its base
    Diff {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Attach to the task's agent tmux session
    Attach {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// engineer (default) or reviewer
        #[arg(long, value_enum)]
        role: Option<ariadne_core::Role>,
    },
    /// Show recent terminal output of the task's agent
    Logs {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// engineer (default) or reviewer
        #[arg(long, value_enum)]
        role: Option<ariadne_core::Role>,
    },
}

pub async fn run(client: &Client, cmd: TaskCommand, format: Format) -> Result<()> {
    match cmd {
        TaskCommand::Ls {
            goal,
            status,
            no_trunc,
        } => {
            let path = query_path("/v1/tasks", &TaskListQuery { goal, status })?;
            let tasks: Vec<TaskDto> = client.get_json(&path).await?;
            match format {
                Format::Json => print_json(&tasks)?,
                Format::Table => {
                    print_table(
                        LS,
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
                        no_trunc,
                    );
                    if tasks.is_empty() {
                        note("no tasks yet — the planner creates them from a goal");
                    }
                }
            }
        }
        TaskCommand::Inspect { id } => {
            let t: TaskDto = client.get_json(&format!("/v1/tasks/{id}")).await?;
            match format {
                Format::Json => print_json(&t)?,
                Format::Table => {
                    let profiles = ProfileNames::fetch(client).await;
                    print_kv(&[
                        ("id", t.id),
                        ("goal", t.goal_id),
                        ("title", t.title),
                        ("status", t.status.as_str().into()),
                        ("engineer", profiles.label(&t.engineer_profile_id)),
                        ("reviewers", profiles.labels(&t.reviewer_profile_ids)),
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
                        ("created", local_time(&t.created_at)),
                        ("description", format!("\n---\n{}", t.description)),
                    ]);
                }
            }
        }
        TaskCommand::Messages { id } => {
            let msgs: Vec<MessageDto> = client
                .get_json(&format!("/v1/tasks/{id}/messages?limit=200"))
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
        TaskCommand::Msg { id, body } => {
            let m: MessageDto = client
                .post_json(
                    &format!("/v1/tasks/{id}/messages"),
                    &CreateMessageRequest { body },
                )
                .await?;
            match format {
                Format::Json => print_json(&m)?,
                Format::Table => println!("posted {}", m.id),
            }
        }
        TaskCommand::Reviews { id, no_trunc } => {
            let reviews: Vec<ReviewDto> =
                client.get_json(&format!("/v1/tasks/{id}/reviews")).await?;
            match format {
                Format::Json => print_json(&reviews)?,
                Format::Table => {
                    let profiles = ProfileNames::fetch(client).await;
                    print_table(
                        REVIEWS,
                        &reviews
                            .iter()
                            .map(|r| {
                                vec![
                                    r.round.to_string(),
                                    profiles.label(&r.reviewer_profile_id),
                                    r.verdict.as_str().into(),
                                    r.body.clone().unwrap_or_else(|| "-".into()),
                                ]
                            })
                            .collect::<Vec<_>>(),
                        no_trunc,
                    );
                    if reviews.is_empty() {
                        note("no reviews yet");
                    }
                }
            }
        }
        TaskCommand::History { id } => {
            let rows: Vec<TaskTransitionDto> = client
                .get_json(&format!("/v1/tasks/{id}/transitions"))
                .await?;
            match format {
                Format::Json => print_json(&rows)?,
                Format::Table => {
                    for t in &rows {
                        println!(
                            "[{}] {} -> {} by {}{}",
                            local_time(&t.created_at),
                            t.from_status,
                            t.to_status,
                            t.actor,
                            t.reason
                                .as_ref()
                                .map(|r| format!(" ({r})"))
                                .unwrap_or_default()
                        );
                    }
                    if rows.is_empty() {
                        note("no transitions yet");
                    }
                }
            }
        }
        TaskCommand::Cancel { id } => {
            let t: TaskDto = client.post_empty(&format!("/v1/tasks/{id}/cancel")).await?;
            print_status(&t, format)?;
        }
        TaskCommand::Retry { id } => {
            let t: TaskDto = client.post_empty(&format!("/v1/tasks/{id}/retry")).await?;
            print_status(&t, format)?;
        }
        TaskCommand::Diff { id } => {
            let diff = client.get_text(&format!("/v1/tasks/{id}/diff")).await?;
            match format {
                // A diff is text, not a document; json mode still has to be
                // parseable, so it travels as one.
                Format::Json => print_json(&json!({"task_id": id, "diff": diff}))?,
                Format::Table => print!("{diff}"),
            }
        }
        TaskCommand::Attach { id, role } => {
            crate::commands::attach::attach(client, &id, role).await?;
        }
        TaskCommand::Logs { id, role } => {
            let session = crate::commands::attach::resolve_tmux(client, &id, role).await?;
            let logs: ariadne_api::sessions::SessionLogsResponse = client
                .get_json(&format!("/v1/sessions/{}/logs", session.id))
                .await?;
            match format {
                Format::Json => print_json(&logs)?,
                Format::Table => print!("{}", logs.logs),
            }
        }
    }
    Ok(())
}

/// What a mutation prints: the task it produced, or a sentence about it.
fn print_status(t: &TaskDto, format: Format) -> Result<()> {
    match format {
        Format::Json => print_json(t)?,
        Format::Table => println!("task {} is now {}", t.id, t.status.as_str()),
    }
    Ok(())
}
