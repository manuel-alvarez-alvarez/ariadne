//! `ariadne agent ...`

use anyhow::{Context, Result};
use clap::Subcommand;

use ariadne_api::agents::{AcpAgentDto, AcpAgentStatus, AgentConfigDto};
use ariadne_client::Client;

use crate::output::{
    Column, Format, UNCAPPED, col, empty_state, ok_id_line, print, print_list, view,
};

/// Columns of `agent ls`. The row is the registry agent, and the two flag
/// lists are what there is to read.
const LS: &[Column] = &[
    col("agent", UNCAPPED).title(),
    col("flags", 44),
    col("defaults", 44).rank(1),
];

/// Columns of `agent refresh`: the registry's current discovery result.
const REFRESH: &[Column] = &[
    col("agent", UNCAPPED).id(),
    col("status", UNCAPPED).check(),
    col("command", 44).title().rank(1),
    col("reason", 44).rank(0),
];

/// What an empty flag list looks like in a table: a cell nobody can mistake
/// for a flag.
const EMPTY: &str = "-";

/// `ariadne agent ...` — how each registry agent is launched.
///
/// The flags belong to the agent, not to the persona: every session that runs
/// on it is spawned and resumed with them, behind its registry command, and
/// an edit lands on the next launch.
#[derive(Subcommand)]
pub(crate) enum AgentCommand {
    /// List the registry agents, their flags and the defaults they came from
    Ls,
    /// Reprobe every agent and list its current status
    Refresh,
    /// Replace an agent's flags
    ///
    /// The list is replaced whole: `--flag` names every flag the agent is to
    /// be launched with, `--clear-flags` launches it with none, and `--reset`
    /// puts back what Ariadne ships for that agent. Exactly one of the three.
    Update {
        /// The agent's registry id, as `ariadne agent ls` lists it
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_ids))]
        agent: String,
        /// One flag to launch this agent with, repeatable
        ///
        /// Flags start with a dash, so a value clap could read as a flag of
        /// its own is taken as it is typed: `--flag --verbose`.
        #[arg(
            long = "flag",
            value_name = "FLAG",
            allow_hyphen_values = true,
            required_unless_present_any = ["reset", "clear_flags"],
            conflicts_with_all = ["reset", "clear_flags"],
        )]
        flags: Vec<String>,
        /// Launch this agent with no extra flags at all
        #[arg(long, conflicts_with = "reset")]
        clear_flags: bool,
        /// Put the flag list back to what Ariadne ships for this agent
        #[arg(long)]
        reset: bool,
    },
}

/// A flag list as a table shows it: the flags as they are launched, or
/// [`EMPTY`] when there are none.
fn flags_cell(flags: &[String]) -> String {
    match flags.is_empty() {
        true => EMPTY.to_string(),
        false => flags.join(" "),
    }
}

/// The flags Ariadne ships for `agent`, as the daemon reports them: what
/// `--reset` writes back, so a default that changes there changes here too.
async fn default_flags(client: &Client, agent: &str) -> Result<Vec<String>> {
    client
        .list_agent_configs()
        .await?
        .into_iter()
        .find(|c| c.agent_id == agent)
        .map(|c| c.default_flags)
        .with_context(|| format!("the daemon knows no agent {agent}"))
}

pub(crate) async fn run(client: &Client, cmd: AgentCommand, format: Format) -> Result<()> {
    match cmd {
        AgentCommand::Ls => {
            let configs: Vec<AgentConfigDto> = client.list_agent_configs().await?;
            print_list(
                format,
                &configs,
                LS,
                |c| {
                    vec![
                        c.agent_id.clone(),
                        flags_cell(&c.extra_flags),
                        flags_cell(&c.default_flags),
                    ]
                },
                empty_state("No agents are in the registry.", Some("ariadne doctor")),
            )?;
        }
        AgentCommand::Refresh => refresh(client, format).await?,
        AgentCommand::Update {
            agent,
            flags,
            clear_flags: _,
            reset,
        } => {
            let extra_flags = match reset {
                true => default_flags(client, &agent).await?,
                // `--clear-flags` is the empty list, which is exactly what
                // `flags` already is: clap has seen to it that one of the two
                // was given, and that neither came with the other.
                false => flags,
            };
            let config = client.update_agent_config(&agent, extra_flags).await?;
            print(format, &config, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "updated", &config.agent_id)
                );
            })?;
        }
    }
    Ok(())
}

