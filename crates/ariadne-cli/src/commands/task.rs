//! `ariadne task ...`

mod edit;

use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use ariadne_api::messages::MessageDto;
use ariadne_api::stream::EventStreamQuery;
use ariadne_api::tasks::{
    AgentAssignment, CreateTaskRequest, TaskDto, TaskListQuery, TaskTransitionDto,
};
use ariadne_api::usage::TokenUsageDto;
use ariadne_client::{Client, SseEvent};
use ariadne_core::{Actor, Landing, Seat, TaskStatus};

use super::follow;
use super::resolve::{self, Kind};
use super::{
    Subject, agent_label, agent_pin_label, confirm, one_of, parse_effort_or_default, parse_model,
    query_path,
};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, Kv, UNCAPPED, age, col, dash, local_time, moment, note, ok_id_line, pager,
    print, print_json, print_kv, print_list, status_line, usage_block, usage_cell, view, yes_no,
};
use edit::{Edits, parse_author, parse_reviewer, resolve_repo, update_request};

/// Columns of `task ls`. Titles and branches are the long ones: a task whose
/// title runs to a paragraph would otherwise push the status off-screen.
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
    col("age", UNCAPPED).rank(4),
    col("stalled", UNCAPPED).rank(3),
    col("pr", UNCAPPED).rank(2),
    col("tokens", UNCAPPED).rank(1),
    col("branch", 40).rank(0),
];

/// Where a continuation line of `task inspect` starts: [`print_kv`] pads its
/// keys to the longest one — `pull_request` — and then two spaces, and a
/// block that spills over several lines lines them all up under the first.
const INDENT: &str = "\n              ";

/// Columns of `task messages`. A message body is prose, and only its opening
/// belongs in a table — `task messages --format json` has all of it.
const MESSAGES: &[Column] = &[
    col("kind", UNCAPPED),
    col("from", 20).title(),
    col("to", 20).rank(1),
    col("body", 60).rank(0),
];

/// What `task create --help` ends with.
const CREATE_EXAMPLES: &str = "\
Examples:
  ariadne task create <goal-id> --title \"Add the rate limiter middleware\" \\
      --author coding,testing=claude_code:claude-sonnet-5 \\
      --reviewer code-review=codex:gpt-5.6-luna

  # after another task, reasoned deeply
  ariadne task create <goal-id> --title \"Wire it up\" --depends-on <task-id> \\
      --author coding,testing=codex:gpt-5.6-sol@xhigh \\
      --reviewer code-review=claude_code:claude-opus-5@high

  # nothing to review: approved as soon as the author asks
  ariadne task create <goal-id> --title \"Cut 0.6.0\" \\
      --author release=claude_code:claude-sonnet-5 --no-reviewer
";

/// What `task update --help` ends with.
const UPDATE_EXAMPLES: &str = "\
Examples:
  ariadne task update <task-id> --title \"Add the rate limiter middleware\"
  ariadne task update <task-id> --model claude_code:claude-opus-5 --effort xhigh
  ariadne task update <task-id> --reviewer code-review=codex:gpt-5.6-luna@high
  ariadne task update <task-id> --no-reviewer          # nothing left to review
  ariadne task update <task-id> --effort default       # at whatever the CLI reasons it at
  ariadne task update <task-id> --clear-depends-on     # free it to start now
";

