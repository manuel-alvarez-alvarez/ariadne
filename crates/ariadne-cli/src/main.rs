//! ariadne — CLI for the Ariadne daemon.

mod commands;
mod complete;
mod output;
mod query;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use ariadne_client::Client;

use crate::commands::goal::GoalCommand;
use crate::commands::profile::ProfileCommand;
use crate::commands::session::SessionCommand;
use crate::commands::task::TaskCommand;
use crate::output::Format;

#[derive(Parser)]
#[command(name = "ariadne", version, about = "Coding-agent orchestrator CLI")]
struct Cli {
    /// Daemon endpoint: unix socket path or http://host:port
    /// (default: $ARIADNE_SOCKET or ~/.ariadne/ariadne.sock)
    #[arg(long, global = true, env = ariadne_client::ENDPOINT_ENV)]
    host: Option<String>,

    /// Output format
    #[arg(long, global = true, value_enum, default_value = "table")]
    format: Format,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show client and daemon version
    Version,
    /// Generate shell completions (bash, zsh, fish, ...) to stdout
    Completions { shell: clap_complete::Shell },
    /// Manage the ariadned daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Manage agent profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Manage goals
    Goal {
        #[command(subcommand)]
        command: GoalCommand,
    },
    /// Manage tasks
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Inspect agent sessions
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Attach to the tmux session of a session, task or goal id
    Attach {
        /// Session, task or goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(complete::attach_ids))]
        id: String,
        /// engineer | reviewer | planner (default: engineer for tasks, planner
        /// for goals; not valid with a session id)
        #[arg(long, value_parser = commands::parse_role, add = clap_complete::engine::ArgValueCandidates::new(complete::roles))]
        role: Option<ariadne_core::Role>,
    },
    /// One-time host setup for the coding agents
    Setup {
        #[command(subcommand)]
        command: SetupCommand,
    },
    /// Serve Ariadne MCP tools over stdio (spawned by coding agents)
    #[command(hide = true)]
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    /// Report an agent hook event to the daemon (called by hooks, fail-safe)
    #[command(hide = true, name = "agent-event")]
    AgentEvent {
        /// claude | codex | opencode
        #[arg(long, default_value = "claude")]
        kind: String,
        /// OpenCode plugin payload
        #[arg(long)]
        json: Option<String>,
    },
}

#[derive(Subcommand)]
enum SetupCommand {
    /// Trust Ariadne's Codex hooks (starts codex once so it can ask)
    CodexHooks {
        /// The `ariadne` binary the hooks call (default: this one). Must match
        /// the daemon's `cli_bin`.
        #[arg(long)]
        cli_bin: Option<String>,
    },
}

#[derive(Subcommand)]
enum McpCommand {
    /// Run the stdio MCP server
    Serve,
}

#[derive(Subcommand)]
enum DaemonCommand {
    /// Start ariadned in the background
    Start {
        /// Ariadne home directory (default: $ARIADNE_HOME or ~/.ariadne)
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Stop a running ariadned
    Stop,
    /// Show daemon status
    Status,
    /// Show (or follow) the daemon log
    Logs {
        /// Follow the log (tail -f)
        #[arg(short, long)]
        follow: bool,
    },
}

fn main() -> Result<()> {
    // Dynamic shell completion: when invoked by the completion shim
    // (COMPLETE=<shell> in the environment) this answers the request and
    // exits before anything else runs. Candidate functions query the daemon
    // with their own tiny runtime, so this must happen before tokio starts.
    clap_complete::CompleteEnv::with_factory(|| {
        use clap::CommandFactory;
        Cli::command()
    })
    .complete();

    let cli = Cli::parse();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(cli))
}

async fn run(cli: Cli) -> Result<()> {
    let client = match &cli.host {
        Some(h) if h.starts_with("http://") || h.starts_with("https://") => Client::tcp(h.clone()),
        Some(h) => Client::unix(h),
        None => Client::from_env(),
    };

    match cli.command {
        Command::Version => commands::version(&client).await,
        Command::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "ariadne",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Command::Daemon { command } => match command {
            DaemonCommand::Start { home } => commands::daemon_start(&client, home).await,
            DaemonCommand::Stop => commands::daemon_stop(),
            DaemonCommand::Status => commands::daemon_status(&client).await,
            DaemonCommand::Logs { follow } => commands::daemon_logs(follow),
        },
        Command::Attach { id, role } => commands::attach::attach_any(&client, &id, role).await,
        Command::Profile { command } => commands::profile::run(&client, command, cli.format).await,
        Command::Goal { command } => commands::goal::run(&client, command, cli.format).await,
        Command::Task { command } => commands::task::run(&client, command, cli.format).await,
        Command::Session { command } => commands::session::run(&client, command, cli.format).await,
        Command::Setup {
            command: SetupCommand::CodexHooks { cli_bin },
        } => commands::setup::codex_hooks(cli_bin),
        Command::AgentEvent { kind, json } => {
            commands::agent_event::run(kind, json).await;
            Ok(()) // always succeeds: hooks must never fail
        }
        Command::Mcp {
            command: McpCommand::Serve,
        } => commands::mcp::serve().await,
    }
}
