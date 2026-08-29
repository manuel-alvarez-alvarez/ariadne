//! `ariadne task ...`

mod edit;

use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::reviews::ReviewDto;
use ariadne_api::stream::EventStreamQuery;
use ariadne_api::tasks::{
    CreateTaskRequest, ReviewerAssignment, TaskDto, TaskListQuery, TaskTransitionDto,
};
use ariadne_api::usage::TokenUsageDto;
use ariadne_client::{Client, SseEvent};
use ariadne_core::TaskStatus;

use super::follow;
use super::resolve::{self, Kind};
use super::{
    ProfileNames, Subject, confirm, one_of, parse_model, parse_model_or_default, print_thread,
    query_path, recipient,
};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, UNCAPPED, age, col, dash, local_time, moment, note, pager, print, print_json,
    print_kv, print_list, usage_block, usage_cell, view, yes_no,
};
use edit::{parse_reviewer, resolve_repo, resolved_reviewers, update_request};

/// Columns of `task ls`. Titles and branches are the long ones: a task whose
/// title runs to a paragraph would otherwise push status and round off-screen.
///
/// `pr` says whether the task has been published yet rather than where: a
/// table is for scanning, and `task inspect` and `--format json` carry the
/// link itself.
///
/// What the task is and where it got to are what an 80-column terminal is
/// left with; the branch goes first, since it is the title again in
/// kebab-case, and the spend last of the droppable ones.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 48).title(),
    col("status", UNCAPPED).status(),
    col("age", UNCAPPED).rank(5),
    col("round", UNCAPPED).rank(4),
    col("stalled", UNCAPPED).rank(3),
    col("pr", UNCAPPED).rank(2),
    col("tokens", UNCAPPED).rank(1),
    col("branch", 40).rank(0),
];

/// Where a continuation line of `task inspect` starts: [`print_kv`] pads its
/// keys to the longest one — `pull_request` — and then two spaces, and a
/// block that spills over several lines lines them all up under the first.
const INDENT: &str = "\n              ";

/// Columns of `task reviews`. A review body is prose, and only its opening
/// belongs in a table — `task reviews --format json` has all of it.
const REVIEWS: &[Column] = &[
    col("round", UNCAPPED),
    col("reviewer", 24).title(),
    col("verdict", UNCAPPED),
    col("body", 60).rank(0),
];

/// What `task create --help` ends with.
const CREATE_EXAMPLES: &str = "\
Examples:
  ariadne task create <goal-id> --title \"Add the rate limiter middleware\"

  # after another task, on a model and a reviewer of your own
  ariadne task create <goal-id> --title \"Wire it up\" --depends-on <task-id> \\
      --model codex:gpt-5.3-codex --reviewer Reviewer=claude_code

  # in one of the goal's repositories, when it has several
  ariadne task create <goal-id> --title \"Document it\" --repo ~/projects/ui
";

/// What `task update --help` ends with.
const UPDATE_EXAMPLES: &str = "\
Examples:
  ariadne task update <task-id> --title \"Add the rate limiter middleware\"
  ariadne task update <task-id> --model claude_code --reviewer Reviewer=codex:o3
  ariadne task update <task-id> --model default        # back to the profile's own
  ariadne task update <task-id> --clear-depends-on     # free it to start now
";