#[derive(Subcommand)]
pub enum TaskCommand {
    /// Create a task in a goal
    ///
    /// What the orchestrator does through its MCP tools, from the terminal: the
    /// task starts out `pending` and is picked up once the goal is active and
    /// the tasks it depends on have merged. Prints the new task id.
    #[command(after_help = CREATE_EXAMPLES)]
    Create {
        /// Goal id the task belongs to
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        goal: String,
        /// Short task title (what the author is asked to do)
        #[arg(long)]
        title: String,
        /// Task description: the brief the author works from
        #[arg(short = 'd', long, default_value = "", hide_default_value = true)]
        description: String,
        /// The author's skills, comma-separated, then `=MODEL` — the agent
        /// CLI and model it runs on, required — and `@EFFORT` to say how
        /// deeply it reasons there
        /// (`--author coding,testing=codex:gpt-5.6-sol@xhigh`)
        #[arg(long, value_name = "SKILLS=MODEL[@EFFORT]", value_parser = parse_author)]
        author: AgentAssignment,
        /// One reviewer's skills and its model, in review order; repeatable.
        /// Spelled the same way as `--author`
        /// (`--reviewer code-review=codex:gpt-5.6-luna@high`)
        #[arg(long = "reviewer", value_name = "SKILLS=MODEL[@EFFORT]", conflicts_with = "no_reviewer", value_parser = parse_reviewer)]
        reviewers: Vec<AgentAssignment>,
        /// Staff no reviewer: the task is approved as soon as its author asks
        /// for review. For work with nothing to review, such as a release
        #[arg(long)]
        no_reviewer: bool,
        /// Id of a task that must finish before this one starts; repeatable
        #[arg(long = "depends-on", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        depends_on: Vec<String>,
        /// Which of the goal's repositories the task works in, by id or by
        /// its registered path (only needed when the goal has several)
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_repositories))]
        repo: Option<String>,
        /// How the task ends: merge on the base branch, pull-request opened
        /// and seen through to its merge, or none where there is nothing to
        /// land. Default: the way the repository takes a change
        #[arg(long, value_enum)]
        landing: Option<Landing>,
    },
    /// Edit a task that has not started yet
    ///
    /// Title, description, what the author runs on, reviewers and
    /// dependencies, while the task is still pending or ready — once an
    /// author is on it the daemon refuses the edit. Every flag left out
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
        /// What the author runs on: AGENT:MODEL — an agent CLI
        /// (claude_code | codex | opencode) and, after the colon, one model
        /// of it (codex:gpt-5.3-codex). A model is required, so "default" is
        /// refused: there is nothing to hand the pin back to
        #[arg(long, value_name = "MODEL", value_parser = parse_model, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// The reasoning effort that model is run at: one of the efforts
        /// `ariadne models ls` lists for it; "default" runs it at whatever
        /// the agent CLI runs it at
        #[arg(long, value_name = "EFFORT|default", value_parser = parse_effort_or_default, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::efforts_or_default))]
        effort: Option<String>,
        /// One reviewer's skills and its model, optionally `@EFFORT`, in
        /// review order; repeatable, and replaces the task's reviewers rather
        /// than adding to them
        #[arg(long = "reviewer", value_name = "SKILLS=MODEL[@EFFORT]", conflicts_with = "no_reviewer", value_parser = parse_reviewer)]
        reviewers: Vec<AgentAssignment>,
        /// Take every reviewer off the task, leaving it approved as soon as
        /// its author asks for review
        #[arg(long)]
        no_reviewer: bool,
        /// Id of a task that must finish first; repeatable, and replaces the
        /// task's dependencies rather than adding to them
        #[arg(long = "depends-on", conflicts_with = "clear_depends_on", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        depends_on: Vec<String>,
        /// Drop every dependency, leaving the task free to start
        #[arg(long)]
        clear_depends_on: bool,
        /// How the task ends: merge, pull-request or none
        #[arg(long, value_enum)]
        landing: Option<Landing>,
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
    /// Show what a task's agents have said to each other
    ///
    /// One channel for all of it: the questions and their answers, the
    /// author's review requests, and the reviewers' verdicts, oldest first.
    Messages {
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
        /// author (default) or reviewer
        #[arg(long, value_parser = Spelling::<ariadne_core::Seat>::new())]
        seat: Option<ariadne_core::Seat>,
    },
    /// Show recent terminal output of the task's agent
    Logs {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// author (default) or reviewer
        #[arg(long, value_parser = Spelling::<ariadne_core::Seat>::new())]
        seat: Option<ariadne_core::Seat>,
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
            author,
            reviewers,
            no_reviewer,
            depends_on,
            repo,
            landing,
        } => {
            let reviewers = if no_reviewer { Vec::new() } else { reviewers };
            let goal = resolve::id(client, Kind::Goal, &goal).await?;
            let depends_on = resolve::ids(client, Kind::Task, &depends_on).await?;
            // The author first, then the reviewers in review order: that is
            // the order the daemon reads a staffing in.
            let mut agents = vec![author];
            agents.extend(reviewers);
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
                        agents,
                        depends_on,
                        landing,
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
            effort,
            reviewers,
            no_reviewer,
            depends_on,
            clear_depends_on,
            landing,
        } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let depends_on = resolve::ids(client, Kind::Task, &depends_on).await?;
            let body = update_request(Edits {
                title,
                description,
                model,
                effort,
                reviewers,
                no_reviewer,
                depends_on,
                clear_depends_on,
                landing,
            })?;
            let t: TaskDto = client.patch_json(&task_path(&id), &body).await?;
            print(format, &t, || {
                println!("{}", ok_id_line(view().color, "updated", &t.id))
            })?;
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
            print(format, &t, || print_kv(&inspect_pairs(&t)))?;
        }
        TaskCommand::Messages { id } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let mut messages: Vec<MessageDto> =
                client.get_json(&format!("/v1/tasks/{id}/messages")).await?;
            // Newest first. The daemon serves the channel in the order it was
            // written, which is what an agent reading it as context wants; a
            // person running this wants what just happened, and a channel
            // that only grows would put it under everything else.
            messages.reverse();
            // The agents of the task, so a message reads as the skills that
            // sent it rather than as an id.
            let t: TaskDto = client.get_json(&task_path(&id)).await?;
            print_list(
                format,
                &messages,
                MESSAGES,
                |m| {
                    vec![
                        m.kind.as_str().into(),
                        party_label(&t, m.from_actor, m.from_agent_id.as_deref()),
                        party_label(&t, m.to_actor, m.to_agent_id.as_deref()),
                        m.body.clone(),
                    ]
                },
                "nothing said yet",
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
        TaskCommand::Attach { id, seat } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            crate::commands::attach::attach(client, &id, seat).await?;
        }
        TaskCommand::Logs { id, seat, follow } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let session = crate::commands::attach::resolve_tmux(client, &id, seat).await?;
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
            (false, true) => "no tasks yet — the orchestrator creates them from a goal",
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
        // named is narrowed here, as `session ls --seat` has always been.
        .filter(|t| statuses.is_empty() || statuses.contains(&t.status))
        .filter(|t| all || !statuses.is_empty() || !t.status.is_terminal())
        .collect();
    tasks.sort_by(|a, b| b.id.cmp(&a.id));
    tasks
}

