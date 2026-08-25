//! The command tree: every command, flag and argument `ariadne` accepts.

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};

use crate::commands::agent::AgentCommand;
use crate::commands::goal::GoalCommand;
use crate::commands::profile::ProfileCommand;
use crate::commands::repo::RepoCommand;
use crate::commands::session::SessionCommand;
use crate::commands::task::TaskCommand;
use crate::output::Format;

#[derive(Parser)]
#[command(name = "ariadne", version, about = "Coding-agent orchestrator CLI")]
pub struct Cli {
    /// Daemon endpoint: unix socket path or http://host:port
    /// (default: $ARIADNE_SOCKET, else the socket of the ariadne home —
    /// $ARIADNE_HOME or ~/.ariadne — as its config.toml names it)
    // `--host` was the old name and never meant a host; it stays as an
    // undocumented alias so existing scripts keep working.
    #[arg(long, alias = "host", global = true, env = ariadne_client::ENDPOINT_ENV)]
    pub endpoint: Option<String>,

    /// Output format
    #[arg(long, global = true, value_enum, default_value = "table")]
    pub format: Format,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Show client and daemon version
    Version,
    /// Check the installation: agents, tools, config, daemon and service
    ///
    /// Reports what this shell sees and what the daemon sees — they differ
    /// more often than one would like — and exits 1 when anything failed.
    Doctor,
    /// Generate shell completions (bash, zsh, fish, ...) to stdout
    Completions { shell: clap_complete::Shell },
    /// Manage the ariadned daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Manage the agent CLIs: the flags each one is launched with
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Manage agent profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Manage repositories
    Repo {
        #[command(subcommand)]
        command: RepoCommand,
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
    /// Show everything that needs a human, grouped by goal
    ///
    /// The UI's Attention page from the terminal: tasks that failed or
    /// stalled, plus agent sessions waiting on a permission prompt or an
    /// answer, in error, disconnected or stalled.
    Attention {
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
    },
    /// Attach to the tmux session of a session, task or goal id
    Attach {
        /// Session, task or goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::attach_ids))]
        id: String,
        /// Which agent of that id to attach to (default: engineer for tasks,
        /// planner for goals; not valid with a session id)
        #[arg(long, value_enum)]
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
    /// Become the agent a daemon-written spawn plan describes (tmux runs this)
    ///
    /// The plan carries the argv, the environment and the working directory,
    /// none of which would fit in a tmux command line — see
    /// `ariadne_core::spawn_plan`.
    #[command(hide = true, name = "_spawn")]
    Spawn {
        /// Path to the JSON spawn plan in the session's run dir
        plan: PathBuf,
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
pub enum SetupCommand {
    /// Trust Ariadne's Codex hooks (starts codex once so it can ask)
    CodexHooks {
        /// The `ariadne` binary the hooks call (default: this one). Must match
        /// the daemon's `cli_bin`.
        #[arg(long)]
        cli_bin: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum McpCommand {
    /// Run the stdio MCP server
    Serve,
}

#[derive(Subcommand)]
pub enum DaemonCommand {
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

/// Subcommand paths where `--format` has nothing to format: they hand the
/// terminal to another program, print a shell script, or answer a machine on
/// stdout in a protocol of their own. The flag is global (so `ariadne
/// --format json task ls` keeps working), which would otherwise advertise it
/// on every one of them — the hidden internal commands included.
///
/// Hiding it on a command hides it on everything under that command.
const NO_FORMAT: &[&[&str]] = &[
    &["completions"],
    &["attach"],
    &["setup"],
    &["mcp"],
    &["agent-event"],
    &["_spawn"],
    &["daemon", "logs"],
    &["goal", "attach"],
    &["task", "attach"],
];

/// The clap command, with `--format` hidden wherever it does nothing.
pub fn command() -> clap::Command {
    NO_FORMAT
        .iter()
        .fold(Cli::command(), |cmd, path| hide_format(cmd, path))
}

/// Hide `--format` on the subcommand at `path` (`[]` = this command itself).
///
/// A global argument is not there to mutate: clap only copies it into the
/// subcommands as it builds them, and skips the copy where the subcommand
/// declares that name itself. So this declares it — same flag, same values,
/// out of the help.
fn hide_format(cmd: clap::Command, path: &[&str]) -> clap::Command {
    match path {
        [] => cmd.arg(
            clap::Arg::new("format")
                .long("format")
                .hide(true)
                .value_parser(clap::value_parser!(Format))
                .default_value("table"),
        ),
        [name, rest @ ..] => cmd.mut_subcommand(name, |sub| hide_format(sub, rest)),
    }
}

#[cfg(test)]
mod tests;