#[derive(Subcommand)]
pub enum TaskCommand {
    /// Create a task in a goal
    ///
    /// What the planner does through its MCP tools, from the terminal: the
    /// task starts out `pending` and is picked up once the goal is active and
    /// the tasks it depends on have merged. Prints the new task id.
    #[command(after_help = CREATE_EXAMPLES)]
    Create {
        /// Goal id the task belongs to
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        goal: String,
        /// Short task title (what the engineer is asked to do)
        #[arg(long)]
        title: String,
        /// Task description: the brief the engineer works from
        #[arg(short = 'd', long, default_value = "", hide_default_value = true)]
        description: String,
        /// Engineer profile id or name that owns the task
        #[arg(long, default_value = "Engineer", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::engineer_profiles))]
        engineer: String,
        /// What the engineer runs on: AGENT[:MODEL] — an agent CLI
        /// (claude_code | codex | opencode) on its own default model, or one
        /// model of it after the colon (codex:gpt-5.3-codex). Default: the
        /// engineer profile's own
        #[arg(long, value_name = "MODEL", value_parser = parse_model, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// Reviewer profile id or name, in review order; repeatable. Add
        /// `=MODEL` to run that reviewer on something other than its profile's
        /// own (`--reviewer Reviewer=codex:gpt-5.3-codex`)
        #[arg(long = "reviewer", value_name = "PROFILE[=MODEL]", default_value = "Reviewer", value_parser = parse_reviewer, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::reviewer_profiles))]
        reviewers: Vec<ReviewerAssignment>,
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
    /// Title, description, what the engineer runs on, reviewers and
    /// dependencies, while the task is still pending or ready — once an
    /// engineer is on it the daemon refuses the edit. Every flag left out
    /// keeps what the task already has; `--reviewer` and `--depends-on`
    /// replace the whole list they name.
    #[command(after_help = UPDATE_EXAMPLES)]
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
        /// What the engineer runs on: AGENT[:MODEL] — an agent CLI
        /// (claude_code | codex | opencode) on its own default model, or one
        /// model of it after the colon (codex:gpt-5.3-codex); "default" hands
        /// it back to the engineer profile's own
        #[arg(long, value_name = "MODEL|default", value_parser = parse_model_or_default, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models_or_default))]
        model: Option<String>,
        /// Reviewer profile id or name, optionally `=MODEL`, in review order;
        /// repeatable, and replaces the task's reviewers rather than adding to
        /// them
        #[arg(long = "reviewer", value_name = "PROFILE[=MODEL]", value_parser = parse_reviewer, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::reviewer_profiles))]
        reviewers: Vec<ReviewerAssignment>,
        /// Id of a task that must merge first; repeatable, and replaces the
        /// task's dependencies rather than adding to them
        #[arg(long = "depends-on", conflicts_with = "clear_depends_on", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        depends_on: Vec<String>,
        /// Drop every dependency, leaving the task free to start
        #[arg(long)]
        clear_depends_on: bool,
    },
    /// List tasks: the unfinished ones, newest first (--all includes the rest)
    Ls {
        /// Filter by goal id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        goal: Option<String>,
        /// Filter by status; repeatable and comma-separated, and a task in
        /// any of the named statuses is listed. Names the statuses precisely,
        /// so it replaces the unfinished/finished split --all makes
        #[arg(long = "status", value_parser = Spelling::<TaskStatus>::new(), value_delimiter = ',')]
        statuses: Vec<TaskStatus>,
        /// Include finished tasks (merged/cancelled/failed), not just the ones
        /// still going; nothing to add once --status names one
        #[arg(short, long)]
        all: bool,
        /// Redraw the table whenever a task changes, until Ctrl-C
        #[arg(long)]
        watch: bool,
    },
    /// Show a task
    Inspect {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
    },
    /// Show a task's conversation
    Thread {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// Read this many messages from the start of the thread
        /// (default 200)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..), conflicts_with = "tail")]
        limit: Option<u32>,
        /// Read this many messages from the end of the thread instead
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        tail: Option<u32>,
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
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::cancellable_task_ids))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Retry a failed task
    Retry {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::retryable_task_ids))]
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
        #[arg(long, value_parser = Spelling::<ariadne_core::Role>::new())]
        role: Option<ariadne_core::Role>,
    },
    /// Show recent terminal output of the task's agent
    Logs {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// engineer (default) or reviewer
        #[arg(long, value_parser = Spelling::<ariadne_core::Role>::new())]
        role: Option<ariadne_core::Role>,
        /// Keep printing output until the session ends
        #[arg(short, long)]
        follow: bool,
    },
}

