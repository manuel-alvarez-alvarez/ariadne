//! `ariadne task ...`

use anyhow::{Result, bail};
use clap::Subcommand;
use serde_json::json;

use ariadne_api::goals::GoalDto;
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::reviews::ReviewDto;
use ariadne_api::tasks::{
    CreateTaskRequest, TaskDto, TaskListQuery, TaskTransitionDto, UpdateTaskRequest,
};
use ariadne_client::Client;
use ariadne_core::TaskStatus;

use super::{ProfileNames, confirm, message_line};
use crate::output::{
    Column, Format, UNCAPPED, local_time, note, print_json, print_kv, print_table,
};
use crate::query::query_path;

/// Columns of `task ls`. Titles and branches are the long ones: a task whose
/// title runs to a paragraph would otherwise push status and round off-screen.
///
/// The landing pair sits together: who lands the task, and whether they have
/// published it yet. `integrator` is `Name (id)` capped like the `reviewer`
/// column of `task reviews`, the CLI's other table that names a profile;
/// `--no-trunc` gives it whole. `pr` is the number rather than the URL — a
/// table is for scanning, and which tasks have a pull request open is the
/// question it answers, with `task inspect` and `--format json` carrying the
/// link itself.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("title", 48),
    ("status", UNCAPPED),
    ("round", UNCAPPED),
    ("stalled", UNCAPPED),
    ("integrator", 24),
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
        /// Integrator profile id or name that lands the task once its
        /// reviewers approve it: onto the base branch with git, or as a pull
        /// request for a person to merge
        #[arg(long, default_value = "Local Integrator", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::integrator_profiles))]
        integrator: String,
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
    /// Title, description, reviewers, integrator and dependencies, while the
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
        /// Integrator profile id or name that lands the task once its
        /// reviewers approve it, replacing the one it was created with
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::integrator_profiles))]
        integrator: Option<String>,
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
        /// the integrator that lands it or the goal's planner, by profile id
        /// or name, or "user" for the human. An addressed recipient is woken
        /// to read it.
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
        /// engineer (default), reviewer or integrator
        #[arg(long, value_enum)]
        role: Option<ariadne_core::Role>,
    },
    /// Show recent terminal output of the task's agent
    Logs {
        /// Task id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        id: String,
        /// engineer (default), reviewer or integrator
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
            integrator,
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
                        integrator_profile: integrator,
                        reviewer_profiles: reviewers,
                        depends_on,
                    },
                )
                .await?;
            match format {
                Format::Json => print_json(&t)?,
                Format::Table => println!("{}", t.id),
            }
        }
        TaskCommand::Update {
            id,
            title,
            description,
            reviewers,
            integrator,
            depends_on,
            clear_depends_on,
        } => {
            let body = update_request(
                title,
                description,
                reviewers,
                integrator,
                depends_on,
                clear_depends_on,
            )?;
            let t: TaskDto = client.patch_json(&format!("/v1/tasks/{id}"), &body).await?;
            match format {
                Format::Json => print_json(&t)?,
                Format::Table => println!("updated {}", t.id),
            }
        }
        TaskCommand::Ls {
            goal,
            status,
            no_trunc,
        } => {
            let filtered = goal.is_some() || status.is_some();
            let path = query_path("/v1/tasks", &TaskListQuery { goal, status })?;
            let tasks: Vec<TaskDto> = client.get_json(&path).await?;
            match format {
                Format::Json => print_json(&tasks)?,
                Format::Table => {
                    let profiles = ProfileNames::fetch(client).await;
                    print_table(
                        LS,
                        &tasks
                            .iter()
                            .map(|t| ls_row(&profiles, t))
                            .collect::<Vec<_>>(),
                        no_trunc,
                    );
                    if tasks.is_empty() {
                        // An empty list under a filter is not an empty
                        // system, and saying so would send the reader
                        // looking for tasks that are right there.
                        note(if filtered {
                            "no tasks match that filter"
                        } else {
                            "no tasks yet — the planner creates them from a goal"
                        });
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
                    // Read before the block below moves the task apart.
                    let integrator = integrator_label(&profiles, &t);
                    print_kv(&[
                        ("id", t.id),
                        ("goal", t.goal_id),
                        ("title", t.title),
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
                            // One reviewer per line: each is now a mention and
                            // the two facts after it, and the review order is
                            // what the column reads down.
                            t.reviewers
                                .iter()
                                .map(|r| {
                                    profiles.pinned_label(
                                        &r.profile_id,
                                        r.agent_kind,
                                        r.model.as_deref(),
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n             "),
                        ),
                        ("integrator", integrator),
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
                        // The forge's own link, where the rest of a published
                        // task's story is; only an integrator that opened one
                        // reports it.
                        ("pull_request", t.pr_url.unwrap_or_else(|| "-".into())),
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
                        println!("{}", message_line(m));
                    }
                    if msgs.is_empty() {
                        note("no messages yet");
                    }
                }
            }
        }
        TaskCommand::Msg { id, body, to } => {
            let m: MessageDto = client
                .post_json(
                    &format!("/v1/tasks/{id}/messages"),
                    &CreateMessageRequest { body, to },
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
        TaskCommand::Cancel { id, yes } => {
            let t: TaskDto = client.get_json(&format!("/v1/tasks/{id}")).await?;
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

/// One row of `task ls`, in [`LS`]'s order.
fn ls_row(profiles: &ProfileNames, t: &TaskDto) -> Vec<String> {
    vec![
        t.id.clone(),
        t.title.clone(),
        t.status.as_str().into(),
        t.review_round.to_string(),
        if t.stalled { "yes".into() } else { "-".into() },
        integrator_label(profiles, t),
        t.pr_number.map_or("-".into(), |n| format!("#{n}")),
        t.branch.clone(),
    ]
}

/// Who lands this task, for `ls` and for `inspect` — one spelling, so the two
/// never disagree about it.
///
/// Assigned like the engineer, but pinned to nothing: an integrator is only
/// started once the reviewers have approved, so what runs is what its profile
/// says then, and there is no snapshot to prefer.
fn integrator_label(profiles: &ProfileNames, t: &TaskDto) -> String {
    profiles.label(&t.integrator_profile_id)
}

/// What `task cancel` asks before the work is thrown away: cancelling is
/// irreversible and the id alone does not say which work that is, so the
/// question names the task and where it got to.
fn cancel_question(t: &TaskDto) -> String {
    format!("Cancel task \"{}\" ({})?", t.title, t.status.as_str())
}

/// What a mutation prints: the task it produced, or a sentence about it.
fn print_status(t: &TaskDto, format: Format) -> Result<()> {
    match format {
        Format::Json => print_json(t)?,
        Format::Table => println!("task {} is now {}", t.id, t.status.as_str()),
    }
    Ok(())
}

/// The PATCH body of `task update`, or the reason there is nothing to send.
///
/// A flag that was not given is `None` — the field keeps what the task has.
/// The two list flags are all-or-nothing by design: they replace the list they
/// name, and `--clear-depends-on` is how an empty one is spelled, since a
/// repeatable flag cannot be given zero times on purpose.
fn update_request(
    title: Option<String>,
    description: Option<String>,
    reviewers: Vec<String>,
    integrator: Option<String>,
    depends_on: Vec<String>,
    clear_depends_on: bool,
) -> Result<UpdateTaskRequest> {
    let req = UpdateTaskRequest {
        title,
        description,
        reviewer_profiles: (!reviewers.is_empty()).then_some(reviewers),
        integrator_profile: integrator,
        depends_on: match (clear_depends_on, depends_on.is_empty()) {
            (true, _) => Some(Vec::new()),
            (false, true) => None,
            (false, false) => Some(depends_on),
        },
    };
    // An empty PATCH would still reach the daemon and still be refused on a
    // started task, which reads as a failure the caller never asked for.
    if req.title.is_none()
        && req.description.is_none()
        && req.reviewer_profiles.is_none()
        && req.integrator_profile.is_none()
        && req.depends_on.is_none()
    {
        bail!(
            "nothing to update — pass --title, --description, --reviewer, \
             --integrator or --depends-on"
        );
    }
    Ok(req)
}

/// A `--repo` argument as the repo id the API wants.
///
/// The goal's repositories answer to their id or to their registered path —
/// the two spellings `goal inspect` prints — because nobody types a ULID they
/// have not been given.
async fn resolve_repo(client: &Client, goal_id: &str, spec: &str) -> Result<String> {
    let g: GoalDto = client.get_json(&format!("/v1/goals/{goal_id}")).await?;
    match pick_repo(&g.repos, spec) {
        Some(id) => Ok(id),
        None => bail!(
            "goal {goal_id} has no repo \"{spec}\" — it has {}",
            g.repos
                .iter()
                .map(|r| format!("{} ({})", r.path, r.id))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The id of the goal repository a `--repo` argument names, by id or by path.
fn pick_repo(repos: &[RepositoryDto], spec: &str) -> Option<String> {
    repos
        .iter()
        .find(|r| r.id == spec || r.path == spec)
        .map(|r| r.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repos() -> Vec<RepositoryDto> {
        let repo = |id: &str, path: &str| RepositoryDto {
            id: id.into(),
            path: path.into(),
            base_branch: "main".into(),
            description: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        vec![
            repo("01REPOAPI", "/home/me/api"),
            repo("01REPOUI", "/home/me/ui"),
        ]
    }

    #[test]
    fn a_repo_is_named_by_id_or_by_path() {
        assert_eq!(pick_repo(&repos(), "01REPOUI").as_deref(), Some("01REPOUI"));
        assert_eq!(
            pick_repo(&repos(), "/home/me/api").as_deref(),
            Some("01REPOAPI")
        );
        assert_eq!(pick_repo(&repos(), "/home/me/other"), None);
    }

    /// The lists replace rather than extend, so an absent flag must not send
    /// an empty list and wipe what the task has.
    #[test]
    fn a_flag_that_was_not_given_is_left_alone() {
        let req =
            update_request(Some("new".into()), None, vec![], None, vec![], false).expect("body");
        assert_eq!(req.title.as_deref(), Some("new"));
        assert!(req.description.is_none());
        assert!(req.reviewer_profiles.is_none());
        assert!(req.integrator_profile.is_none());
        assert!(req.depends_on.is_none());
    }

    #[test]
    fn the_lists_are_replaced_by_what_was_given() {
        let req = update_request(
            None,
            None,
            vec!["Reviewer".into(), "rev-strict".into()],
            None,
            vec!["01TASK".into()],
            false,
        )
        .expect("body");
        assert_eq!(
            req.reviewer_profiles.as_deref(),
            Some(["Reviewer".to_string(), "rev-strict".to_string()].as_slice())
        );
        assert_eq!(
            req.depends_on.as_deref(),
            Some(["01TASK".to_string()].as_slice())
        );
    }

    /// The integrator is reassignable while the task has not started, the way
    /// the reviewers are, so `--integrator` alone is an update worth sending.
    #[test]
    fn the_integrator_can_be_reassigned_on_its_own() {
        let req = update_request(
            None,
            None,
            vec![],
            Some("GitHub Integrator".into()),
            vec![],
            false,
        )
        .expect("body");
        assert_eq!(req.integrator_profile.as_deref(), Some("GitHub Integrator"));
        assert!(req.title.is_none());
        assert!(req.reviewer_profiles.is_none());
    }

    /// The one thing the repeatable flag cannot say on its own.
    #[test]
    fn clearing_the_dependencies_sends_an_empty_list() {
        let req = update_request(None, None, vec![], None, vec![], true).expect("body");
        assert_eq!(req.depends_on.as_deref(), Some([].as_slice()));
    }

    #[test]
    fn an_update_with_no_flags_is_refused_before_it_is_sent() {
        let err = update_request(None, None, vec![], None, vec![], false).expect_err("no-op");
        assert!(err.to_string().starts_with("nothing to update"), "{err}");
    }

    /// A plain task, for the blocks that render one.
    fn dto() -> TaskDto {
        TaskDto {
            id: "01TASK".into(),
            goal_id: "01GOAL".into(),
            repo_id: "01REPO".into(),
            title: "Add the frobnicator".into(),
            description: String::new(),
            status: TaskStatus::InProgress,
            engineer_profile_id: "01ENG".into(),
            integrator_profile_id: "01INT".into(),
            agent_kind: None,
            model: None,
            reviewers: vec![],
            depends_on: vec![],
            branch: "ariadne/task-01TASK".into(),
            worktree_path: None,
            review_round: 0,
            stalled: false,
            merge_commit: None,
            pr_number: None,
            pr_url: None,
            created_at: "2026-08-17T08:00:00Z".into(),
            updated_at: "2026-08-17T08:00:00Z".into(),
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

    /// Who lands a task is an assignment like its engineer, so the list says
    /// so rather than making the reader inspect every row to find out.
    #[test]
    fn the_list_names_the_integrator_and_the_pull_request() {
        let profiles =
            ProfileNames::from_pairs([("01INT".to_string(), "GitHub Integrator".to_string())]);
        let t = TaskDto {
            status: TaskStatus::Integrating,
            pr_number: Some(12),
            pr_url: Some("https://github.com/owner/repo/pull/12".into()),
            ..dto()
        };

        let row = ls_row(&profiles, &t);
        assert_eq!(row.len(), LS.len(), "a row per column, in LS's order");
        assert_eq!(
            row,
            [
                "01TASK",
                "Add the frobnicator",
                "integrating",
                "0",
                "-",
                "GitHub Integrator (01INT)",
                "#12",
                "ariadne/task-01TASK",
            ]
        );
    }

    /// Every task names an integrator, and the cell names it the way the
    /// engineer's does — one spelling for the list and the inspect block.
    #[test]
    fn the_integrator_is_named_the_way_the_engineer_is() {
        let profiles =
            ProfileNames::from_pairs([("01INT".to_string(), "Local Integrator".to_string())]);
        let row = ls_row(&profiles, &dto());
        assert_eq!(row[5], "Local Integrator (01INT)");
        assert_eq!(row[6], "-", "and no pull request was ever opened for it");
        assert_eq!(integrator_label(&profiles, &dto()), row[5]);
    }

    /// A profile the daemon would not name still leaves the id, the way every
    /// other mention of one does.
    #[test]
    fn an_unresolvable_integrator_is_left_as_its_id() {
        let t = TaskDto {
            integrator_profile_id: "01GONE".into(),
            ..dto()
        };
        assert_eq!(
            integrator_label(&ProfileNames::from_pairs([]), &t),
            "01GONE"
        );
    }
}
