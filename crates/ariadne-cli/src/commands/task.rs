//! `ariadne task ...`

pub(super) mod edit;

use anyhow::{Result, bail};
use clap::Subcommand;
use serde_json::json;

use ariadne_api::goals::GoalDto;
use ariadne_api::messages::MessageDto;
use ariadne_api::stream::EventStreamQuery;
use ariadne_api::tasks::{
    AgentAssignment, CreateTaskRequest, TaskDto, TaskListQuery, TaskTransitionDto,
};
use ariadne_api::usage::TokenUsageDto;
use ariadne_api::workflows::WorkflowStepDto;
use ariadne_client::{Client, SseEvent};
use ariadne_core::{Actor, Seat, TaskStatus};

use super::follow;
use super::resolve::{self, Kind};
use super::{
    Subject, agent_label, agent_pin_label, confirm, one_of, parse_effort_or_default, parse_model,
    query_path,
};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, Kv, UNCAPPED, age, col, dash, empty_state, local_time, moment, note,
    ok_id_line, pager, print, print_json, print_kv, print_list, status_line, usage_block,
    usage_cell, view, yes_no,
};
use ariadne_console::transcript::{Filters, Since};
use edit::{Edits, parse_agent_slot, parse_author, parse_reviewer, resolve_repo, update_request};

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
    col("step", UNCAPPED).rank(5),
    col("age", UNCAPPED).rank(4),
    col("stalled", UNCAPPED).rank(3),
    col("pr", UNCAPPED).rank(2),
    col("tokens", UNCAPPED).rank(1),
    col("branch", 40).rank(0),
];

/// Where a continuation line of `task inspect` starts: [`print_kv`] pads its
/// keys to the longest one — `pull request` — and then two spaces, and a
/// block that spills over several lines lines them all up under the first.
const INDENT: &str = "\n              ";

/// Columns of `task messages`. A message body is prose, and only its opening
/// belongs in a table — `task messages --format json` has all of it, and
/// `task messages --full` pages every body whole.
const MESSAGES: &[Column] = &[
    col("kind", UNCAPPED),
    col("from", 20).title(),
    col("to", 20).rank(1),
    col("body", 60).rank(0),
];

/// Columns of `task history`. `time` is when each transition happened rather
/// than how long ago, since a run of them is read as a sequence and the exact
/// times are what line one up against the daemon's own log; `from` and `to`
/// carry the same colour and glyph a status cell always does.
const HISTORY: &[Column] = &[
    col("time", UNCAPPED),
    col("from", UNCAPPED).status(),
    col("from step", UNCAPPED).rank(2),
    col("to", UNCAPPED).status(),
    col("to step", UNCAPPED).rank(2),
    col("actor", UNCAPPED).rank(1),
    col("reason", 60).rank(0),
];

/// What `task create --help` ends with.
const CREATE_EXAMPLES: &str = "\
Examples:
  ariadne task create <goal-id> --title \"Add the rate limiter middleware\" \\
      --author coding,testing=claude-acp:claude-sonnet-5 \\
      --reviewer code-review=codex-acp:gpt-5.6-luna

  # after another task, reasoned deeply
  ariadne task create <goal-id> --title \"Wire it up\" --depends-on <task-id> \\
      --author coding,testing=codex-acp:gpt-5.6-sol@xhigh \\
      --reviewer code-review=claude-acp:claude-opus-5@high

  # nothing to review: approved as soon as the author asks
  ariadne task create <goal-id> --title \"Write the 0.6.0 release notes\" \\
      --author documentation=claude-acp:claude-sonnet-5 --no-reviewer
";

/// What `task update --help` ends with.
const UPDATE_EXAMPLES: &str = "\
Examples:
  ariadne task update <task-id> --title \"Add the rate limiter middleware\"
  ariadne task update <task-id> --model claude-acp:claude-opus-5 --effort xhigh
  ariadne task update <task-id> --reviewer code-review=codex-acp:gpt-5.6-luna@high
  ariadne task update <task-id> --no-reviewer          # nothing left to review
  ariadne task update <task-id> --effort default       # at whatever the agent reasons it at
  ariadne task update <task-id> --clear-depends-on     # free it to start now
";

