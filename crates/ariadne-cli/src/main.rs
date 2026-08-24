//! ariadne — CLI for the Ariadne daemon.

mod codex_trust;
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

use crate::commands::agent::AgentCommand;
use crate::commands::goal::GoalCommand;
use crate::commands::profile::ProfileCommand;
use crate::commands::repo::RepoCommand;
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

    // `doctor` is the one command whose verdict is an exit code rather than a
    // failure: a report that found something is a report, not an error.
    if let Command::Doctor = cli.command {
        return commands::doctor::run(&client, cli.format).await;
    }

    let outcome = match cli.command {
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
        Command::Agent { command } => commands::agent::run(&client, command, cli.format).await,
        Command::Profile { command } => commands::profile::run(&client, command, cli.format).await,
        Command::Repo { command } => commands::repo::run(&client, command, cli.format).await,
        Command::Goal { command } => commands::goal::run(&client, command, cli.format).await,
        Command::Task { command } => commands::task::run(&client, command, cli.format).await,
        Command::Session { command } => commands::session::run(&client, command, cli.format).await,
        Command::Attention { no_trunc } => {
            commands::attention::run(&client, no_trunc, cli.format).await
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_core::{GoalStatus, Role, TaskStatus};

    /// clap's own consistency check over the whole tree, shadowed `--format`
    /// arguments included.
    #[test]
    fn the_command_tree_is_well_formed() {
        command().debug_assert();
    }

    /// Every command a user can actually run, and whether `--format` shapes
    /// what it prints. Hidden internal commands are in here too: `--format`
    /// is global, so it reaches them whether or not anyone meant it to.
    const LEAVES: &[(&str, bool)] = &[
        ("_spawn", false),
        ("agent list", true),
        ("agent update", true),
        ("agent-event", false),
        ("attach", false),
        ("attention", true),
        ("completions", false),
        ("daemon logs", false),
        ("daemon start", true),
        ("daemon status", true),
        ("daemon stop", true),
        ("doctor", true),
        ("goal attach", false),
        ("goal cancel", true),
        ("goal create", true),
        ("goal finalize", true),
        ("goal inspect", true),
        ("goal ls", true),
        ("goal messages", true),
        ("goal msg", true),
        ("goal rm", true),
        ("mcp serve", false),
        ("profile create", true),
        ("profile inspect", true),
        ("profile ls", true),
        ("profile prompt get", true),
        ("profile prompt reset", true),
        ("profile prompt set", true),
        ("profile prompts", true),
        ("profile rm", true),
        ("profile update", true),
        ("repo add", true),
        ("repo edit", true),
        ("repo inspect", true),
        ("repo ls", true),
        ("repo rm", true),
        ("session inspect", true),
        ("session kill", true),
        ("session logs", true),
        ("session ls", true),
        ("session resume", true),
        ("setup codex-hooks", false),
        ("task attach", false),
        ("task cancel", true),
        ("task create", true),
        ("task diff", true),
        ("task history", true),
        ("task inspect", true),
        ("task logs", true),
        ("task ls", true),
        ("task messages", true),
        ("task msg", true),
        ("task retry", true),
        ("task reviews", true),
        ("task update", true),
        ("version", true),
    ];

    /// The list above is the whole tree, so a command added later is not
    /// classified by accident: it fails here until someone says whether
    /// `--format` means anything to it.
    #[test]
    fn every_command_in_the_tree_is_classified() {
        let mut expected: Vec<&str> = LEAVES.iter().map(|(path, _)| *path).collect();
        expected.sort_unstable();
        assert_eq!(leaf_paths(), expected);
    }

    #[test]
    fn format_is_advertised_exactly_where_it_is_honored() {
        for (path, honored) in LEAVES {
            let path: Vec<&str> = path.split(' ').collect();
            assert_eq!(
                advertises_format(&path),
                *honored,
                "--format is {} on {path:?}",
                if *honored { "missing" } else { "advertised" }
            );
        }
    }

    /// A path in [`NO_FORMAT`] that names no command hides nothing —
    /// `mut_subcommand` shrugs at a name it does not know.
    #[test]
    fn every_no_format_path_names_a_real_command() {
        let mut cmd = command();
        cmd.build();
        for path in NO_FORMAT {
            let mut sub = &cmd;
            for name in *path {
                sub = sub
                    .find_subcommand(name)
                    .unwrap_or_else(|| panic!("NO_FORMAT names no command: {path:?}"));
            }
        }
    }

    /// `_spawn` is tmux's end of a launch, not a command anyone types: it
    /// takes the plan path and stays out of the help.
    #[test]
    fn the_spawn_command_takes_a_plan_path_and_is_hidden() {
        let Command::Spawn { plan } = parse(&["ariadne", "_spawn", "/run/s/spawn.json"]).command
        else {
            panic!("_spawn");
        };
        assert_eq!(plan, PathBuf::from("/run/s/spawn.json"));
        assert!(
            try_parse(&["ariadne", "_spawn"]).is_err(),
            "a launch with no plan is a usage error"
        );
        let mut cmd = command();
        cmd.build();
        assert!(
            cmd.find_subcommand("_spawn").expect("_spawn").is_hide_set(),
            "_spawn is advertised in the help"
        );
    }

    /// `goal ls --status` names as many statuses as the caller likes, and only
    /// real ones: an unknown status is clap's refusal here rather than a
    /// request the daemon has to turn down.
    #[test]
    fn listing_goals_takes_any_number_of_real_statuses() {
        let statuses = |args: &[&str]| {
            let mut argv = vec!["ariadne", "goal", "ls"];
            argv.extend_from_slice(args);
            let Command::Goal {
                command: GoalCommand::Ls { statuses, .. },
            } = parse(&argv).command
            else {
                panic!("goal ls");
            };
            statuses
        };
        assert_eq!(statuses(&[]), []);
        assert_eq!(statuses(&["--status", "active"]), [GoalStatus::Active]);
        assert_eq!(
            statuses(&["--status", "active", "--status", "completed"]),
            [GoalStatus::Active, GoalStatus::Completed]
        );
        let Err(err) = try_parse(&["ariadne", "goal", "ls", "--status", "done"]) else {
            panic!("\"done\" is not a goal status");
        };
        let msg = err.to_string();
        assert!(msg.contains("invalid value 'done'"), "{msg}");
        assert!(msg.contains("completed"), "the refusal lists the real ones");
    }

    /// `--to` is what addresses a message, and leaving it out is still the
    /// unaddressed post it has always been.
    #[test]
    fn a_message_is_addressed_only_when_to_says_so() {
        let goal_to = |args: &[&str]| {
            let mut argv = vec!["ariadne", "goal", "msg", "01GOAL", "ping"];
            argv.extend_from_slice(args);
            let Command::Goal {
                command: GoalCommand::Msg { to, .. },
            } = parse(&argv).command
            else {
                panic!("goal msg");
            };
            to
        };
        assert_eq!(goal_to(&[]), None);
        assert_eq!(goal_to(&["--to", "user"]).as_deref(), Some("user"));

        let task_to = |args: &[&str]| {
            let mut argv = vec!["ariadne", "task", "msg", "01TASK", "ping"];
            argv.extend_from_slice(args);
            let Command::Task {
                command: TaskCommand::Msg { to, .. },
            } = parse(&argv).command
            else {
                panic!("task msg");
            };
            to
        };
        assert_eq!(task_to(&[]), None);
        assert_eq!(task_to(&["--to", "Engineer"]).as_deref(), Some("Engineer"));
    }

    /// `session ls --role` names one of the four roles, and nothing else
    /// reaches the list to be filtered on.
    #[test]
    fn listing_sessions_takes_one_real_role() {
        let Command::Session {
            command: SessionCommand::Ls { role, .. },
        } = parse(&["ariadne", "session", "ls", "--role", "reviewer"]).command
        else {
            panic!("session ls");
        };
        assert_eq!(role, Some(Role::Reviewer));
        assert!(
            try_parse(&["ariadne", "session", "ls", "--role", "critic"]).is_err(),
            "an unknown role is a usage error"
        );
    }

    /// The integrator is a per-task assignment like the engineer, and the
    /// wire requires one, so `task create` always names it: the built-in
    /// local one when the caller does not say, exactly as `--engineer`
    /// defaults to the built-in Engineer.
    #[test]
    fn creating_a_task_always_names_an_integrator() {
        let integrator = |args: &[&str]| {
            let mut argv = vec!["ariadne", "task", "create", "01GOAL", "--title", "t"];
            argv.extend_from_slice(args);
            let Command::Task {
                command: TaskCommand::Create { integrator, .. },
            } = parse(&argv).command
            else {
                panic!("task create");
            };
            integrator
        };
        assert_eq!(integrator(&[]), "Integrator");
        assert_eq!(
            integrator(&["--integrator", "My Integrator"]),
            "My Integrator"
        );
    }

    /// The status the board grew a column for is one the terminal can filter
    /// on, spelled the way the wire spells it.
    #[test]
    fn tasks_can_be_listed_by_the_status_their_integrator_holds_them_in() {
        let Command::Task {
            command: TaskCommand::Ls { status, .. },
        } = parse(&["ariadne", "task", "ls", "--status", "integrating"]).command
        else {
            panic!("task ls");
        };
        assert_eq!(status, Some(TaskStatus::Integrating));
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

    /// The flag list is replaced whole, so a line has to say how: name the
    /// flags, clear them, or go back to the default — and never two of those.
    #[test]
    fn updating_an_agent_takes_flags_or_clear_or_reset_but_only_one() {
        let update = |args: &[&str]| {
            let mut argv = vec!["ariadne", "agent", "update", "claude_code"];
            argv.extend_from_slice(args);
            try_parse(&argv).is_ok()
        };
        assert!(!update(&[]), "nothing to do");
        assert!(update(&["--flag", "--verbose"]), "flags");
        assert!(update(&["--clear-flags"]), "--clear-flags");
        assert!(update(&["--reset"]), "--reset");
        assert!(
            !update(&["--flag", "--verbose", "--reset"]),
            "flags + reset"
        );
        assert!(!update(&["--flag", "--verbose", "--clear-flags"]), "both");
        assert!(!update(&["--clear-flags", "--reset"]), "clear + reset");
    }

    /// Every flag an agent takes starts with a dash, so a `--flag` value that
    /// reads like a flag of clap's own has to reach the daemon as it was
    /// typed — that is the whole point of the option.
    #[test]
    fn an_agent_flag_that_looks_like_a_flag_is_taken_as_it_is() {
        let Command::Agent {
            command: AgentCommand::Update { kind, flags, .. },
        } = parse(&[
            "ariadne",
            "agent",
            "update",
            "claude-code",
            "--flag",
            "--dangerously-skip-permissions",
            "--flag",
            "--verbose",
        ])
        .command
        else {
            panic!("agent update");
        };
        // The dash spelling names the agent the daemon calls claude_code.
        assert_eq!(kind, ariadne_core::AgentKind::ClaudeCode);
        assert_eq!(flags, ["--dangerously-skip-permissions", "--verbose"]);
    }

    /// An agent nobody runs is a usage error: no daemon is asked about it,
    /// and the message says which agents there are.
    #[test]
    fn an_unknown_agent_kind_never_reaches_the_daemon() {
        let err = try_parse(&["ariadne", "agent", "update", "emacs", "--reset"])
            .err()
            .expect("unknown agent kind")
            .to_string();
        assert!(err.contains("unknown agent kind: emacs"), "{err}");
        for kind in ariadne_core::AgentKind::ALL {
            assert!(err.contains(kind.as_str()), "{err}");
        }
    }

    /// `profile prompt reset` has to be told what to reset: one kind, or
    /// `--all`, and never both at once.
    #[test]
    fn resetting_a_prompt_takes_a_kind_or_all_but_not_both() {
        let reset = |args: &[&str]| {
            let mut argv = vec!["ariadne", "profile", "prompt", "reset", "Engineer"];
            argv.extend_from_slice(args);
            try_parse(&argv).is_ok()
        };
        assert!(!reset(&[]), "neither");
        assert!(reset(&["system"]), "a kind");
        assert!(reset(&["--all"]), "--all");
        assert!(!reset(&["system", "--all"]), "both");
    }

    /// A kind no role owns is a usage error: no daemon is asked about it.
    #[test]
    fn an_unknown_prompt_kind_never_reaches_the_daemon() {
        let get =
            |kind: &str| try_parse(&["ariadne", "profile", "prompt", "get", "Engineer", kind]);
        assert!(get("engineer_briefing").is_ok());
        assert!(get("system").is_ok());
        let err = get("briefing").err().expect("unknown kind").to_string();
        assert!(err.contains("unknown prompt kind: briefing"), "{err}");
        assert!(err.contains("engineer_briefing"), "{err}");
    }

    /// The prompt flags name the kind they set, so one line can seed the
    /// system prompt and a briefing at once, from text and from a file.
    #[test]
    fn a_prompt_flag_names_the_kind_it_sets() {
        let (texts, files) = create_flags(&[
            "--prompt",
            "system=You are...",
            "--prompt",
            "changes_requested=Fix it",
            "--prompt-file",
            "engineer_briefing=/tmp/b.md",
        ]);
        assert_eq!(
            pairs(texts),
            ["system=You are...", "changes_requested=Fix it"]
        );
        assert_eq!(pairs(files), ["engineer_briefing=/tmp/b.md"]);
    }

    /// `profile update` takes the same flags as `profile create`.
    #[test]
    fn an_update_sets_prompts_by_kind_too() {
        let Command::Profile {
            command:
                ProfileCommand::Update {
                    prompts,
                    prompt_files,
                    ..
                },
        } = parse(&[
            "ariadne",
            "profile",
            "update",
            "Engineer",
            "--prompt",
            "system=You are...",
            "--prompt-file",
            "changes_requested=/tmp/c.md",
        ])
        .command
        else {
            panic!("profile update");
        };
        assert_eq!(pairs(prompts), ["system=You are..."]);
        assert_eq!(pairs(prompt_files), ["changes_requested=/tmp/c.md"]);
    }

    /// The old bare `--prompt <text>` is gone: a value with no kind is a
    /// usage error, and the message says how to spell what was meant.
    #[test]
    fn a_prompt_without_a_kind_is_a_usage_error() {
        let err = try_create(&["--prompt", "You are..."])
            .expect_err("no kind")
            .to_string();
        assert!(err.contains("missing <kind>="), "{err}");
        assert!(err.contains("write system=<text>"), "{err}");
        assert!(err.contains("engineer_briefing"), "{err}");
        let err = try_create(&["--prompt-file", "/tmp/b.md"])
            .expect_err("no kind")
            .to_string();
        assert!(err.contains("write system=<path>"), "{err}");
    }

    /// A kind no role owns never reaches the daemon, on either flag.
    #[test]
    fn an_unknown_kind_on_a_prompt_flag_never_reaches_the_daemon() {
        for flag in ["--prompt", "--prompt-file"] {
            let err = try_create(&[flag, "briefing=x"])
                .expect_err("unknown kind")
                .to_string();
            assert!(err.contains("unknown prompt kind: briefing"), "{err}");
            assert!(err.contains("system"), "{err}");
        }
    }

    /// One kind, one value: a second one for the same prompt is refused
    /// before anything is sent, whichever flag carries it.
    #[test]
    fn the_same_kind_twice_is_refused_before_any_request() {
        let (texts, files) = create_flags(&[
            "--prompt",
            "engineer_briefing=one",
            "--prompt-file",
            "engineer_briefing=/tmp/b.md",
        ]);
        let err = commands::profile::read_prompts(texts, files)
            .expect_err("duplicate")
            .to_string();
        assert!(err.starts_with("engineer_briefing is set twice"), "{err}");
    }

    /// The prompt flags of a `profile create` line, as clap parsed them.
    fn create_flags(
        args: &[&str],
    ) -> (
        Vec<commands::profile::PromptAssignment>,
        Vec<commands::profile::PromptAssignment>,
    ) {
        let Command::Profile {
            command:
                ProfileCommand::Create {
                    prompts,
                    prompt_files,
                    ..
                },
        } = parse(&create_argv(args)).command
        else {
            panic!("profile create");
        };
        (prompts, prompt_files)
    }

    fn try_create(args: &[&str]) -> Result<(), clap::Error> {
        try_parse(&create_argv(args)).map(|_| ())
    }

    /// A `profile create` line with everything it needs, plus `args`.
    fn create_argv<'a>(args: &[&'a str]) -> Vec<&'a str> {
        let mut argv = vec![
            "ariadne", "profile", "create", "--name", "X", "--role", "engineer",
        ];
        argv.extend_from_slice(args);
        argv
    }

    /// Prompt flags back as `<kind>=<value>`, the way they were typed.
    fn pairs(args: Vec<commands::profile::PromptAssignment>) -> Vec<String> {
        args.into_iter()
            .map(|a| format!("{}={}", a.kind.as_str(), a.value))
            .collect()
    }

    fn parse(argv: &[&str]) -> Cli {
        Cli::from_arg_matches(&command().get_matches_from(argv)).expect("parse")
    }

    fn try_parse(argv: &[&str]) -> Result<Cli, clap::Error> {
        Cli::from_arg_matches(&command().try_get_matches_from(argv)?)
    }

    /// Every runnable command in the tree, sorted, as `"task ls"` —
    /// hidden ones included, clap's generated `help` left out.
    fn leaf_paths() -> Vec<String> {
        fn walk(cmd: &clap::Command, prefix: &str, out: &mut Vec<String>) {
            let mut leaf = true;
            for sub in cmd.get_subcommands().filter(|s| s.get_name() != "help") {
                leaf = false;
                let path = match prefix {
                    "" => sub.get_name().to_string(),
                    _ => format!("{prefix} {}", sub.get_name()),
                };
                walk(sub, &path, out);
            }
            if leaf && !prefix.is_empty() {
                out.push(prefix.to_string());
            }
        }
        let mut cmd = command();
        cmd.build();
        let mut out = Vec::new();
        walk(&cmd, "", &mut out);
        out.sort();
        out
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
