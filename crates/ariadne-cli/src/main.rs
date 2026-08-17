//! ariadne — CLI for the Ariadne daemon.

mod commands;
mod complete;
mod error;
mod output;
mod query;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

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
    /// (default: $ARIADNE_SOCKET, else the socket of the ariadne home —
    /// $ARIADNE_HOME or ~/.ariadne — as its config.toml names it)
    // `--host` was the old name and never meant a host; it stays as an
    // undocumented alias so existing scripts keep working.
    #[arg(long, alias = "host", global = true, env = ariadne_client::ENDPOINT_ENV)]
    endpoint: Option<String>,

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

/// Subcommand paths where `--format` has nothing to format: they hand the
/// terminal to another program, or print a shell script. The flag is global
/// (so `ariadne --format json task ls` keeps working), which would otherwise
/// advertise it on every one of them.
const NO_FORMAT: &[&[&str]] = &[
    &["completions"],
    &["attach"],
    &["setup"],
    &["daemon", "logs"],
    &["goal", "attach"],
    &["task", "attach"],
];

/// The clap command, with `--format` hidden wherever it does nothing.
fn command() -> clap::Command {
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

/// Failures are reported by [`error::report`] rather than by anyhow's default
/// `Error: ...` + `Caused by:` block: one line, and exit code 1. Usage errors
/// never reach here — clap prints and exits 2 itself.
fn main() -> ExitCode {
    // Dynamic shell completion: when invoked by the completion shim
    // (COMPLETE=<shell> in the environment) this answers the request and
    // exits before anything else runs. Candidate functions query the daemon
    // with their own tiny runtime, so this must happen before tokio starts.
    clap_complete::CompleteEnv::with_factory(command).complete();

    let cli = match Cli::from_arg_matches(&command().get_matches()) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };
    let format = cli.format;
    match block_on(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error::report(&e, format);
            ExitCode::FAILURE
        }
    }
}

fn block_on(cli: Cli) -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(cli))
}

async fn run(cli: Cli) -> Result<()> {
    // Commands talk to an already-running daemon, so an explicit endpoint wins
    // here. `daemon start` is the exception and resolves its own target from
    // the home it spawns the daemon in.
    let client = Client::resolve(cli.endpoint.as_deref(), None);

    match cli.command {
        Command::Version => commands::version(&client, cli.format).await,
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut command(), "ariadne", &mut std::io::stdout());
            Ok(())
        }
        Command::Daemon { command } => match command {
            DaemonCommand::Start { home } => commands::daemon_start(home, cli.format).await,
            DaemonCommand::Stop => commands::daemon_stop(cli.format),
            DaemonCommand::Status => commands::daemon_status(&client, cli.format).await,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// clap's own consistency check over the whole tree, shadowed `--format`
    /// arguments included.
    #[test]
    fn the_command_tree_is_well_formed() {
        command().debug_assert();
    }

    #[test]
    fn format_is_advertised_exactly_where_it_is_honored() {
        for path in NO_FORMAT {
            assert!(!advertises_format(path), "--format advertised on {path:?}");
        }
        for path in [
            &["version"][..],
            &["daemon", "status"],
            &["task", "ls"],
            &["task", "diff"],
            &["session", "inspect"],
            &["profile", "rm"],
        ] {
            assert!(advertises_format(path), "--format missing from {path:?}");
        }
    }

    /// `--host` was the documented spelling before `--endpoint`; scripts that
    /// still use it must land in the same field.
    #[test]
    fn the_old_host_flag_still_names_the_endpoint() {
        for flag in ["--endpoint", "--host"] {
            let cli = parse(&["ariadne", flag, "/tmp/x.sock", "version"]);
            assert_eq!(cli.endpoint.as_deref(), Some("/tmp/x.sock"), "{flag}");
        }
    }

    /// The flag keeps working where it is no longer advertised — hiding it is
    /// about help text, not about breaking a command line that has it.
    #[test]
    fn a_hidden_format_flag_is_still_parsed() {
        assert_eq!(
            parse(&["ariadne", "attach", "--format", "json", "x"]).format,
            Format::Json
        );
        assert_eq!(
            parse(&["ariadne", "--format", "json", "attach", "x"]).format,
            Format::Json
        );
        assert_eq!(parse(&["ariadne", "attach", "x"]).format, Format::Table);
    }

    fn parse(argv: &[&str]) -> Cli {
        Cli::from_arg_matches(&command().get_matches_from(argv)).expect("parse")
    }

    /// Whether `--format` shows up in that subcommand's help.
    fn advertises_format(path: &[&str]) -> bool {
        let mut cmd = command();
        // Globals only reach the subcommands once the tree is built.
        cmd.build();
        let mut sub = &cmd;
        for name in path {
            sub = sub.find_subcommand(name).expect("subcommand");
        }
        sub.get_arguments()
            .any(|a| a.get_id() == "format" && !a.is_hide_set())
    }
}