#[derive(Subcommand)]
pub(crate) enum TaskCommand {
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
        /// and model it runs on, required — and `@EFFORT` to say how
        /// deeply it reasons there
        /// (`--author coding,testing=codex-acp:gpt-5.6-sol@xhigh`).
        /// Repeatable: several authors each write the task alone, and the
        /// reviewers pick the one change that lands. Refused on a stepped
        /// goal — staff the task with `--agent` instead
        #[arg(long = "author", required_unless_present = "agents", conflicts_with = "agents", value_name = "SKILLS=MODEL[@EFFORT]", value_parser = parse_author)]
        authors: Vec<AgentAssignment>,
        /// One reviewer's skills and its model, in review order; repeatable.
        /// Spelled the same way as `--author`. Refused on a stepped goal
        /// (`--reviewer code-review=codex-acp:gpt-5.6-luna@high`)
        #[arg(long = "reviewer", value_name = "SKILLS=MODEL[@EFFORT]", conflicts_with_all = ["no_reviewer", "agents"], value_parser = parse_reviewer)]
        reviewers: Vec<AgentAssignment>,
        /// Staff no reviewer: the task is approved as soon as its author asks
        /// for review. For work with nothing to review, such as a release.
        /// Refused on a stepped goal
        #[arg(long, conflicts_with = "agents")]
        no_reviewer: bool,
        /// One workflow column's staffing: the column id, then optionally
        /// `:SKILLS` — empty means the column's own — then `=MODEL` and
        /// optionally `@EFFORT`
        /// (`--agent review:code-review=codex-acp:gpt-5.6-luna@high`).
        /// Repeatable, one per column staffed; a stepped goal takes this
        /// instead of `--author`/`--reviewer`/`--no-reviewer`
        #[arg(long = "agent", required_unless_present = "authors", value_name = "STEP[:SKILLS]=MODEL[@EFFORT]", value_parser = parse_agent_slot)]
        agents: Vec<AgentAssignment>,
        /// Id of a task that must finish before this one starts; repeatable
        #[arg(long = "depends-on", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        depends_on: Vec<String>,
        /// Which of the goal's repositories the task works in, by id or by
        /// its registered path (only needed when the goal has several)
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_repositories))]
        repo: Option<String>,
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
        /// What the author runs on: AGENT:MODEL — the id of an agent of the
        /// ACP registry and, after the colon, one model of it
        /// (codex-acp:gpt-5.3-codex). A model is required, so "default" is
        /// refused: there is nothing to hand the pin back to
        #[arg(long, value_name = "MODEL", value_parser = parse_model, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// The reasoning effort that model is run at: one of the efforts
        /// `ariadne models ls` lists for it; "default" runs it at whatever
        /// the agent runs it at
        #[arg(long, value_name = "EFFORT|default", value_parser = parse_effort_or_default, conflicts_with = "agents", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::efforts_or_default))]
        effort: Option<String>,
        /// One reviewer's skills and its model, optionally `@EFFORT`, in
        /// review order; repeatable, and replaces the task's reviewers rather
        /// than adding to them
        #[arg(long = "reviewer", value_name = "SKILLS=MODEL[@EFFORT]", conflicts_with_all = ["no_reviewer", "agents"], value_parser = parse_reviewer)]
        reviewers: Vec<AgentAssignment>,
        /// Take every reviewer off the task, leaving it approved as soon as
        /// its author asks for review
        #[arg(long, conflicts_with = "agents")]
        no_reviewer: bool,
        /// Every workflow column's staffing, replaced whole: the same
        /// `STEP[:SKILLS]=MODEL[@EFFORT]` form `task create --agent` takes;
        /// repeatable, one per column
        #[arg(long = "agent", value_name = "STEP[:SKILLS]=MODEL[@EFFORT]", conflicts_with_all = ["model", "effort"], value_parser = parse_agent_slot)]
        agents: Vec<AgentAssignment>,
        /// Id of a task that must finish first; repeatable, and replaces the
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
        /// Narrow to tasks whose current workflow column is this one
        #[arg(long)]
        step: Option<String>,
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
        /// Print each message in full, through the pager, instead of the
        /// table's truncated body — no effect on `--format json`, which
        /// already carries every body whole
        #[arg(long)]
        full: bool,
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
    /// Attach to the console of the task's agent
    Attach {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// author (default) or reviewer; agent takes the slot author does on
        /// a stepped task
        #[arg(long, conflicts_with = "step", value_parser = Spelling::<ariadne_core::Seat>::new())]
        seat: Option<ariadne_core::Seat>,
        /// Workflow column id, on a stepped task; with none, the task's
        /// current column
        #[arg(long)]
        step: Option<String>,
    },
    /// Show the transcript of the task's agent
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
        /// Print only the last N transcript items
        #[arg(long, value_name = "N")]
        tail: Option<usize>,
        /// Print items at or after an RFC 3339 time, or from a duration ago
        #[arg(long, value_name = "TIME")]
        since: Option<Since>,
        /// Print only this event kind; repeatable
        #[arg(id = "kind", long = "kind", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::transcript_kinds))]
        kinds: Vec<String>,
    },
}