/// The key/value pairs `task inspect` prints, in the order it prints them —
/// pulled out of the `Inspect` arm so the block's own content is testable
/// without a daemon behind it.
fn inspect_pairs(t: &TaskDto) -> Vec<(&'static str, Kv)> {
    vec![
        ("id", Kv::id(t.id.clone())),
        ("goal", Kv::id(t.goal_id.clone())),
        ("title", Kv::title(t.title.clone())),
        ("status", Kv::status(t.status.as_str())),
        (
            "author",
            match t.agents.iter().find(|a| a.seat == Seat::Author) {
                Some(a) => agent_pin_label(&a.skills, &a.model, a.effort.as_deref()),
                None => "-".to_string(),
            }
            .into(),
        ),
        (
            "reviewers",
            // One reviewer per line: each is its skills and the two facts
            // after them, and the review order is what the column reads down.
            t.agents
                .iter()
                .filter(|a| a.seat == Seat::Reviewer)
                .map(|a| agent_pin_label(&a.skills, &a.model, a.effort.as_deref()))
                .collect::<Vec<_>>()
                .join(INDENT)
                .into(),
        ),
        (
            "depends_on",
            Kv::id(match t.depends_on.is_empty() {
                true => "-".into(),
                false => t.depends_on.join(", "),
            }),
        ),
        ("branch", t.branch.clone().into()),
        ("worktree", dash(t.worktree_path.as_deref()).into()),
        ("tokens", usage_lines(t).into()),
        ("stalled", yes_no(t.stalled, "no").into()),
        ("merge", dash(t.merge_commit.as_deref()).into()),
        // Why a failed or cancelled task ended, which is the whole of what
        // the author that gave it up said about it.
        ("reason", dash(t.reason.as_deref()).into()),
        // The forge's own link, where the rest of a published task's story
        // is; only an author that opened one reports it.
        ("pull_request", dash(t.pr_url.as_deref()).into()),
        ("created", Kv::meta(moment(&t.created_at))),
        ("description", format!("\n---\n{}", t.description).into()),
    ]
}