/// Reprobe the ACP registry, then show exactly the snapshot the daemon
/// answered with. A rejected entry keeps its reason in the table rather than
/// disappearing: installing or correcting its command is what refresh is for.
async fn refresh(client: &Client, format: Format) -> Result<()> {
    let agents = client.refresh_acp_agents().await?;
    print_list(
        format,
        &agents,
        REFRESH,
        refresh_row,
        empty_state("No ACP agents are in the registry.", Some("ariadne doctor")),
    )
}

fn refresh_row(agent: &AcpAgentDto) -> Vec<String> {
    refresh_row_values(
        &agent.id,
        agent.status,
        &agent.command,
        agent.rejection_reason.as_deref(),
    )
}

fn refresh_row_values(
    id: &str,
    status: AcpAgentStatus,
    command: &[String],
    rejection_reason: Option<&str>,
) -> Vec<String> {
    vec![
        id.into(),
        match status {
            AcpAgentStatus::Ready => "ok",
            AcpAgentStatus::Rejected => "warn",
        }
        .into(),
        command.join(" "),
        rejection_reason.unwrap_or(EMPTY).into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::{Json, Router};

    use ariadne_api::error::ErrorBody;
    use ariadne_client::ClientError;

    #[test]
    fn a_flag_list_reads_as_a_command_line_and_an_empty_one_as_a_dash() {
        assert_eq!(flags_cell(&[]), EMPTY);
        assert_eq!(
            flags_cell(&["--auto".into(), "--verbose".into()]),
            "--auto --verbose"
        );
    }

    #[test]
    fn the_agent_keeps_the_agent_column_name() {
        let table = crate::output::render_table(
            LS,
            &[vec![String::new(); LS.len()]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.contains("AGENT"), "{table}");
        assert!(!header.contains("TITLE"), "{table}");
    }

    #[test]
    fn a_rejected_agent_keeps_its_reason_in_the_refresh_row() {
        let reason = "the program is not on PATH";
        let row = refresh_row_values(
            "rejected",
            AcpAgentStatus::Rejected,
            &["missing-agent".into(), "acp".into()],
            Some(reason),
        );
        assert_eq!(row, ["rejected", "warn", "missing-agent acp", reason]);
        assert_eq!(REFRESH[1].cell, crate::output::table::Cell::Check);
    }

    #[tokio::test]
    async fn refresh_calls_the_reprobe_endpoint_once() {
        async fn handler(State(calls): State<Arc<AtomicUsize>>) -> Json<Vec<serde_json::Value>> {
            calls.fetch_add(1, Ordering::SeqCst);
            Json(vec![])
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/v1/acp-agents/refresh", post(handler))
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        refresh(&Client::tcp(format!("http://{address}")), Format::Json)
            .await
            .unwrap();
        server.abort();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn refresh_keeps_the_daemon_error_message() {
        async fn handler() -> (StatusCode, Json<ErrorBody>) {
            (
                StatusCode::CONFLICT,
                Json(ErrorBody::new("refresh_refused", "the registry is busy")),
            )
        }

        let app = Router::new().route("/v1/acp-agents/refresh", post(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let error = refresh(&Client::tcp(format!("http://{address}")), Format::Table)
            .await
            .unwrap_err();
        server.abort();

        assert_eq!(
            error.downcast_ref::<ClientError>().unwrap().human(),
            "the registry is busy"
        );
    }
}