pub(crate) async fn run(client: &Client, cmd: TaskCommand, format: Format) -> Result<()> {
    match cmd {
        TaskCommand::Create {
            goal,
            title,
            description,
            authors,
            reviewers,
            no_reviewer,
            agents: steps,
            depends_on,
            repo,
        } => {
            let reviewers = if no_reviewer { Vec::new() } else { reviewers };
            let goal = resolve::id(client, Kind::Goal, &goal).await?;
            let depends_on = resolve::ids(client, Kind::Task, &depends_on).await?;
            let g: GoalDto = client.get_json(&format!("/v1/goals/{goal}")).await?;
            refuse_mixed_staffing(
                g.workflow.is_some(),
                !authors.is_empty() || !reviewers.is_empty() || no_reviewer,
                !steps.is_empty(),
            )?;
            // A stepped goal is staffed by `--agent` alone; everywhere else
            // the authors come first, then the reviewers in review order,
            // which is the order the daemon reads a staffing in.
            let mut agents = steps;
            agents.extend(authors);
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
                    },
                )
                .await?;
            print(format, &t, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "created", &t.id)
                )
            })?;
        }
        TaskCommand::Update {
            id,
            title,
            description,
            model,
            effort,
            agents,
            reviewers,
            no_reviewer,
            depends_on,
            clear_depends_on,
        } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let depends_on = resolve::ids(client, Kind::Task, &depends_on).await?;
            // Only asked when a flag that could be either staffing model was
            // actually given: a plain `--title` edit touches neither, and
            // costs no extra round trip to say so.
            if !reviewers.is_empty() || no_reviewer || !agents.is_empty() {
                let t: TaskDto = client.get_json(&task_path(&id)).await?;
                let g: GoalDto = client.get_json(&format!("/v1/goals/{}", t.goal_id)).await?;
                refuse_mixed_staffing(
                    g.workflow.is_some(),
                    !reviewers.is_empty() || no_reviewer,
                    !agents.is_empty(),
                )?;
            }
            let body = update_request(Edits {
                title,
                description,
                model,
                effort,
                agents,
                reviewers,
                no_reviewer,
                depends_on,
                clear_depends_on,
            })?;
            let t: TaskDto = client.patch_json(&task_path(&id), &body).await?;
            print(format, &t, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "updated", &t.id)
                )
            })?;
        }
        TaskCommand::Ls {
            goal,
            statuses,
            all,
            step,
            watch,
        } => ls(client, goal, statuses, all, step, watch, format).await?,
        TaskCommand::Inspect { id } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let t: TaskDto = client.get_json(&task_path(&id)).await?;
            let g: GoalDto = client.get_json(&format!("/v1/goals/{}", t.goal_id)).await?;
            print(format, &t, || {
                print_kv(&inspect_pairs(&t, stepped_view(&g)))
            })?;
        }
        TaskCommand::Messages { id, full } => {
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
            if full && format == Format::Table {
                match messages.is_empty() {
                    true => note("nothing said yet"),
                    false => pager::page(&full_messages(&t, &messages))?,
                }
                return Ok(());
            }
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
            print_list(format, &rows, HISTORY, history_row, "no transitions yet")?;
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
        TaskCommand::Attach { id, seat, step } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            match (step, seat) {
                (Some(step), _) => crate::commands::attach::attach_step(client, &id, &step).await?,
                (None, Some(seat)) => {
                    crate::commands::attach::attach(client, &id, Some(seat)).await?
                }
                // Neither flag: a stepped task's agent sits in `Seat::Agent`,
                // not `Seat::Author`, so the task's own current column says
                // which session that is — a pending task with no column
                // yet falls through to the plain seat-based lookup, which
                // reports the same "nothing to attach to" it always did.
                (None, None) => {
                    let t: TaskDto = client.get_json(&task_path(&id)).await?;
                    match t.step {
                        Some(step) => {
                            crate::commands::attach::attach_step(client, &id, &step).await?
                        }
                        None => crate::commands::attach::attach(client, &id, None).await?,
                    }
                }
            }
        }
        TaskCommand::Logs {
            id,
            seat,
            follow,
            tail,
            since,
            kinds,
        } => {
            let id = resolve::id(client, Kind::Task, &id).await?;
            let session = crate::commands::attach::resolve_live(client, &id, seat).await?;
            crate::commands::console::logs(
                client,
                &session.id,
                follow,
                Filters { tail, since, kinds },
                format,
            )
            .await?;
        }
    }
    Ok(())
}

fn task_path(id: &str) -> String {
    format!("/v1/tasks/{id}")
}

/// Whether `task inspect` reads a stepped task's view: the goal's own
/// `workflow`, not the task's own `step` — a stepped task's column is null
/// until it starts, so `t.step` alone would read a pending one as
/// unstepped and print author/reviewer rows it does not have.
fn stepped_view(g: &GoalDto) -> Option<&GoalDto> {
    g.workflow.is_some().then_some(g)
}