/// What the task cost, spender by spender: the total first, then the
/// author and each reviewer under it, named by their profiles.
///
/// Every reviewer of the task gets a line, whether or not it has spent
/// anything: a reviewer missing from the block would read as one the task
/// does not have, and `0` is a fact where a gap is a question. An agent that
/// spent on the task and is staffed no longer is listed after them, so the
/// lines still add up to the total.
fn usage_lines(t: &TaskDto) -> String {
    let staffed: Vec<_> = t
        .agents
        .iter()
        .filter(|a| a.seat == Seat::Reviewer)
        .collect();
    let mut agents: Vec<(String, TokenUsageDto)> = vec![("author".into(), t.usage.author)];
    for r in &staffed {
        let spent = spent_by(t, &r.id).unwrap_or_default();
        agents.push((agent_label(&r.skills), spent));
    }
    agents.extend(
        t.usage
            .reviewers
            .iter()
            .filter(|u| !staffed.iter().any(|r| r.id == u.agent_id))
            .map(|u| (agent_label(&u.skills), u.usage)),
    );

    usage_block(&t.usage.total, &agents, INDENT)
}

/// What one reviewer profile spent on the task, if the daemon reported it at
/// all — a reviewer that has never been spawned has no entry.
fn spent_by(t: &TaskDto, agent_id: &str) -> Option<TokenUsageDto> {
    t.usage
        .reviewers
        .iter()
        .find(|u| u.agent_id == agent_id)
        .map(|u| u.usage)
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
        println!(
            "{}",
            status_line(view().color, "task", &t.id, t.status.as_str())
        )
    })
}

