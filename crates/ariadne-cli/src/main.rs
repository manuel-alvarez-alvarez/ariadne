//! ariadne — CLI for the Ariadne daemon.

mod cli;
mod commands;
mod complete;
mod error;
mod output;
mod telemetry;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use clap::{ArgMatches, FromArgMatches};

use ariadne_client::Client;

use crate::cli::{Cli, Command, DaemonCommand, Layout, McpCommand, command};
use crate::output::View;
use crate::telemetry::Telemetry;

/// Failures are reported by [`error::report`] rather than by anyhow's default
/// `Error: ...` + `Caused by:` block: one line, and the exit code that says
/// what kind of failure it was ([`error::Exit`], listed in `ariadne --help`).
/// A line clap itself refuses never reaches here — it prints and exits 2, the
/// same code this gives a command refused for how it was typed.
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

    let matches = command().get_matches();
    let command = command_path(&matches);
    let cli = match Cli::from_arg_matches(&matches) {
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

    match block_on(cli, command) {
        Ok(code) => code,
        Err(e) => {
            error::report(&e, format);
            error::exit_code(&e)
        }
    }
}

fn block_on(cli: Cli, command: String) -> Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let client = Arc::new(Client::resolve(cli.endpoint.as_deref(), None));
        let telemetry = match &cli.command {
            Command::Daemon {
                command: DaemonCommand::Start,
                ..
            } => None,
            _ => Telemetry::enabled(Arc::clone(&client)),
        };
        let started = Instant::now();
        let outcome = run(cli, client).await;
        if let Some(telemetry) = telemetry {
            let result = match &outcome {
                Ok(code) if code == &ExitCode::SUCCESS => "ok",
                Ok(_) => "error",
                Err(error) => {
                    let exit = error::exit(error);
                    tracing::error!(exit_code = exit as u8, outcome = ?exit, "command failed");
                    if exit == error::Exit::Usage {
                        "usage"
                    } else {
                        "error"
                    }
                }
            };
            telemetry.command(&command, result, started.elapsed());
            telemetry.flush().await;
        }
        outcome
    })
}

async fn run(cli: Cli, client: Arc<Client>) -> Result<ExitCode> {
    // Commands talk to an already-running daemon, so an explicit endpoint wins
    // here. `daemon start` is the exception and resolves its own target from
    // the home it spawns the daemon in.
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
            // Read like `status` rather than driven like the rest: the log
            // comes from the daemon itself, so `--endpoint` reaches the one it
            // names, and the home is only what the file fallback is found in.
            DaemonCommand::Logs { follow } => {
                let client = Client::resolve(cli.endpoint.as_deref(), home.clone());
                commands::daemon::logs(&client, home, follow).await
            }
        },
        Command::Attach { id, seat, agent } => {
            commands::attach::attach_any(&client, &id, seat, agent.as_deref()).await
        }
        Command::Agent { command } => commands::agent::run(&client, command, format).await,
        Command::Skill { command } => commands::skill::run(&client, command, format).await,
        Command::Workflow { command } => commands::workflow::run(&client, command, format).await,
        Command::Dashboard { command } => commands::dashboard::run(&client, command, format).await,
        Command::Pr { command } => commands::pr::run(&client, command, format).await,
        Command::Repo { command } => commands::repo::run(&client, command, format).await,
        Command::Forge { command } => commands::forge::run(&client, command, format).await,
        Command::Issue { command } => commands::issue::run(&client, command, format).await,
        Command::Permissions { command } => {
            commands::permissions::run(&client, command, format).await
        }
        Command::Goal { command } => commands::goal::run(&client, command, format).await,
        Command::Task { command } => commands::task::run(&client, command, format).await,
        Command::Session { command } => commands::session::run(&client, command, format).await,
        Command::Models { command } => commands::models::run(&client, command, format).await,
        Command::Stats {
            command,
            since,
            repo,
        } => {
            let filters = commands::stats::Filters { since, repo };
            commands::stats::run(&client, command, filters, format).await
        }
        Command::Attention { watch } => commands::attention::run(&client, watch, format).await,
        Command::Events {
            follow,
            goal,
            task,
            session,
            kinds,
        } => {
            let filters = commands::events::Filters {
                goal,
                task,
                session,
                kinds,
            };
            commands::events::run(&client, filters, follow, format).await
        }
        Command::Mcp {
            command: McpCommand::Serve,
        } => commands::mcp::serve().await,
        // Handled above: it is the only command with an exit code of its own.
        Command::Doctor => unreachable!(),
    };
    outcome.map(|()| ExitCode::SUCCESS)
}

fn command_path(matches: &ArgMatches) -> String {
    let mut path = Vec::new();
    let mut current = matches;
    while let Some((name, nested)) = current.subcommand() {
        path.push(name);
        current = nested;
    }
    path.join(" ")
}