/// `--author`/`--reviewer`/`--no-reviewer` and `--agent` are the two
/// staffing models a task takes, and never both: a stepped goal takes only
/// the second, since its columns say what runs where; an unstepped one
/// takes only the first, since it has no column for `--agent` to name.
/// Checked against the goal rather than the flags alone, so a goal that
/// started unstepped and gained a workflow since is read as it stands now.
fn refuse_mixed_staffing(stepped: bool, legacy_used: bool, agent_used: bool) -> Result<()> {
    if stepped && legacy_used {
        bail!(
            "--author, --reviewer and --no-reviewer are refused on a stepped goal — \
             staff the task with --agent instead"
        );
    }
    if !stepped && agent_used {
        bail!(
            "--agent is refused on a goal with no workflow — staff the task with \
             --author instead"
        );
    }
    Ok(())
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
    step: Option<String>,
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
        return render(client, goal, &statuses, all, step.as_deref(), format).await;
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
        render(
            client,
            goal.clone(),
            &statuses,
            all,
            step.as_deref(),
            format,
        )
        .await
    })
    .await
}

/// The table as it stands, read afresh. `--step` narrows client-side: the
/// daemon's own filter is the goal and the one status `one_of` can ask for,
/// the same split every other listing follows.
async fn render(
    client: &Client,
    goal: Option<String>,
    statuses: &[TaskStatus],
    all: bool,
    step: Option<&str>,
    format: Format,
) -> Result<()> {
    let filtered = goal.is_some() || !statuses.is_empty() || step.is_some();
    // `GET /v1/tasks` takes one status, so one is asked for and the rest is
    // narrowed on the answer — with the live/finished split.
    let status = one_of(statuses);
    let path = query_path("/v1/tasks", &TaskListQuery { goal, status })?;
    let tasks: Vec<TaskDto> = client.get_json(&path).await?;
    let tasks = visible(tasks, all, statuses, step);
    let now = chrono::Utc::now();
    print_list(
        format,
        &tasks,
        LS,
        |t| ls_row(t, now),
        // An empty list under a filter is not an empty system, and saying so
        // would send the reader looking for tasks that are right there.
        match (filtered, all) {
            (true, _) => empty_state("No tasks match that filter.", Some("ariadne task ls")),
            (false, true) => empty_state("No tasks yet.", Some("ariadne goal create --help")),
            (false, false) => empty_state("No tasks are under way.", Some("ariadne task ls --all")),
        },
    )
}

/// One row of `task ls`, in [`LS`]'s order.
fn ls_row(t: &TaskDto, now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    vec![
        t.id.clone(),
        t.title.clone(),
        t.status.as_str().into(),
        dash(t.step.as_deref()),
        age(&t.created_at, now),
        yes_no(t.stalled, "-"),
        yes_no(t.pr_url.is_some(), "-"),
        usage_cell(&t.usage.total),
        t.branch.clone(),
    ]
}

