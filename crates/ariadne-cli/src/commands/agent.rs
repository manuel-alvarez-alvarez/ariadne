//! `ariadne agent ...`

use anyhow::{Context, Result};
use clap::Subcommand;

use ariadne_api::agents::AgentConfigDto;
use ariadne_client::Client;
use ariadne_core::AgentKind;

use crate::output::{Column, Format, UNCAPPED, note, print, print_list};

/// Columns of `agent list`.
const LS: &[Column] = &[("agent", UNCAPPED), ("flags", 44), ("defaults", 44)];

/// What an empty flag list looks like in a table: a cell nobody can mistake
/// for a flag.
const EMPTY: &str = "-";

/// `ariadne agent ...` — how each coding-agent CLI is launched.
///
/// The flags belong to the agent kind, not to the persona: every profile that
/// runs on that CLI is spawned and resumed with them, and an edit lands on the
/// next launch. `ariadne profile` is the other half — the model, the role and
/// the prompts one agent runs with.
#[derive(Subcommand)]
pub enum AgentCommand {
    /// List the agent CLIs, their flags and the defaults they came from
    #[command(alias = "ls")]
    List {
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
    },
    /// Replace an agent CLI's flags
    ///
    /// The list is replaced whole: `--flag` names every flag the agent is to
    /// be launched with, `--clear-flags` launches it with none, and `--reset`
    /// puts back what Ariadne ships for that kind. Exactly one of the three.
    Update {
        /// claude_code | codex | opencode
        #[arg(value_parser = parse_kind, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_kinds))]
        kind: AgentKind,
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
        /// Put the flag list back to the default of this agent kind
        #[arg(long)]
        reset: bool,
    },
}

/// An agent kind as a command line spells it, or an error naming the ones
/// there are — the same answer the daemon would give, without the round trip.
///
/// Both spellings are accepted: `claude_code` is how the daemon writes it,
/// `claude-code` is how a shell tends to.
fn parse_kind(s: &str) -> Result<AgentKind, String> {
    s.replace('-', "_").parse().map_err(|_| {
        let known = AgentKind::ALL
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!("unknown agent kind: {s} (expected one of {known})")
    })
}

/// A flag list as a table shows it: the flags as they are launched, or
/// [`EMPTY`] when there are none.
fn flags_cell(flags: &[String]) -> String {
    match flags.is_empty() {
        true => EMPTY.to_string(),
        false => flags.join(" "),
    }
}

/// The flags Ariadne ships for `kind`, as the daemon reports them: what
/// `--reset` writes back, so a default that changes there changes here too.
async fn default_flags(client: &Client, kind: AgentKind) -> Result<Vec<String>> {
    client
        .list_agent_configs()
        .await?
        .into_iter()
        .find(|c| c.agent_kind == kind)
        .map(|c| c.default_flags)
        .with_context(|| format!("the daemon knows no {} agent", kind.as_str()))
}

pub async fn run(client: &Client, cmd: AgentCommand, format: Format) -> Result<()> {
    match cmd {
        AgentCommand::List { no_trunc } => {
            let configs: Vec<AgentConfigDto> = client.list_agent_configs().await?;
            print_list(
                format,
                &configs,
                LS,
                no_trunc,
                |c| {
                    vec![
                        c.agent_kind.as_str().into(),
                        flags_cell(&c.extra_flags),
                        flags_cell(&c.default_flags),
                    ]
                },
                "",
            )?;
        }
        AgentCommand::Update {
            kind,
            flags,
            clear_flags: _,
            reset,
        } => {
            let extra_flags = match reset {
                true => default_flags(client, kind).await?,
                // `--clear-flags` is the empty list, which is exactly what
                // `flags` already is: clap has seen to it that one of the two
                // was given, and that neither came with the other.
                false => flags,
            };
            let config = client.update_agent_config(kind, extra_flags).await?;
            print(format, &config, || {
                println!("{}", config.agent_kind.as_str());
                note(&format!("flags: {}", flags_cell(&config.extra_flags)));
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_is_spelled_as_the_daemon_spells_it() {
        assert_eq!(parse_kind("claude_code"), Ok(AgentKind::ClaudeCode));
        assert_eq!(parse_kind("codex"), Ok(AgentKind::Codex));
        assert_eq!(parse_kind("opencode"), Ok(AgentKind::Opencode));
    }

    /// `claude-code` is what fingers type; it means the same agent.
    #[test]
    fn the_dash_spelling_names_the_same_agent() {
        assert_eq!(parse_kind("claude-code"), Ok(AgentKind::ClaudeCode));
    }

    /// A typo must not send the caller to `--help` to find the spelling.
    #[test]
    fn an_unknown_kind_lists_the_kinds_that_exist() {
        let err = parse_kind("emacs").expect_err("unknown");
        assert!(err.starts_with("unknown agent kind: emacs"), "{err}");
        for kind in AgentKind::ALL {
            assert!(err.contains(kind.as_str()), "{err}");
        }
    }

    #[test]
    fn a_flag_list_reads_as_a_command_line_and_an_empty_one_as_a_dash() {
        assert_eq!(flags_cell(&[]), EMPTY);
        assert_eq!(
            flags_cell(&["--auto".into(), "--verbose".into()]),
            "--auto --verbose"
        );
    }
}