pub async fn run(client: &Client, cmd: TaskCommand, format: Format) -> Result<()> {
    match cmd {
        TaskCommand::Create {
            goal,
            title,
            description,
            engineer,
            model,
            reviewers,
            depends_on,
            repo,
        } => {
            let goal = resolve::id(client, Kind::Goal, &goal).await?;
            let depends_on = resolve::ids(client, Kind::Task, &depends_on).await?;
            let mut profiles = resolve::Profiles::new(client);
            let engineer = profiles.id(&engineer).await?;
            let reviewers = resolved_reviewers(&mut profiles, reviewers).await?;
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
                        model,
                        reviewers,
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
            model,
            reviewers,
            depends_on,
            clear_depends_on,
        } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let depends_on = resolve::ids(client, Kind::Task, &depends_on).await?;
            let reviewers =
                resolved_reviewers(&mut resolve::Profiles::new(client), reviewers).await?;
            let body = update_request(
                title,
                description,
                model,
                reviewers,
                depends_on,
                clear_depends_on,
            )?;
            let t: TaskDto = client.patch_json(&task_path(&id), &body).await?;
            print(format, &t, || println!("updated {}", t.id))?;
        }
        TaskCommand::Ls {
            goal,
            statuses,
            all,
            watch,
        } => ls(client, goal, statuses, all, watch, format).await?,
        TaskCommand::Inspect { id } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
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
                        profiles.pinned_label(&t.engineer_profile_id, t.model.as_deref()),
                    ),
                    (
                        "reviewers",
                        // One reviewer per line: each is a mention and the two
                        // facts after it, and the review order is what the
                        // column reads down.
                        t.reviewers
                            .iter()
                            .map(|r| profiles.pinned_label(&r.profile_id, r.model.as_deref()))
                            .collect::<Vec<_>>()
                            .join(INDENT),
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
                    ("tokens", usage_lines(&t)),
                    ("round", t.review_round.to_string()),
                    ("stalled", yes_no(t.stalled, "no")),
                    ("merge", dash(t.merge_commit.as_deref())),
                    // The forge's own link, where the rest of a published
                    // task's story is; only an engineer that opened one
                    // reports it.
                    ("pull_request", dash(t.pr_url.as_deref())),
                    ("created", moment(&t.created_at)),
                    ("description", format!("\n---\n{}", t.description)),
                ])
            })?;
        }
        TaskCommand::Thread { id, limit, tail } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            print_thread(
                client,
                &format!("/v1/tasks/{id}/messages"),
                limit,
                tail,
                format,
            )
            .await?;
        }
        TaskCommand::Msg { id, body, to } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let to = recipient(client, to).await?;
            let m: MessageDto = client
                .post_json(
                    &format!("/v1/tasks/{id}/messages"),
                    &CreateMessageRequest { body, to },
                )
                .await?;
            print(format, &m, || println!("posted {}", m.id))?;
        }
        TaskCommand::Reviews { id } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let reviews: Vec<ReviewDto> =
                client.get_json(&format!("/v1/tasks/{id}/reviews")).await?;
            let profiles = ProfileNames::for_format(client, format).await;
            print_list(
                format,
                &reviews,
                REVIEWS,
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
            let id = resolve::id(client, Kind::Task, &id).await?;
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
            let id = resolve::id(client, Kind::Task, &id).await?;
            let t: TaskDto = client.get_json(&task_path(&id)).await?;
            let subject = Subject::new("task", &t.title, &t.id);
            confirm("cancel", &subject, &cancel_question(&t, &subject), yes)?;
            let t: TaskDto = client.post_empty(&format!("/v1/tasks/{id}/cancel")).await?;
            print_status(&t, format)?;
        }
        TaskCommand::Retry { id } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let t: TaskDto = client.post_empty(&format!("/v1/tasks/{id}/retry")).await?;
            print_status(&t, format)?;
        }
        TaskCommand::Diff { id } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let diff = client.get_text(&format!("/v1/tasks/{id}/diff")).await?;
            match format {
                // A diff is text, not a document; json mode still has to be
                // parseable, so it travels as one.
                Format::Json => print_json(&json!({"task_id": id, "diff": diff}))?,
                // On a terminal: coloured, and through the pager, since a
                // review-sized diff is not something one reads by scrolling
                // back. In a pipe: the bytes the daemon sent, so `task diff |
                // git apply` still works.
                Format::Table => pager::page(&pager::diff(&diff, view().color))?,
            }
        }
        TaskCommand::Attach { id, role } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            crate::commands::attach::attach(client, &id, role).await?;
        }
        TaskCommand::Logs { id, role, follow } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let session = crate::commands::attach::resolve_tmux(client, &id, role).await?;
            crate::commands::session::logs(client, &session.id, follow, format).await?;
        }
    }
    Ok(())
}