/// One row of `task history`, in [`HISTORY`]'s order. The columns a move
/// crossed travel beside the statuses they are a reading of, and a dash is
/// the task they moved before any workflow carried a step at all.
fn history_row(t: &TaskTransitionDto) -> Vec<String> {
    vec![
        local_time(&t.created_at),
        t.from_status.clone(),
        dash(t.from_step.as_deref()),
        t.to_status.clone(),
        dash(t.to_step.as_deref()),
        t.actor.clone(),
        dash(t.reason.as_deref()),
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
fn visible(
    tasks: Vec<TaskDto>,
    all: bool,
    statuses: &[TaskStatus],
    step: Option<&str>,
) -> Vec<TaskDto> {
    let mut tasks: Vec<TaskDto> = tasks
        .into_iter()
        // `--status` is asked of the daemon one at a time; the rest of what it
        // named is narrowed here, as `session ls --seat` has always been.
        .filter(|t| statuses.is_empty() || statuses.contains(&t.status))
        .filter(|t| all || !statuses.is_empty() || !t.status.is_terminal())
        // `--step` is a column id, which the daemon's own filters know
        // nothing of: narrowed here, the same way.
        .filter(|t| step.is_none() || t.step.as_deref() == step)
        .collect();
    tasks.sort_by(|a, b| b.id.cmp(&a.id));
    tasks
}

/// The key/value pairs `task inspect` prints, in the order it prints them —
/// pulled out of the `Inspect` arm so the block's own content is testable
/// without a daemon behind it.
fn inspect_pairs(t: &TaskDto, goal: Option<&GoalDto>) -> Vec<(&'static str, Kv)> {
    let mut pairs = vec![
        ("id", Kv::id(t.id.clone())),
        ("goal", Kv::id(t.goal_id.clone())),
        ("title", Kv::title(t.title.clone())),
        ("status", Kv::status(t.status.as_str())),
    ];
    match goal {
        // A stepped task names its workflow and its current column, and
        // lists the agent of every column rather than an author and
        // reviewers it does not have.
        Some(g) => {
            pairs.push((
                "workflow",
                g.workflow.clone().unwrap_or_else(|| "-".into()).into(),
            ));
            pairs.push(("step", dash(t.step.as_deref()).into()));
            pairs.push(("agents", step_lines(t, &g.steps).into()));
        }
        None => {
            let authors: Vec<_> = t.agents.iter().filter(|a| a.seat == Seat::Author).collect();
            pairs.push((
                "author",
                match authors.as_slice() {
                    [] => "-".to_string(),
                    [a] => agent_pin_label(&a.skills, &a.model, a.effort.as_deref()),
                    // Several authors: each on its own line with the branch
                    // it owns, and the one the reviewers picked marked as
                    // such.
                    several => several
                        .iter()
                        .map(|a| {
                            let label = agent_pin_label(&a.skills, &a.model, a.effort.as_deref());
                            let branch = a.branch.as_deref().unwrap_or("-");
                            let picked = match t.picked_agent_id.as_deref() == Some(a.id.as_str()) {
                                true => " — picked",
                                false => "",
                            };
                            format!("{label} on {branch}{picked}")
                        })
                        .collect::<Vec<_>>()
                        .join(INDENT),
                }
                .into(),
            ));
            pairs.push((
                "reviewers",
                // One reviewer per line: each is its skills and the two
                // facts after them, and the review order is what the column
                // reads down.
                t.agents
                    .iter()
                    .filter(|a| a.seat == Seat::Reviewer)
                    .map(|a| agent_pin_label(&a.skills, &a.model, a.effort.as_deref()))
                    .collect::<Vec<_>>()
                    .join(INDENT)
                    .into(),
            ));
            // The pick, on the tasks that have one to show: who each
            // reviewer chose, one line per pick. A one-author task prints
            // exactly what it always did.
            if authors.len() > 1 {
                pairs.push((
                    "picks",
                    match t.picks.is_empty() {
                        true => "-".to_string(),
                        false => t
                            .picks
                            .iter()
                            .map(|p| {
                                format!(
                                    "{} picked {}",
                                    staffed_label(t, &p.reviewer_agent_id),
                                    staffed_label(t, &p.author_agent_id)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(INDENT),
                    }
                    .into(),
                ));
            }
        }
    }
    pairs.extend(vec![
        (
            "depends on",
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
        ("pull request", dash(t.pr_url.as_deref()).into()),
        ("created", Kv::meta(moment(&t.created_at))),
        ("description", format!("\n---\n{}", t.description).into()),
    ]);
    pairs
}

/// A staffed agent named for a reader: the skills it carries, or its id
/// where the task no longer staffs it.
fn staffed_label(t: &TaskDto, agent_id: &str) -> String {
    match t.agents.iter().find(|a| a.id == agent_id) {
        Some(a) => agent_label(&a.skills),
        None => agent_id.to_string(),
    }
}

/// One line per column of a stepped task's workflow, in column order: the
/// column, the skills its agent carries, the pin it runs on, and its
/// session — a column nobody has staffed yet reads as unstaffed rather than
/// vanishing from the block.
fn step_lines(t: &TaskDto, steps: &[WorkflowStepDto]) -> String {
    steps
        .iter()
        .map(|s| {
            match t
                .agents
                .iter()
                .find(|a| a.step.as_deref() == Some(s.id.as_str()))
            {
                Some(a) => format!(
                    "{} · {} · {}",
                    s.title,
                    agent_pin_label(&a.skills, &a.model, a.effort.as_deref()),
                    dash(a.session_id.as_deref())
                ),
                None => format!("{} · unstaffed", s.title),
            }
        })
        .collect::<Vec<_>>()
        .join(INDENT)
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
            status_line(view().color, view().quiet, "task", &t.id, t.status.as_str())
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

/// One message of `task messages --full`: the same header its row of the
/// table carries, over the body exactly as it came — no cut, since printing
/// it whole is the point of `--full`.
fn full_message(t: &TaskDto, m: &MessageDto) -> String {
    format!(
        "{} {} -> {} ({})\n{}",
        local_time(&m.created_at),
        party_label(t, m.from_actor, m.from_agent_id.as_deref()),
        party_label(t, m.to_actor, m.to_agent_id.as_deref()),
        m.kind.as_str(),
        m.body
    )
}

/// Every message of `task messages --full`, in the order they were passed —
/// one text handed to the pager, rather than a page per message.
fn full_messages(t: &TaskDto, messages: &[MessageDto]) -> String {
    messages
        .iter()
        .map(|m| full_message(t, m))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_api::tasks::{AgentUsageDto, TaskUsageDto};
    use ariadne_core::MessageKind;

    use crate::commands::fixtures;
    use crate::output::{View, kv_block, render_table, style};

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
        assert_eq!(ls_row(&dto(), now())[7], "↑0 0.0% ↓0");
        let block = usage_lines(&dto());
        assert_eq!(block.lines().next().unwrap(), "input   0  0.0%");
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
                agents: Vec::new(),
                total: usage(1_204_567, 1_100_000, 45_300),
                author: usage(1_200_000, 1_100_000, 45_000),
                reviewers: vec![AgentUsageDto {
                    step: None,
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
                "input   1.2M  91.3%",
                "              output   45k",
                "              author           ↑1.2M ↓45k",
                "              code-review      ↑4.6k ↓300",
                "              security-review  ↑0 ↓0",
            ]
            .join("\n")
        );
        assert_eq!(
            ls_row(&t, now())[7],
            "↑1.2M 91.3% ↓45k",
            "the row carries the total, and the same share"
        );
    }

    /// An agent that spent on the task and is staffed on it no longer is
    /// still listed: the lines under the total are meant to add up to it.
    #[test]
    fn a_spender_the_task_no_longer_staffs_is_still_listed() {
        let t = TaskDto {
            usage: TaskUsageDto {
                agents: Vec::new(),
                total: usage(1_000, 0, 100),
                author: usage(600, 0, 60),
                reviewers: vec![AgentUsageDto {
                    step: None,
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
        let pairs = inspect_pairs(&t, None);
        let keys: Vec<_> = pairs.iter().map(|(key, _)| *key).collect();
        assert!(keys.contains(&"depends on"), "{keys:?}");
        assert!(keys.contains(&"pull request"), "{keys:?}");
        assert!(keys.iter().all(|key| !key.contains('_')), "{keys:?}");

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
            "depends on is a list of ids, painted whole: {coloured}"
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
                "-",
                "3h",
                "-",
                "yes",
                "↑0 0.0% ↓0",
                "add-the-frobnicator-01task",
            ]
        );
        assert_eq!(
            ls_row(&dto(), now())[6],
            "-",
            "and a task nobody published says nothing"
        );
    }

    /// `--step` narrows the list to the one column named, client-side: the
    /// daemon knows nothing of workflow columns in its own task filters.
    #[test]
    fn step_narrows_the_list_to_the_named_column() {
        let develop = TaskDto {
            id: "01DEVELOP".into(),
            step: Some("develop".into()),
            ..dto()
        };
        let review = TaskDto {
            id: "01REVIEW".into(),
            step: Some("review".into()),
            ..dto()
        };
        let tasks = visible(
            vec![develop.clone(), review.clone()],
            true,
            &[],
            Some("review"),
        );
        assert_eq!(
            tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["01REVIEW"]
        );
        let unfiltered = visible(vec![develop, review], true, &[], None);
        assert_eq!(unfiltered.len(), 2, "no --step leaves every column in");
    }

    /// A stepped task has no author or reviewers to show: it names its
    /// workflow, its current column, and the agent of every one of them, in
    /// column order — a column nobody has staffed yet still gets a line.
    #[test]
    fn a_stepped_task_lists_its_workflow_and_every_columns_agent() {
        use ariadne_api::workflows::WorkflowStepDto;

        let t = TaskDto {
            step: Some("review".into()),
            agents: vec![ariadne_api::tasks::TaskAgentDto {
                step: Some("review".into()),
                session_id: Some("01SESSION".into()),
                ..fixtures::agent("01REV", Seat::Agent, &["code-review"])
            }],
            ..dto()
        };
        let g = GoalDto {
            workflow: Some("develop-review-merge".into()),
            steps: vec![
                WorkflowStepDto {
                    id: "develop".into(),
                    title: "Develop".into(),
                    description: String::new(),
                    skills: vec!["coding".into()],
                    rank: None,
                    gate: None,
                },
                WorkflowStepDto {
                    id: "review".into(),
                    title: "Review".into(),
                    description: String::new(),
                    skills: vec!["code-review".into()],
                    rank: None,
                    gate: None,
                },
            ],
            ..fixtures::goal("01GOAL", "Ship the board")
        };
        let pairs = inspect_pairs(&t, Some(&g));
        let keys: Vec<_> = pairs.iter().map(|(key, _)| *key).collect();
        assert!(keys.contains(&"workflow"), "{keys:?}");
        assert!(keys.contains(&"step"), "{keys:?}");
        assert!(keys.contains(&"agents"), "{keys:?}");
        assert!(!keys.contains(&"author"), "{keys:?}");
        assert!(!keys.contains(&"reviewers"), "{keys:?}");

        let block = kv_block(&pairs, &View::plain());
        assert!(
            block.contains("Develop · unstaffed"),
            "an unstaffed column still gets a line: {block}"
        );
        assert!(
            block.contains("Review · code-review · stub:test-model · 01SESSION"),
            "{block}"
        );
    }

    /// `task inspect` reads whether a task is stepped off the goal's own
    /// `workflow`, never off the task's `step` — a task pending its first
    /// column carries no `step` of its own yet, and must still get the
    /// stepped view rather than one asking for an author it has none of.
    #[test]
    fn stepped_view_reads_the_goals_workflow_not_the_tasks_own_step() {
        let stepped = GoalDto {
            workflow: Some("develop-review-merge".into()),
            ..fixtures::goal("01GOAL", "Ship the board")
        };
        assert!(stepped_view(&stepped).is_some());
        let unstepped = fixtures::goal("01GOAL", "Ship the board");
        assert!(stepped_view(&unstepped).is_none());
    }

    /// 26-char, ULID-shaped ids a `resolve::id` call takes as whole, with no
    /// list fetch behind it — all these tests need from the daemon is the
    /// one goal or task route they stub.
    const GOAL_ID: &str = "01gggggggggggggggggggggggg";
    const TASK_ID: &str = "01tttttttttttttttttttttttt";

    /// `task create` is staffed by `--author`/`--reviewer`/`--no-reviewer`
    /// or by `--agent`, never both: the goal says which, and a legacy flag
    /// on a stepped goal is refused before the task is ever sent.
    #[tokio::test]
    async fn legacy_staffing_on_a_stepped_goal_is_refused_before_anything_is_sent() {
        use axum::{Json, Router, routing::get};

        async fn goal() -> Json<GoalDto> {
            Json(GoalDto {
                workflow: Some("develop-review-merge".into()),
                ..fixtures::goal(GOAL_ID, "Ship the board")
            })
        }
        let app = Router::new().route("/v1/goals/{id}", get(goal));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        crate::output::init(crate::output::View::plain());
        let err = run(
            &Client::tcp(format!("http://{address}")),
            TaskCommand::Create {
                goal: GOAL_ID.into(),
                title: "Do it".into(),
                description: String::new(),
                authors: vec![parse_author("coding=stub:test-model").unwrap()],
                reviewers: Vec::new(),
                no_reviewer: false,
                agents: Vec::new(),
                depends_on: Vec::new(),
                repo: None,
            },
            Format::Json,
        )
        .await
        .expect_err("legacy staffing on a stepped goal");
        server.abort();
        assert!(err.to_string().contains("--agent"), "{err}");
    }

    /// The other way round: `--agent` on a goal with no workflow is refused
    /// too, since there is no column for it to name.
    #[tokio::test]
    async fn agent_staffing_on_an_unstepped_goal_is_refused_before_anything_is_sent() {
        use axum::{Json, Router, routing::get};

        async fn goal() -> Json<GoalDto> {
            Json(fixtures::goal(GOAL_ID, "Ship the board"))
        }
        let app = Router::new().route("/v1/goals/{id}", get(goal));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        crate::output::init(crate::output::View::plain());
        let err = run(
            &Client::tcp(format!("http://{address}")),
            TaskCommand::Create {
                goal: GOAL_ID.into(),
                title: "Do it".into(),
                description: String::new(),
                authors: Vec::new(),
                reviewers: Vec::new(),
                no_reviewer: false,
                agents: vec![parse_agent_slot("develop=stub:test-model").unwrap()],
                depends_on: Vec::new(),
                repo: None,
            },
            Format::Json,
        )
        .await
        .expect_err("--agent on an unstepped goal");
        server.abort();
        assert!(err.to_string().contains("--author"), "{err}");
    }

    /// `task update --reviewer` reads the task's own goal before sending
    /// anything: a stepped task's reviewers are not its to restage.
    #[tokio::test]
    async fn task_update_reads_the_goal_before_refusing_legacy_reviewers_on_a_stepped_task() {
        use axum::{Json, Router, routing::get};

        async fn task() -> Json<TaskDto> {
            Json(TaskDto {
                step: Some("review".into()),
                goal_id: GOAL_ID.into(),
                ..fixtures::task(TASK_ID, GOAL_ID)
            })
        }
        async fn goal() -> Json<GoalDto> {
            Json(GoalDto {
                workflow: Some("develop-review-merge".into()),
                ..fixtures::goal(GOAL_ID, "Ship the board")
            })
        }
        let app = Router::new()
            .route("/v1/tasks/{id}", get(task))
            .route("/v1/goals/{id}", get(goal));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        crate::output::init(crate::output::View::plain());
        let err = run(
            &Client::tcp(format!("http://{address}")),
            TaskCommand::Update {
                id: TASK_ID.into(),
                title: None,
                description: None,
                model: None,
                effort: None,
                agents: Vec::new(),
                reviewers: vec![parse_reviewer("code-review=stub:test-model").unwrap()],
                no_reviewer: false,
                depends_on: Vec::new(),
                clear_depends_on: false,
            },
            Format::Json,
        )
        .await
        .expect_err("legacy reviewer on a stepped task");
        server.abort();
        assert!(err.to_string().contains("--agent"), "{err}");
    }

    /// A transition, in [`HISTORY`]'s order — and a dash where it carries no
    /// reason or no step, the way every optional cell in this CLI reads.
    #[test]
    fn a_history_row_carries_the_transition_and_a_dash_for_no_reason() {
        let t = TaskTransitionDto {
            from_step: None,
            to_step: None,
            id: "01TRANS".into(),
            from_status: "in_progress".into(),
            to_status: "under_review".into(),
            actor: "author".into(),
            reason: None,
            created_at: fixtures::NOW.into(),
        };
        let row = history_row(&t);
        assert_eq!(
            row.len(),
            HISTORY.len(),
            "a row per column, in HISTORY's order"
        );
        assert_eq!(row[0], local_time(fixtures::NOW));
        assert_eq!(row[1], "in_progress");
        assert_eq!(row[2], "-", "no step on an unstepped task's move");
        assert_eq!(row[3], "under_review");
        assert_eq!(row[4], "-");
        assert_eq!(row[5], "author");
        assert_eq!(row[6], "-");

        let reasoned = TaskTransitionDto {
            reason: Some("asked for review".into()),
            ..t
        };
        assert_eq!(history_row(&reasoned)[6], "asked for review");
    }

    /// A stepped task's move carries the column it left and the one it
    /// entered, beside the statuses those columns sit under.
    #[test]
    fn a_history_row_paints_the_columns_a_move_crossed() {
        let t = TaskTransitionDto {
            from_step: Some("develop".into()),
            to_step: Some("review".into()),
            id: "01TRANS".into(),
            from_status: "in_progress".into(),
            to_status: "in_progress".into(),
            actor: "agent".into(),
            reason: Some("ready for review".into()),
            created_at: fixtures::NOW.into(),
        };
        let row = history_row(&t);
        assert_eq!(row[2], "develop");
        assert_eq!(row[4], "review");
    }

    /// `task history` colours `from` and `to` the way every status cell is —
    /// glyph inside the colour — the same contract `task ls`'s status column
    /// keeps; with colour off, the row is bare words and no escapes.
    #[test]
    fn history_paints_the_from_and_to_statuses() {
        let row = history_row(&TaskTransitionDto {
            from_step: None,
            to_step: None,
            id: "01TRANS".into(),
            from_status: "in_progress".into(),
            to_status: "under_review".into(),
            actor: "author".into(),
            reason: None,
            created_at: fixtures::NOW.into(),
        });
        let rows = vec![row];

        let coloured = render_table(
            HISTORY,
            &rows,
            &View {
                color: true,
                ..View::plain()
            },
        )
        .expect("render");
        assert!(
            coloured.contains(&style::paint(
                true,
                style::status("in_progress").0,
                "● in_progress"
            )),
            "{coloured}"
        );
        assert!(
            coloured.contains(&style::paint(
                true,
                style::status("under_review").0,
                "● under_review"
            )),
            "{coloured}"
        );

        let plain = render_table(HISTORY, &rows, &View::plain()).expect("render");
        assert!(!plain.contains('\u{1b}'), "{plain}");
        assert_eq!(strip_escapes(&coloured), plain, "colour adds only escapes");
    }

    /// `--full` prints the same header a table row carries, over the body
    /// exactly as it came — proven directly, with no pager or terminal
    /// behind it.
    #[test]
    fn a_full_message_carries_its_header_and_its_whole_body() {
        let t = TaskDto {
            agents: vec![reviewer("01REV", "code-review")],
            ..dto()
        };
        let whole_body = "a".repeat(200);
        let m = MessageDto {
            id: "01MSG".into(),
            goal_id: t.goal_id.clone(),
            task_id: Some(t.id.clone()),
            kind: MessageKind::Message,
            from_actor: Actor::Reviewer,
            from_agent_id: Some("01REV".into()),
            from_session: None,
            to_actor: Actor::Author,
            to_agent_id: None,
            body: whole_body.clone(),
            delivered_at: None,
            created_at: fixtures::NOW.into(),
        };

        let text = full_message(&t, &m);
        assert!(text.contains(&whole_body), "the body is not cut: {text}");
        assert!(
            text.contains("code-review -> author"),
            "the header names both ends: {text}"
        );

        let joined = full_messages(&t, &[m.clone(), m]);
        assert_eq!(
            joined.matches(&whole_body).count(),
            2,
            "every message is printed, not just the first: {joined}"
        );
    }
}
