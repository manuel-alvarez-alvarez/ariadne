//! `ariadne models ls` — the catalogue every `--model` is pinned from.

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::models::ModelDto;
use ariadne_client::Client;
use ariadne_core::AgentKind;

use crate::output::{Column, Format, UNCAPPED, col, print_list};

/// Columns of `models ls`. `model` is the whole id — `claude_code:o3`, not
/// `o3` — because that is the string `--model` takes, and a column somebody
/// copies out of has to be copyable. `agent` repeats the half of it that
/// groups the table, which is what the eye scans by. `efforts` is what that
/// model can be run at, cheapest first, and `-` where it takes none.
/// The description is the one cell a narrow terminal can do without: the
/// model id is what a reader came for, and what they will paste into
/// `--model`, and the efforts are the other half of the same choice.
const LS: &[Column] = &[
    col("agent", UNCAPPED),
    col("model", UNCAPPED).title(),
    col("efforts", 36).rank(1),
    col("description", 60).rank(0),
];

#[derive(Subcommand)]
pub enum ModelsCommand {
    /// List what every agent CLI can be pinned to
    Ls {
        /// Only what one agent CLI can be pinned to
        #[arg(long, value_parser = super::agent::parse_kind,
              add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_kinds))]
        agent: Option<AgentKind>,
    },
}

pub async fn run(client: &Client, cmd: ModelsCommand, format: Format) -> Result<()> {
    let ModelsCommand::Ls { agent } = cmd;
    let models: Vec<ModelDto> = client.get_json("/v1/models").await?;
    // `GET /v1/models` takes no filter: the catalogue is the union, so an
    // agent narrows what it answered rather than what was asked for.
    let models = of_agent(models, agent);
    print_list(
        format,
        &models,
        LS,
        |m| {
            vec![
                m.agent_kind.as_str().to_string(),
                m.id.clone(),
                match m.efforts.is_empty() {
                    true => "-".into(),
                    false => m.efforts.join(", "),
                },
                m.description.clone().unwrap_or_else(|| "-".into()),
            ]
        },
        match agent {
            // opencode's half of the catalogue is whatever `opencode models`
            // answered, which is nothing at all when it is not installed.
            Some(kind) => match kind {
                AgentKind::Opencode => "no opencode models — is opencode installed and signed in?",
                _ => "no models for that agent",
            },
            None => "no models — is the daemon of a version that serves them?",
        },
    )
}

/// The catalogue narrowed to one agent CLI, or all of it.
fn of_agent(models: Vec<ModelDto>, agent: Option<AgentKind>) -> Vec<ModelDto> {
    models
        .into_iter()
        .filter(|m| agent.is_none_or(|kind| m.agent_kind == kind))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, agent_kind: AgentKind) -> ModelDto {
        ModelDto {
            id: id.to_string(),
            agent_kind,
            description: None,
            efforts: Vec::new(),
            default_effort: None,
        }
    }

    fn catalogue() -> Vec<ModelDto> {
        vec![
            model("claude_code", AgentKind::ClaudeCode),
            model("claude_code:claude-fable-5", AgentKind::ClaudeCode),
            model("codex:gpt-5.6-luna", AgentKind::Codex),
        ]
    }

    fn ids(models: Vec<ModelDto>) -> Vec<String> {
        models.into_iter().map(|m| m.id).collect()
    }

    /// The whole catalogue by default, and one CLI's share of it — the entry
    /// for the CLI on its own default model included — when one is named.
    #[test]
    fn an_agent_narrows_the_catalogue_to_its_own() {
        assert_eq!(ids(of_agent(catalogue(), None)).len(), 3);
        assert_eq!(
            ids(of_agent(catalogue(), Some(AgentKind::ClaudeCode))),
            ["claude_code", "claude_code:claude-fable-5"]
        );
        assert_eq!(
            ids(of_agent(catalogue(), Some(AgentKind::Opencode))),
            [] as [String; 0]
        );
    }
}