/// How a verdict names the agent that gave it: the skills that agent reviewed
/// with, and the id where the task no longer staffs it.
/// One end of a message, as a reader sees it.
///
/// An agent has no name, so it is named by the skills it works with, which is
/// the only thing about it that says anything. The orchestrator has no skills
/// and needs none: there is one of it.
fn party_label(task: &TaskDto, actor: Actor, agent_id: Option<&str>) -> String {
    let Some(agent_id) = agent_id else {
        return actor.as_str().to_string();
    };
    match task.agents.iter().find(|a| a.id == agent_id) {
        Some(agent) => agent_label(&agent.skills),
        None => agent_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_api::tasks::{AgentUsageDto, TaskUsageDto};

    use crate::commands::fixtures;
    use crate::output::{View, kv_block, style};

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

    /// One reviewer staffed on the task, named by the skills it reviews with.
    fn reviewer(id: &str, skill: &str) -> ariadne_api::tasks::TaskAgentDto {
        fixtures::agent(id, ariadne_core::Seat::Reviewer, &[skill])
    }

    /// A task nobody has run yet still says what it spent: `0`, which is a
    /// figure, where a blank or a dash would read as "the daemon does not
    /// know".
    #[test]
    fn a_task_that_has_spent_nothing_says_zero() {
        assert_eq!(ls_row(&dto(), now())[6], "↑0 0% ↓0");
        let block = usage_lines(&dto());
        assert_eq!(block.lines().next().unwrap(), "input   0  0%");
        assert!(block.contains("output  0"), "{block}");
    }

    /// The block is the total and then who spent it: the author, and every
    /// reviewer by the skills it reviews with — including the one that has
    /// never been spawned, which spent `0` rather than nothing at all.
    #[test]
    fn the_block_names_the_author_and_every_reviewer_of_the_task() {
        let t = TaskDto {
            agents: vec![
                fixtures::agent("01AUTHOR", ariadne_core::Seat::Author, &["coding"]),
                reviewer("01REV", "code-review"),
                reviewer("01SEC", "security-review"),
            ],
            usage: TaskUsageDto {
                total: usage(1_204_567, 1_100_000, 45_300),
                author: usage(1_200_000, 1_100_000, 45_000),
                reviewers: vec![AgentUsageDto {
                    agent_id: "01REV".into(),
                    skills: vec!["code-review".into()],
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
                "              author           ↑1.2M ↓45k",
                "              code-review      ↑4.6k ↓300",
                "              security-review  ↑0 ↓0",
            ]
            .join("\n")
        );
        assert_eq!(
            ls_row(&t, now())[6],
            "↑1.2M 91% ↓45k",
            "the row carries the total, and the same share"
        );
    }

    /// An agent that spent on the task and is staffed on it no longer is
    /// still listed: the lines under the total are meant to add up to it.
    #[test]
    fn a_spender_the_task_no_longer_staffs_is_still_listed() {
        let t = TaskDto {
            usage: TaskUsageDto {
                total: usage(1_000, 0, 100),
                author: usage(600, 0, 60),
                reviewers: vec![AgentUsageDto {
                    agent_id: "01GONE".into(),
                    skills: Vec::new(),
                    usage: usage(400, 0, 40),
                }],
            },
            ..dto()
        };
        assert!(
            usage_lines(&t).contains("no skills  ↑400 ↓40"),
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

    /// An agent has no name, so a message names its ends by the skills they
    /// work with — by their id where the task staffs them no longer, and by
    /// what they are where they hold no skills at all.
    #[test]
    fn a_message_names_its_ends_by_the_skills_they_work_with() {
        let t = TaskDto {
            agents: vec![reviewer("01REV", "code-review")],
            ..dto()
        };
        assert_eq!(
            party_label(&t, Actor::Reviewer, Some("01REV")),
            "code-review"
        );
        assert_eq!(party_label(&t, Actor::Reviewer, Some("01GONE")), "01GONE");
        assert_eq!(party_label(&t, Actor::Orchestrator, None), "orchestrator");
    }

    /// `task inspect` types its id, its goal, its title and its status the
    /// way a row of `task ls` would: the id and the goal dimmed, the title
    /// bold, the status carrying its glyph inside its colour. Colour is
    /// escapes and nothing else — strip them and the block reads exactly as
    /// it does with `--color never`, and everything this task leaves plain
    /// (branch, tokens, …) is untouched either way.
    #[test]
    fn the_inspect_block_types_its_id_title_and_status() {
        let t = TaskDto {
            depends_on: vec!["01DEP".into()],
            ..dto()
        };
        let pairs = inspect_pairs(&t);

        let coloured = kv_block(
            &pairs,
            &View {
                color: true,
                ..View::plain()
            },
        );
        assert!(
            coloured.contains(&style::paint(true, style::ID, &t.id)),
            "{coloured}"
        );
        assert!(
            coloured.contains(&style::paint(
                true,
                style::status("in_progress").0,
                "● in_progress"
            )),
            "{coloured}"
        );
        assert!(
            coloured.contains(&style::paint(true, style::ID, "01DEP")),
            "depends_on is a list of ids, painted whole: {coloured}"
        );
        assert!(coloured.contains("add-the-frobnicator"), "{coloured}");

        let plain = kv_block(&pairs, &View::plain());
        assert!(!plain.contains('\u{1b}'), "{plain}");
        assert_eq!(strip_escapes(&coloured), plain, "colour adds only escapes");
    }

    /// The escapes taken back out of a line, the way a reader's terminal
    /// would show it: what is left is what `--color never` prints outright.
    fn strip_escapes(line: &str) -> String {
        let mut out = String::new();
        let mut escaped = false;
        for c in line.chars() {
            match (escaped, c) {
                (false, '\u{1b}') => escaped = true,
                (true, 'm') => escaped = false,
                (true, _) => {}
                (false, c) => out.push(c),
            }
        }
        out
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
                "-",
                "yes",
                "↑0 0% ↓0",
                "add-the-frobnicator-01task",
            ]
        );
        assert_eq!(
            ls_row(&dto(), now())[5],
            "-",
            "and a task nobody published says nothing"
        );
    }
}
