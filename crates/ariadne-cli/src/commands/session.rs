//! `ariadne session ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::sessions::{SessionDto, SessionListQuery};
use ariadne_client::Client;

use crate::output::{Format, print_json, print_table};
use crate::query::query_path;

#[derive(Subcommand)]
pub enum SessionCommand {
    /// List agent sessions
    Ls {
        /// Filter by task id
        #[arg(long)]
        task: Option<String>,
        /// Filter by goal id
        #[arg(long)]
        goal: Option<String>,
    },
    /// Show a session
    Inspect { id: String },
    /// Show recent terminal output of a session
    Logs { id: String },
    /// Kill a session's tmux process
    Kill { id: String },
}

pub async fn run(client: &Client, cmd: SessionCommand, format: Format) -> Result<()> {
    match cmd {
        SessionCommand::Ls { task, goal } => {
            let query = SessionListQuery {
                goal,
                task,
                status: None,
            };
            let sessions: Vec<SessionDto> = client
                .get_json(&query_path("/v1/sessions", &query)?)
                .await?;
            match format {
                Format::Json => print_json(&sessions)?,
                Format::Table => print_table(
                    &["id", "role", "agent", "status", "tmux", "internal id"],
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
                ),
            }
        }
        SessionCommand::Inspect { id } => {
            let s: SessionDto = client.get_json(&format!("/v1/sessions/{id}")).await?;
            print_json(&s)?;
        }
        SessionCommand::Logs { id } => {
            let logs: ariadne_api::sessions::SessionLogsResponse =
                client.get_json(&format!("/v1/sessions/{id}/logs")).await?;
            print!("{}", logs.logs);
        }
        SessionCommand::Kill { id } => {
            let s: SessionDto = client
                .post_empty(&format!("/v1/sessions/{id}/kill"))
                .await?;
            println!("session {} is now {}", s.id, s.status.as_str());
        }
    }
    Ok(())
}
