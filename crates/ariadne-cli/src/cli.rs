//! The command tree: every command, flag and argument `ariadne` accepts.

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};

pub mod values;

use crate::commands::agent::AgentCommand;
use crate::commands::goal::GoalCommand;
use crate::commands::models::ModelsCommand;
use crate::commands::profile::ProfileCommand;
use crate::commands::repo::RepoCommand;
use crate::commands::session::SessionCommand;
use crate::commands::task::TaskCommand;
use crate::output::Format;

/// Where the two flags every command takes are listed. They belong to no
/// command in particular, so they are not filed under any command's options.
pub const GLOBAL: &str = "Global options";

/// What `ariadne --help` ends with: the first goal, start to finish, as the
/// README's quick start runs it.
const EXAMPLES: &str = "\
Examples:
  ariadne daemon start                     # the daemon everything else talks to
  ariadne repo add ~/projects/api          # register a checkout to work in
  ariadne goal create --title \"Add rate limiting\" --repo ~/projects/api
  ariadne goal attach <goal-id>            # agree the plan with the planner
  ariadne attention                        # what is waiting for you, every goal
  ariadne task ls --goal <goal-id>         # the tasks it was broken into
  ariadne attach <id>                      # a session, task or goal id
";

/// What each group's help ends with: what that group is most often asked to
/// do, in the spelling it takes today.
const DAEMON_EXAMPLES: &str = "\
Examples:
  ariadne daemon start                     # or --home <dir> for another home
  ariadne daemon status                    # and which service manages it
  ariadne daemon logs --follow
  ariadne daemon restart
  ariadne daemon stop
";

const AGENT_EXAMPLES: &str = "\
Examples:
  ariadne agent ls                         # the flags every CLI is launched with
  ariadne agent update codex --flag --dangerously-bypass-approvals-and-sandbox
  ariadne agent update claude_code --reset # back to what Ariadne ships
";

const PROFILE_EXAMPLES: &str = "\
Examples:
  ariadne profile ls --role reviewer
  ariadne profile create --name Architect --role planner --model codex:gpt-5.3-codex
  ariadne profile update Reviewer --model default
  ariadne profile prompt get Engineer engineer_briefing > briefing.md
";

const REPO_EXAMPLES: &str = "\
Examples:
  ariadne repo add ~/projects/api --description \"the public API\"
  ariadne repo add ~/projects/ui --branch next --merge-strategy pull-request
  ariadne repo ls
  ariadne repo update <repo-id> --branch main
";

const GOAL_EXAMPLES: &str = "\
Examples:
  ariadne goal create --title \"Add rate limiting\" --repo ~/projects/api
  ariadne goal ls --status planning,active
  ariadne goal attach <goal-id>            # the planner's terminal
  ariadne goal msg <goal-id> \"the middleware crate, not a new one\"
  ariadne goal thread <goal-id>            # the whole conversation
";

const TASK_EXAMPLES: &str = "\
Examples:
  ariadne task ls --goal <goal-id>
  ariadne task ls --status in-progress,under-review
  ariadne task inspect <task-id>           # and: diff, reviews, history, thread
  ariadne task msg <task-id> \"hold on, use the middleware crate instead\"
  ariadne task attach <task-id>            # the engineer's terminal
";

const SESSION_EXAMPLES: &str = "\
Examples:
  ariadne session ls                       # every live session
  ariadne session ls --all --goal <goal-id>
  ariadne session logs <session-id>        # what its pane last printed
  ariadne session resume <session-id>      # new tmux, same conversation
  ariadne session kill <session-id>
";

/// What `ariadne attach --help` ends with.
const ATTACH_EXAMPLES: &str = "\
Examples:
  ariadne attach <goal-id>                 # the goal's planner
  ariadne attach <task-id>                 # the task's engineer
  ariadne attach <task-id> --role reviewer # its reviewer instead
  ariadne attach <session-id>              # that one session
";

