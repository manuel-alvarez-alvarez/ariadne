//! ariadne — CLI for the Ariadne daemon.

mod cli;
mod codex_trust;
mod commands;
mod complete;
mod error;
mod output;

use std::process::ExitCode;

use anyhow::Result;
use clap::FromArgMatches;

use ariadne_client::Client;

use crate::cli::{Cli, Command, DaemonCommand, Layout, McpCommand, SetupCommand, command};
use crate::output::View;

/// Failures are reported by [`error::report`] rather than by anyhow's default
/// `Error: ...` + `Caused by:` block: one line, and exit code 1. Usage errors
/// never reach here — clap prints and exits 2 itself.
fn main() -> ExitCode {
    // Rust starts every process with SIGPIPE ignored, which turns a reader
    // that walked away (`ariadne task ls | head`) into a write error and a
    // panic inside a printing macro. The default disposition — end quietly on
    // a closed pipe — is what a command-line tool wants.
    sigpipe::reset();

    // Dynamic shell completion: when invoked by the completion shim
    // (COMPLETE=<shell> in the environment) this answers the request and
    // exits before anything else runs. Candidate functions query the daemon
    // on a small runtime of their own, so this must happen before tokio
    // starts.
    clap_complete::CompleteEnv::with_factory(command).complete();

    let cli = match Cli::from_arg_matches(&command().get_matches()) {
        Ok(cli) => cli,
        Err(e) => e.exit(),
    };
    let format = cli.format;
    // How everything after this renders: colour, width and the listing flags,
    // settled once from the command line and the environment.
    output::init(View {
        color: cli.color.enabled(),
        no_trunc: cli.no_trunc,
        quiet: cli.quiet,
        wide: cli.layout == Layout::Wide,
        columns: cli.columns.clone(),
        width: output::terminal_width(),
        pager: !cli.no_pager,
    });

    // `_spawn` does not talk to the daemon: it *becomes* the agent. It is
    // handled before the runtime starts because what tmux is watching is this
    // process, and it has to reach its `exec` as itself — no worker threads
    // and no tokio between the pane and the agent.
    if let Command::Spawn { plan } = &cli.command {
        let Err(e) = commands::spawn::exec_plan(plan);
        error::report(&e, format);
        return ExitCode::FAILURE;
    }

    match block_on(cli) {
        Ok(code) => code,
        Err(e) => {
            error::report(&e, format);
            ExitCode::FAILURE
        }
    }
}

fn block_on(cli: Cli) -> Result<ExitCode> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(cli))
}

async fn run(cli: Cli) -> Result<ExitCode> {
    // Commands talk to an already-running daemon, so an explicit endpoint wins
    // here. `daemon start` is the exception and resolves its own target from
    // the home it spawns the daemon in.
    let client = Client::resolve(cli.endpoint.as_deref(), None);
    let format = cli.format;

    // `doctor` is the one command whose verdict is an exit code rather than a
    // failure: a report that found something is a report, not an error.
    if let Command::Doctor = cli.command {
        return commands::doctor::run(&client, format).await;
    }

    let outcome = match cli.command {
        Command::Version => commands::version(&client, format).await,
        Command::Completions {
            shell,
            static_script,
            command,
        } => commands::completions::run(shell, static_script, command),
        Command::Daemon { home, command } => match command {
            DaemonCommand::Start => commands::daemon::start(home, format).await,
            DaemonCommand::Stop { timeout } => commands::daemon::stop(home, timeout, format).await,
            DaemonCommand::Restart { timeout } => {
                commands::daemon::restart(home, timeout, format).await
            }
            // The endpoint still wins over the home here: `status` reads a
            // daemon rather than driving one, and `--endpoint` is how a daemon
            // that is not this host's home is read.
            DaemonCommand::Status => {
                let client = Client::resolve(cli.endpoint.as_deref(), home.clone());
                commands::daemon::status(&client, home, format).await
            }
            DaemonCommand::Logs { follow } => commands::daemon::logs(home, follow),
        },
        Command::Attach { id, role } => commands::attach::attach_any(&client, &id, role).await,
        Command::Agent { command } => commands::agent::run(&client, command, format).await,
        Command::Profile { command } => commands::profile::run(&client, command, format).await,
        Command::Repo { command } => commands::repo::run(&client, command, format).await,
        Command::Goal { command } => commands::goal::run(&client, command, format).await,
        Command::Task { command } => commands::task::run(&client, command, format).await,
        Command::Session { command } => commands::session::run(&client, command, format).await,
        Command::Models { command } => commands::models::run(&client, command, format).await,
        Command::Attention => commands::attention::run(&client, format).await,
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
        // Handled above: it is the only command with an exit code of its own.
        Command::Doctor => unreachable!(),
        // Handled in `main`, before the runtime it must not run inside.
        Command::Spawn { .. } => unreachable!(),
    };
    outcome.map(|()| ExitCode::SUCCESS)
}