fn task_path(id: &str) -> String {
    format!("/v1/tasks/{id}")
}

/// The events that change what `task ls` shows. A goal going takes its tasks
/// with it, which no `task_*` event says.
fn relevant(frame: &SseEvent) -> bool {
    matches!(
        frame.event.as_str(),
        "task_created" | "task_updated" | "goal_deleted"
    )
}

/// `task ls [--watch]`: the table, and with `--watch` the table again every
/// time a task moves.
async fn ls(
    client: &Client,
    goal: Option<String>,
    statuses: Vec<TaskStatus>,
    all: bool,
    watch: bool,
    format: Format,
) -> Result<()> {
    // Resolved once rather than per redraw: what the caller typed names the
    // same goal every time round, and a watch is not a new question.
    let goal = match goal {
        Some(goal) => Some(resolve::id(client, Kind::Goal, &goal).await?),
        None => None,
    };
    if !watch {
        return render(client, goal, &statuses, all, format).await;
    }
    // The stream takes the same goal filter the list does, so a watch on one
    // goal is not woken by every other goal in the system.
    let path = query_path(
        "/v1/events/stream",
        &EventStreamQuery {
            goal: goal.clone(),
            task: None,
        },
    )?;
    follow::watch(client, &path, relevant, async || {
        render(client, goal.clone(), &statuses, all, format).await
    })
    .await
}

/// The table as it stands, read afresh.
async fn render(
    client: &Client,
    goal: Option<String>,
    statuses: &[TaskStatus],
    all: bool,
    format: Format,
) -> Result<()> {
    let filtered = goal.is_some() || !statuses.is_empty();
    // `GET /v1/tasks` takes one status, so one is asked for and the rest is
    // narrowed on the answer — with the live/finished split.
    let status = one_of(statuses);
    let path = query_path("/v1/tasks", &TaskListQuery { goal, status })?;
    let tasks: Vec<TaskDto> = client.get_json(&path).await?;
    let tasks = visible(tasks, all, statuses);
    let now = chrono::Utc::now();
    print_list(
        format,
        &tasks,
        LS,
        |t| ls_row(t, now),
        // An empty list under a filter is not an empty system, and saying so
        // would send the reader looking for tasks that are right there.
        match (filtered, all) {
            (true, _) => "no tasks match that filter",
            (false, true) => "no tasks yet — the planner creates them from a goal",
            (false, false) => "no tasks under way — finished ones are behind --all",
        },
    )
}

/// One row of `task ls`, in [`LS`]'s order.
fn ls_row(t: &TaskDto, now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    vec![
        t.id.clone(),
        t.title.clone(),
        t.status.as_str().into(),
        age(&t.created_at, now),
        t.review_round.to_string(),
        yes_no(t.stalled, "-"),
        yes_no(t.pr_url.is_some(), "-"),
        usage_cell(&t.usage.total),
        t.branch.clone(),
    ]
}