#[derive(Parser)]
#[command(
    name = "ariadne",
    version,
    about = "Coding-agent orchestrator CLI",
    long_about = "Coding-agent orchestrator CLI.\n\n\
        Every command here asks the ariadned daemon for something. It plans a \
        goal with a planner agent, hands each task to an engineer that owns it \
        from its first commit to the merge that lands it, and gates that merge \
        behind reviewer agents. Each of them works in a tmux session `ariadne \
        attach` drops you into, and `ariadne attention` is what says which of \
        them is waiting for you.",
    after_help = EXAMPLES
)]
pub struct Cli {
    /// Daemon endpoint: unix socket path or http://host:port
    // `--host` was the old name and never meant a host; it stays as an
    // undocumented alias so existing scripts keep working.
    //
    // No `env =`: the environment is read where the endpoint is resolved
    // (`Client::resolve`), so no help screen carries the value this shell
    // happens to have, and no usage line reads as though the flag were
    // required.
    #[arg(
        long,
        alias = "host",
        global = true,
        help_heading = GLOBAL,
        long_help = "Daemon endpoint: a unix socket path, or an http://host:port URL where \
            the daemon has a TCP listener.\n\n\
            Resolved in this order: this flag, $ARIADNE_ENDPOINT, $ARIADNE_SOCKET \
            (the older name for it), then the socket of the ariadne home — \
            $ARIADNE_HOME or ~/.ariadne — as that home's config.toml names it."
    )]
    pub endpoint: Option<String>,

    /// Output format
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "table",
        help_heading = GLOBAL
    )]
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
    ///
    /// Every one of these is about the daemon of one home — its process, its
    /// pidfile and its socket all live in one — so `--home` is the group's and
    /// applies to all of them. Where a service manager holds that home's
    /// daemon (launchd, `systemd --user`), it is what start, stop and restart
    /// drive, and each says which command it used.
    #[command(after_help = DAEMON_EXAMPLES)]
    Daemon {
        /// Ariadne home directory (default: $ARIADNE_HOME or ~/.ariadne)
        #[arg(long, global = true)]
        home: Option<PathBuf>,
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Manage the agent CLIs: the flags each one is launched with
    ///
    /// One entry per coding-agent CLI Ariadne can run — claude_code, codex,
    /// opencode — holding the flags every session of that CLI is launched and
    /// resumed with. `ariadne profile` is the other half: the persona, this
    /// is the program it runs in.
    #[command(after_help = AGENT_EXAMPLES)]
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// List what agents can be pinned to
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
    /// Manage agent profiles
    ///
    /// A profile is one agent as it is spawned: the role it plays, what it
    /// runs on, and the prompts it is briefed and resumed with. Goals and
    /// tasks are assigned to profiles by name, and a change here reaches
    /// every session started after it.
    #[command(after_help = PROFILE_EXAMPLES)]
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Manage repositories
    ///
    /// The checkouts goals may work in. A repository is registered once —
    /// with the base branch its tasks branch off and how an approved task
    /// lands on it — and named by every goal after that; the same checkout
    /// can be registered once per base branch.
    #[command(after_help = REPO_EXAMPLES)]
    Repo {
        #[command(subcommand)]
        command: RepoCommand,
    },
    /// Manage goals
    ///
    /// A goal is a whole effort. A planner agent breaks it into tasks and
    /// discusses the breakdown with you in the goal's conversation — `goal
    /// thread` reads it, `goal msg` writes to it — and nothing runs until you
    /// confirm the plan there.
    #[command(after_help = GOAL_EXAMPLES)]
    Goal {
        #[command(subcommand)]
        command: GoalCommand,
    },
    /// Manage tasks
    ///
    /// A task is one unit of a goal, owned by an engineer agent in a worktree
    /// of its own from its first commit to the merge that lands it, with
    /// reviewer agents gating that merge. Its diff, its reviews, its history
    /// and its conversation are all here.
    #[command(after_help = TASK_EXAMPLES)]
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Manage agent sessions
    ///
    /// A session is one agent in one tmux window: the terminal a planner, an
    /// engineer or a reviewer is actually working in. They are listed
    /// docker-style — live ones by default, finished ones behind --all — and
    /// one that has ended can be revived with the same conversation.
    #[command(after_help = SESSION_EXAMPLES)]
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
    ///
    /// The terminal of whichever agent that id names, revived first when its
    /// tmux is gone. Leaving it is tmux's own detach (Ctrl-b d): the agent
    /// keeps working.
    #[command(after_help = ATTACH_EXAMPLES)]
    Attach {
        /// Session, task or goal id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::attach_ids))]
        id: String,
        /// Which agent of that id to attach to (default: engineer for tasks,
        /// planner for goals; not valid with a session id)
        #[arg(long, value_parser = values::Spelling::<ariadne_core::Role>::new())]
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
        /// Which agent's hook is reporting, as everything else spells it:
        /// claude_code | codex | opencode
        #[arg(long, default_value = "claude_code", value_parser = crate::commands::agent::parse_kind)]
        kind: ariadne_core::AgentKind,
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
    Start,
    /// Stop a running ariadned and wait for it to be gone
    Stop {
        /// Seconds to wait for the daemon's socket to disappear
        #[arg(long, default_value_t = STOP_TIMEOUT)]
        timeout: u64,
    },
    /// Stop a running ariadned and start it again
    Restart {
        /// Seconds to wait for the daemon's socket to disappear
        #[arg(long, default_value_t = STOP_TIMEOUT)]
        timeout: u64,
    },
    /// Show daemon status, and which service manages it
    Status,
    /// Show (or follow) the daemon log
    Logs {
        /// Follow the log (tail -f)
        #[arg(short, long)]
        follow: bool,
    },
}

/// How long `daemon stop` and `daemon restart` wait for the daemon to be
/// gone before giving up on it, in seconds.
pub const STOP_TIMEOUT: u64 = 10;

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
