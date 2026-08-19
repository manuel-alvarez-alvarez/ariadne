//! `ariadne session ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::sessions::{SessionDto, SessionListQuery};
use ariadne_client::Client;

use super::{ProfileNames, confirm};
use crate::output::{
    Column, Format, UNCAPPED, local_time, note, print_json, print_kv, print_table,
};
use crate::query::query_path;

/// Columns of `session ls`.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("role", UNCAPPED),
    ("agent", UNCAPPED),
    ("status", UNCAPPED),
    ("tmux", 32),
    ("internal id", 36),
];

#[derive(Subcommand)]
pub enum SessionCommand {
    /// List live agent sessions (docker-style; --all includes history)
    Ls {
        /// Filter by task id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::task_ids))]
        task: Option<String>,
        /// Filter by goal id
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::goal_ids))]
        goal: Option<String>,
        /// Include finished sessions (exited/failed), not just live ones
        #[arg(short, long)]
        all: bool,
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
    },
    /// Show a session
    Inspect {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
    },
    /// Show recent terminal output of a session
    Logs {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
    },
    /// Revive an ended session: new tmux, same agent conversation
    Resume {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
    },
    /// Kill a session's tmux process
    Kill {
        /// Session id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::session_ids))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

pub async fn run(client: &Client, cmd: SessionCommand, format: Format) -> Result<()> {
    match cmd {
        SessionCommand::Ls {
            task,
            goal,
            all,
            no_trunc,
        } => {
            let filtered = goal.is_some() || task.is_some();
            let query = SessionListQuery {
                goal,
                task,
                status: None,
                attention: None,
            };
            let mut sessions: Vec<SessionDto> = client
                .get_json(&query_path("/v1/sessions", &query)?)
                .await?;
            if !all {
                sessions.retain(|s| s.status.is_live());
            }
            match format {
                Format::Json => print_json(&sessions)?,
                Format::Table => {
                    print_table(
                        LS,
                        &sessions
                            .iter()
                            .map(|s| {
                                vec![
                                    s.id.clone(),
                                    s.role.as_str().into(),
                                    s.agent_kind.as_str().into(),
                                    s.status.as_str().into(),
                                    s.tmux_session.clone(),
                                    s.internal_session_id.clone().unwrap_or_else(|| "-".into()),
                                ]
                            })
                            .collect::<Vec<_>>(),
                        no_trunc,
                    );
                    if sessions.is_empty() {
                        note(match (filtered, all) {
                            (true, true) => "no sessions match that filter",
                            (true, false) => {
                                "no live sessions match that filter — finished ones are behind --all"
                            }
                            (false, true) => "no sessions yet",
                            (false, false) => "no live sessions — finished ones are behind --all",
                        });
                    }
                }
            }
        }
        SessionCommand::Inspect { id } => {
            let s: SessionDto = client.get_json(&format!("/v1/sessions/{id}")).await?;
            match format {
                Format::Json => print_json(&s)?,
                Format::Table => {
                    let profiles = ProfileNames::fetch(client).await;
                    print_kv(&[
                        ("id", s.id),
                        ("goal", s.goal_id),
                        ("task", s.task_id.unwrap_or_else(|| "-".into())),
                        ("role", s.role.as_str().into()),
                        ("profile", profiles.label(&s.profile_id)),
                        ("agent", s.agent_kind.as_str().into()),
                        ("status", s.status.as_str().into()),
                        ("tmux", s.tmux_session),
                        ("worktree", s.worktree_path.unwrap_or_else(|| "-".into())),
                        (
                            "round",
                            s.review_round.map_or("-".into(), |r| r.to_string()),
                        ),
                        (
                            "internal id",
                            s.internal_session_id.unwrap_or_else(|| "-".into()),
                        ),
                        (
                            "activity",
                            s.last_activity_at.as_deref().map_or("-".into(), local_time),
                        ),
                        ("created", local_time(&s.created_at)),
                        (
                            "ended",
                            s.ended_at.as_deref().map_or("-".into(), local_time),
                        ),
                    ]);
                }
            }
        }
        SessionCommand::Logs { id } => {
            let logs: ariadne_api::sessions::SessionLogsResponse =
                client.get_json(&format!("/v1/sessions/{id}/logs")).await?;
            match format {
                Format::Json => print_json(&logs)?,
                Format::Table => print!("{}", logs.logs),
            }
        }
        SessionCommand::Resume { id } => {
            // The daemon answers with this same session either way: relaunched
            // when it really resumed it, or untouched when its pane turned out
            // to be alive already. What the row said before the call is what
            // tells a relaunch from a session that never needed one.
            let before: SessionDto = client.get_json(&format!("/v1/sessions/{id}")).await?;
            let s: SessionDto = client
                .post_empty(&format!("/v1/sessions/{id}/resume"))
                .await?;
            let resumed = !before.status.is_live() && s.status.is_live();
            match format {
                Format::Json => print_json(&serde_json::json!({
                    "resumed": resumed,
                    "session": s,
                }))?,
                Format::Table => {
                    if resumed {
                        println!("session {} resumed ({})", s.id, s.tmux_session);
                    } else {
                        println!(
                            "session {} already has a running agent ({}); nothing to resume",
                            s.id, s.tmux_session
                        );
                    }
                }
            }
        }
        SessionCommand::Kill { id, yes } => {
            let s: SessionDto = client.get_json(&format!("/v1/sessions/{id}")).await?;
            confirm(&kill_question(&s), yes)?;
            let s: SessionDto = client
                .post_empty(&format!("/v1/sessions/{id}/kill"))
                .await?;
            match format {
                Format::Json => print_json(&s)?,
                Format::Table => println!("session {} is now {}", s.id, s.status.as_str()),
            }
        }
    }
    Ok(())
}

/// What `session kill` asks: a live agent is about to lose its terminal, and
/// the id alone does not say whose.
fn kill_question(s: &SessionDto) -> String {
    let what = match &s.task_id {
        Some(task) => format!("{} on task {task}", s.role.as_str()),
        None => format!("{} of goal {}", s.role.as_str(), s.goal_id),
    };
    format!("Kill session {} ({what}, {})?", s.id, s.status.as_str())
}