/// Which of the tasks the daemon answered with `task ls` shows: the ones
/// still going, newest first, with everything behind --all.
///
/// The same default as `session ls` and `goal ls`. A goal that has run its
/// course is thirty merged tasks and the two that matter, and the two are
/// what a list is read for; `--status merged` is how one asks for the thirty.
/// A named --status takes over, since it has already said which tasks are
/// wanted.
fn visible(tasks: Vec<TaskDto>, all: bool, statuses: &[TaskStatus]) -> Vec<TaskDto> {
    let mut tasks: Vec<TaskDto> = tasks
        .into_iter()
        // `--status` is asked of the daemon one at a time; the rest of what it
        // named is narrowed here, as `session ls --role` has always been.
        .filter(|t| statuses.is_empty() || statuses.contains(&t.status))
        .filter(|t| all || !statuses.is_empty() || !t.status.is_terminal())
        .collect();
    tasks.sort_by(|a, b| b.id.cmp(&a.id));
    tasks
}

/// What the task cost, spender by spender: the total first, then the
/// engineer and each reviewer under it, named the way a message addresses
/// them.
///
/// Every reviewer slot of the task gets a line, whether or not it has spent
/// anything: a reviewer missing from the block would read as one the task
/// does not have, and `0` is a fact where a gap is a question. A profile that
/// spent on the task without holding a slot any more is listed after them, so
/// the lines still add up to the total.
fn usage_lines(t: &TaskDto) -> String {
    let mut agents: Vec<(String, TokenUsageDto)> = vec![("engineer".into(), t.usage.engineer)];
    for r in &t.reviewers {
        let spent = spent_by(t, &r.profile_id).unwrap_or_default();
        agents.push((
            profile_label(r.profile_name.as_deref(), &r.profile_id),
            spent,
        ));
    }
    agents.extend(
        t.usage
            .reviewers
            .iter()
            .filter(|u| !t.reviewers.iter().any(|r| r.profile_id == u.profile_id))
            .map(|u| {
                (
                    profile_label(u.profile_name.as_deref(), &u.profile_id),
                    u.usage,
                )
            }),
    );

    usage_block(&t.usage.total, &agents, INDENT)
}

/// What one reviewer profile spent on the task, if the daemon reported it at
/// all — a slot whose reviewer has never been spawned has no entry.
fn spent_by(t: &TaskDto, profile_id: &str) -> Option<TokenUsageDto> {
    t.usage
        .reviewers
        .iter()
        .find(|u| u.profile_id == profile_id)
        .map(|u| u.usage)
}

/// A profile as this block names it: its name, or its id where the daemon
/// would not name it — the way every other mention of a profile falls back.
fn profile_label(name: Option<&str>, profile_id: &str) -> String {
    name.unwrap_or(profile_id).to_string()
}

