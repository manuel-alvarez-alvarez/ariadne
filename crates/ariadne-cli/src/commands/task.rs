//! `ariadne task ...`

mod edit;

use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::reviews::ReviewDto;
use ariadne_api::tasks::{CreateTaskRequest, TaskDto, TaskListQuery, TaskTransitionDto};
use ariadne_client::Client;
use ariadne_core::TaskStatus;

use edit::{resolve_repo, update_request};
use super::{ProfileNames, confirm, print_messages, query_path};
use crate::output::{Column, Format, UNCAPPED, dash, local_time, note, print, print_kv, print_list, yes_no};

/// Columns of `task ls`. Titles and branches are the long ones: a task whose
/// title runs to a paragraph would otherwise push status and round off-screen.
///
/// `pr` says whether the task has been published yet rather than where: a
/// table is for scanning, and `task inspect` and `--format json` carry the
/// link itself.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("title", 48),
    ("status", UNCAPPED),
    ("round", UNCAPPED),
    ("stalled", UNCAPPED),
    ("pr", UNCAPPED),
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
    /// Create a task in a goal
    ///
    /// What the planner does through its MCP tools, from the terminal: the
    /// task starts out `pending` and is picked up once the goal is active and
    /// the tasks it depends on have merged. Prints the new task id.
    Create {
        /// Goal id the task belongs to
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        goal: String,
        /// Short task title (what the engineer is asked to do)
        #[arg(long)]
        title: String,
        /// Task description: the brief the engineer works from
        #[arg(short = 'd', long, default_value = "")]
        description: String,
        /// Engineer profile id or name that owns the task
        #[arg(long, default_value = "Engineer", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::engineer_profiles))]
        engineer: String,
        /// Reviewer profile id or name, in review order; repeatable
        #[arg(long = "reviewer", default_value = "Reviewer", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::reviewer_profiles))]
        reviewers: Vec<String>,
        /// Id of a task that must merge before this one starts; repeatable
        #[arg(long = "depends-on", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        depends_on: Vec<String>,
        /// Which of the goal's repositories the task works in, by id or by
        /// its registered path (only needed when the goal has several)
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_repositories))]
        repo: Option<String>,
    },
    /// Edit a task that has not started yet
    ///
    /// Title, description, reviewers and dependencies, while the
    /// task is still pending or ready — once an engineer is on it the daemon
    /// refuses the edit. Every flag left out keeps what the task already has;
    /// `--reviewer` and `--depends-on` replace the whole list they name.
    Update {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New description
        #[arg(short = 'd', long)]
        description: Option<String>,
        /// Reviewer profile id or name, in review order; repeatable, and
        /// replaces the task's reviewers rather than adding to them
        #[arg(long = "reviewer", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::reviewer_profiles))]
        reviewers: Vec<String>,
        /// Id of a task that must merge first; repeatable, and replaces the
        /// task's dependencies rather than adding to them
        #[arg(long = "depends-on", conflicts_with = "clear_depends_on", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        depends_on: Vec<String>,
        /// Drop every dependency, leaving the task free to start
        #[arg(long)]
        clear_depends_on: bool,
    },
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
        /// Address the message: the task's engineer, one of its reviewers,
        /// or the goal's planner, by profile id or name, or "user" to reach
        /// the human. An addressed recipient is woken to read it.
        #[arg(long, value_name = "PROFILE|user", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_message_recipients))]
        to: Option<String>,
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
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
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
        TaskCommand::Create {
            goal,
            title,
            description,
            engineer,
            reviewers,
            depends_on,
            repo,
        } => {
            let repo_id = match repo {
                Some(spec) => Some(resolve_repo(client, &goal, &spec).await?),
                None => None,
            };
            let t: TaskDto = client
                .post_json(
                    &format!("/v1/goals/{goal}/tasks"),
                    &CreateTaskRequest {
                        title,
                        description,
                        repo_id,
                        engineer_profile: engineer,
                        reviewer_profiles: reviewers,
                        depends_on,
                    },
                )
                .await?;
            print(format, &t, || println!("{}", t.id))?;
        }
        TaskCommand::Update {
            id,
            title,
            description,
            reviewers,
            depends_on,
            clear_depends_on,
        } => {
            let body = update_request(title, description, reviewers, depends_on, clear_depends_on)?;
            let t: TaskDto = client.patch_json(&task_path(&id), &body).await?;
            print(format, &t, || println!("updated {}", t.id))?;
        }
        TaskCommand::Ls {
            goal,
            status,
            no_trunc,
        } => {
            let filtered = goal.is_some() || status.is_some();
            let path = query_path("/v1/tasks", &TaskListQuery { goal, status })?;
            let tasks: Vec<TaskDto> = client.get_json(&path).await?;
            print_list(
                format,
                &tasks,
                LS,
                no_trunc,
                ls_row,
                // An empty list under a filter is not an empty system, and
                // saying so would send the reader looking for tasks that are
                // right there.
                match filtered {
                    true => "no tasks match that filter",
                    false => "no tasks yet — the planner creates them from a goal",
                },
            )?;
        }
        TaskCommand::Inspect { id } => {
            let t: TaskDto = client.get_json(&task_path(&id)).await?;
            let profiles = ProfileNames::for_format(client, format).await;
            print(format, &t, || {
                print_kv(&[
                    ("id", t.id.clone()),
                    ("goal", t.goal_id.clone()),
                    ("title", t.title.clone()),
                    ("status", t.status.as_str().into()),
                    (
                        "engineer",
                        profiles.pinned_label(
                            &t.engineer_profile_id,
                            t.agent_kind,
                            t.model.as_deref(),
                        ),
                    ),
                    (
                        "reviewers",
                        // One reviewer per line: each is a mention and the two
                        // facts after it, and the review order is what the
                        // column reads down.
                        t.reviewers
                            .iter()
                            .map(|r| {
                                profiles.pinned_label(&r.profile_id, r.agent_kind, r.model.as_deref())
                            })
                            .collect::<Vec<_>>()
                            .join("\n             "),
                    ),
                    (
                        "depends_on",
                        match t.depends_on.is_empty() {
                            true => "-".into(),
                            false => t.depends_on.join(", "),
                        },
                    ),
                    ("branch", t.branch.clone()),
                    ("worktree", dash(t.worktree_path.as_deref())),
                    ("round", t.review_round.to_string()),
                    ("stalled", yes_no(t.stalled, "no")),
                    ("merge", dash(t.merge_commit.as_deref())),
                    // The forge's own link, where the rest of a published
                    // task's story is; only an engineer that opened one
                    // reports it.
                    ("pull_request", dash(t.pr_url.as_deref())),
                    ("created", local_time(&t.created_at)),
                    ("description", format!("\n---\n{}", t.description)),
                ])
            })?;
        }
        TaskCommand::Messages { id } => {
            let msgs: Vec<MessageDto> = client
                .get_json(&format!("/v1/tasks/{id}/messages?limit=200"))
                .await?;
            print_messages(&msgs, format)?;
        }
        TaskCommand::Msg { id, body, to } => {
            let m: MessageDto = client
                .post_json(
                    &format!("/v1/tasks/{id}/messages"),
                    &CreateMessageRequest { body, to },
                )
                .await?;
            print(format, &m, || println!("posted {}", m.id))?;
        }
        TaskCommand::Reviews { id, no_trunc } => {
            let reviews: Vec<ReviewDto> =
                client.get_json(&format!("/v1/tasks/{id}/reviews")).await?;
            let profiles = ProfileNames::for_format(client, format).await;
            print_list(
                format,
                &reviews,
                REVIEWS,
                no_trunc,
                |r| {
                    vec![
                        r.round.to_string(),
                        profiles.label(&r.reviewer_profile_id),
                        r.verdict.as_str().into(),
                        r.body.clone().unwrap_or_else(|| "-".into()),
                    ]
                },
                "no reviews yet",
            )?;
        }
        TaskCommand::History { id } => {
            let rows: Vec<TaskTransitionDto> = client
                .get_json(&format!("/v1/tasks/{id}/transitions"))
                .await?;
            print(format, &rows, || {
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
            })?;
        }
        TaskCommand::Cancel { id, yes } => {
            let t: TaskDto = client.get_json(&task_path(&id)).await?;
            confirm(&cancel_question(&t), yes)?;
            let t: TaskDto = client.post_empty(&format!("/v1/tasks/{id}/cancel")).await?;
            print_status(&t, format)?;
        }
        TaskCommand::Retry { id } => {
            let t: TaskDto = client.post_empty(&format!("/v1/tasks/{id}/retry")).await?;
            print_status(&t, format)?;
        }
        TaskCommand::Diff { id } => {
            let diff = client.get_text(&format!("/v1/tasks/{id}/diff")).await?;
            // A diff is text, not a document; json mode still has to be
            // parseable, so it travels as one.
            print(format, &json!({"task_id": id, "diff": diff}), || {
                print!("{diff}")
            })?;
        }
        TaskCommand::Attach { id, role } => {
            crate::commands::attach::attach(client, &id, role).await?;
        }
        TaskCommand::Logs { id, role } => {
            let session = crate::commands::attach::resolve_tmux(client, &id, role).await?;
            let logs: ariadne_api::sessions::SessionLogsResponse = client
                .get_json(&format!("/v1/sessions/{}/logs", session.id))
                .await?;
            print(format, &logs, || print!("{}", logs.logs))?;
        }
    }
    Ok(())
}