/// What `task cancel` asks before the work is thrown away: cancelling is
/// irreversible and the id alone does not say which work that is, so the
/// question names the task and where it got to.
fn cancel_question(t: &TaskDto, subject: &Subject) -> String {
    format!("Cancel {} task {}?", t.status.as_str(), subject.named())
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

    use ariadne_api::tasks::{ProfileUsageDto, TaskReviewerDto, TaskUsageDto};

    use crate::commands::fixtures;

    /// Three hours after every fixture was created, so an `AGE` cell is a
    /// figure a test can name.
    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(fixtures::NOW)
            .expect("parse")
            .with_timezone(&chrono::Utc)
            + chrono::Duration::hours(3)
    }

    /// A plain task, for the blocks that render one.
    fn dto() -> TaskDto {
        TaskDto {
            title: "Add the frobnicator".into(),
            branch: "add-the-frobnicator-01task".into(),
            ..fixtures::task("01TASK", "01GOAL")
        }
    }

    fn usage(input: u64, cached: u64, output: u64) -> TokenUsageDto {
        TokenUsageDto {
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
        }
    }

    fn reviewer(profile_id: &str, name: &str) -> TaskReviewerDto {
        TaskReviewerDto {
            profile_id: profile_id.into(),
            profile_name: Some(name.into()),
            model: None,
        }
    }

    /// A task nobody has run yet still says what it spent: `0`, which is a
    /// figure, where a blank or a dash would read as "the daemon does not
    /// know".
    #[test]
    fn a_task_that_has_spent_nothing_says_zero() {
        assert_eq!(ls_row(&dto(), now())[7], "↑0 0% ↓0");
        let block = usage_lines(&dto());
        assert_eq!(block.lines().next().unwrap(), "input   0  0%");
        assert!(block.contains("output  0"), "{block}");
    }

    /// The block is the total and then who spent it: the engineer, and every
    /// reviewer slot by the name a message addresses it with — including the
    /// one that has never been spawned, which spent `0` rather than nothing
    /// at all.
    #[test]
    fn the_block_names_the_engineer_and_every_reviewer_of_the_task() {
        let t = TaskDto {
            reviewers: vec![reviewer("01REV", "Reviewer"), reviewer("01SEC", "Security")],
            usage: TaskUsageDto {
                total: usage(1_204_567, 1_100_000, 45_300),
                engineer: usage(1_200_000, 1_100_000, 45_000),
                reviewers: vec![ProfileUsageDto {
                    profile_id: "01REV".into(),
                    profile_name: Some("Reviewer".into()),
                    usage: usage(4_567, 0, 300),
                }],
            },
            ..dto()
        };
        assert_eq!(
            usage_lines(&t),
            [
                "input   1.2M  91%",
                "              output   45k",
                "              engineer  ↑1.2M ↓45k",
                "              Reviewer  ↑4.6k ↓300",
                "              Security  ↑0 ↓0",
            ]
            .join("\n")
        );
        assert_eq!(
            ls_row(&t, now())[7],
            "↑1.2M 91% ↓45k",
            "the row carries the total, and the same share"
        );
    }

    /// A profile that spent on the task without holding one of its slots is
    /// still listed: the lines under the total are meant to add up to it.
    #[test]
    fn a_spender_that_holds_no_reviewer_slot_is_still_listed() {
        let t = TaskDto {
            usage: TaskUsageDto {
                total: usage(1_000, 0, 100),
                engineer: usage(600, 0, 60),
                reviewers: vec![ProfileUsageDto {
                    profile_id: "01GONE".into(),
                    profile_name: None,
                    usage: usage(400, 0, 40),
                }],
            },
            ..dto()
        };
        assert!(
            usage_lines(&t).contains("01GONE    ↑400 ↓40"),
            "{}",
            usage_lines(&t)
        );
    }

    /// The question is the last thing between the caller and a cancelled
    /// task, so it says which task by title, not by the id already typed.
    #[test]
    fn the_cancel_question_names_the_task_and_its_status() {
        let t = TaskDto {
            id: "01m15jmta93b130wka2qdn2p1x".into(),
            ..dto()
        };
        let subject = Subject::new("task", &t.title, &t.id);
        assert_eq!(
            cancel_question(&t, &subject),
            "Cancel in_progress task \"Add the frobnicator\" (…2qdn2p1x)?"
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
        let row = ls_row(&published, now());
        assert_eq!(row.len(), LS.len(), "a row per column, in LS's order");
        assert_eq!(
            row,
            [
                "01TASK",
                "Add the frobnicator",
                "approved",
                "3h",
                "0",
                "-",
                "yes",
                "↑0 0% ↓0",
                "add-the-frobnicator-01task",
            ]
        );
        assert_eq!(
            ls_row(&dto(), now())[6],
            "-",
            "and a task nobody published says nothing"
        );
    }
}