fn task_path(id: &str) -> String {
    format!("/v1/tasks/{id}")
}

/// One row of `task ls`, in [`LS`]'s order.
fn ls_row(t: &TaskDto) -> Vec<String> {
    vec![
        t.id.clone(),
        t.title.clone(),
        t.status.as_str().into(),
        t.review_round.to_string(),
        yes_no(t.stalled, "-"),
        yes_no(t.pr_url.is_some(), "-"),
        t.branch.clone(),
    ]
}

/// What `task cancel` asks before the work is thrown away: cancelling is
/// irreversible and the id alone does not say which work that is, so the
/// question names the task and where it got to.
fn cancel_question(t: &TaskDto) -> String {
    format!("Cancel task \"{}\" ({})?", t.title, t.status.as_str())
}

/// What a mutation prints: the task it produced, or where it got to.
fn print_status(t: &TaskDto, format: Format) -> Result<()> {
    print(format, t, || {
        println!("task {} is now {}", t.id, t.status.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::commands::fixtures;

    /// A plain task, for the blocks that render one.
    fn dto() -> TaskDto {
        TaskDto {
            title: "Add the frobnicator".into(),
            branch: "add-the-frobnicator-01task".into(),
            ..fixtures::task("01TASK", "01GOAL")
        }
    }

    /// The question is the last thing between the caller and a cancelled
    /// task, so it says which task by title, not by the id already typed.
    #[test]
    fn the_cancel_question_names_the_task_and_its_status() {
        assert_eq!(
            cancel_question(&dto()),
            "Cancel task \"Add the frobnicator\" (in_progress)?"
        );
    }

    /// A profile is mentioned by the name that addresses it, with the bare id
    /// where the daemon would not name it — the `reviewer` column, and every
    /// other mention of a profile in the CLI.
    #[test]
    fn a_profile_is_mentioned_by_name_and_falls_back_to_its_id() {
        let profiles = ProfileNames::from_pairs([("01REV".to_string(), "My Reviewer".to_string())]);
        assert_eq!(profiles.label("01REV"), "My Reviewer (01REV)");
        assert_eq!(profiles.label("01GONE"), "01GONE");
    }

    /// A published task says so in the list, which is the question a table
    /// answers; the link itself is `task inspect`'s.
    #[test]
    fn the_list_says_whether_a_task_was_published() {
        let published = TaskDto {
            status: TaskStatus::Approved,
            pr_url: Some("https://github.com/owner/repo/pull/12".into()),
            ..dto()
        };
        let row = ls_row(&published);
        assert_eq!(row.len(), LS.len(), "a row per column, in LS's order");
        assert_eq!(
            row,
            [
                "01TASK",
                "Add the frobnicator",
                "approved",
                "0",
                "-",
                "yes",
                "add-the-frobnicator-01task",
            ]
        );
        assert_eq!(ls_row(&dto())[5], "-", "and a task nobody published says nothing");
    }
}
