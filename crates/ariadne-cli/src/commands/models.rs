//! `ariadne models ls` / `ariadne models show` — the catalogue every
//! `--model` is pinned from.

use anyhow::{Context, Result};
use clap::Subcommand;

use ariadne_api::models::{EffortDto, ModelDto};
use ariadne_client::Client;
use ariadne_core::AgentKind;

use super::parse_model;
use crate::output::{Column, Format, Kv, UNCAPPED, col, note, print, print_kv, print_list, view};

/// Columns of `models ls`. `model` is the whole id — `claude_code:o3`, not
/// `o3` — because that is the string `--model` takes, and a column somebody
/// copies out of has to be copyable. `agent` repeats the half of it that
/// groups the table, which is what the eye scans by.
///
/// `tier`, `cost` and `speed` are one word or one digit each, and they say
/// nothing the description does not already say in a sentence — so they are
/// the first three to go on a narrow terminal, ranked below everything else.
/// `efforts` is what a reader still needs to choose `--effort`, and the
/// description is the fullest account of what a model is for, so between the
/// two that a narrow terminal must still drop, description goes first, the
/// way it always has: `--model` and `--effort` are the choice a reader came
/// to make, and are kept longest.
const LS: &[Column] = &[
    col("agent", UNCAPPED),
    col("model", UNCAPPED).title(),
    col("tier", UNCAPPED).rank(0),
    col("cost", UNCAPPED).rank(1),
    col("speed", UNCAPPED).rank(2),
    col("efforts", 36).rank(4),
    col("description", 60).rank(3),
];

/// Where a continuation line of `models show` starts: [`print_kv`] pads its
/// keys to `description`, the longest one it prints.
const SHOW_KEY_WIDTH: usize = "description".len();

#[derive(Subcommand)]
pub enum ModelsCommand {
    /// List what every agent CLI can be pinned to
    Ls {
        /// Only what one agent CLI can be pinned to
        #[arg(long, value_parser = super::agent::parse_kind,
              add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_kinds))]
        agent: Option<AgentKind>,
    },
    /// Show what one model is, costs and is for
    Show {
        /// Model id, `<agent_kind>[:<model>]` — the same spelling `--model`
        /// takes
        #[arg(value_parser = parse_model,
              add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: String,
    },
}

pub async fn run(client: &Client, cmd: ModelsCommand, format: Format) -> Result<()> {
    match cmd {
        ModelsCommand::Ls { agent } => ls(client, agent, format).await,
        ModelsCommand::Show { model } => show(client, &model, format).await,
    }
}

async fn ls(client: &Client, agent: Option<AgentKind>, format: Format) -> Result<()> {
    let models: Vec<ModelDto> = client.get_json("/v1/models").await?;
    // `GET /v1/models` takes no filter: the catalogue is the union, so an
    // agent narrows what it answered rather than what was asked for.
    let models = of_agent(models, agent);
    let marks_a_default = models.iter().any(|m| m.efforts.iter().any(|e| e.default));
    print_list(
        format,
        &models,
        LS,
        row,
        match agent {
            // opencode's half of the catalogue is whatever `opencode models`
            // answered, which is nothing at all when it is not installed.
            Some(kind) => match kind {
                AgentKind::Opencode => "no opencode models — is opencode installed and signed in?",
                _ => "no models for that agent",
            },
            None => "no models — is the daemon of a version that serves them?",
        },
    )?;
    // The star is only ever printed on a table a person is reading, and only
    // when it says something: `--format json`, `-q` and a catalogue with no
    // default at all all go without.
    if format == Format::Table && !view().quiet && marks_a_default {
        note("* the effort that agent CLI runs the model at by default");
    }
    Ok(())
}

async fn show(client: &Client, model: &str, format: Format) -> Result<()> {
    let models: Vec<ModelDto> = client.get_json("/v1/models").await?;
    let m = find(models, model)?;
    print(format, &m, || print_card(&m))?;
    Ok(())
}

/// A row of `models ls`, in the order [`LS`] declares its columns.
fn row(m: &ModelDto) -> Vec<String> {
    vec![
        m.agent_kind.as_str().to_string(),
        m.id.clone(),
        m.tier.as_str().to_string(),
        band(m.cost),
        band(m.speed),
        effort_cell(&m.efforts),
        m.description.clone().unwrap_or_else(|| "-".into()),
    ]
}

/// The entry `models show` names, by exact id — the only match worth making:
/// a prefix would risk showing the wrong model for one typo, where `ariadne
/// models ls` is right there to copy the whole id from.
fn find(models: Vec<ModelDto>, id: &str) -> Result<ModelDto> {
    models
        .into_iter()
        .find(|m| m.id == id)
        .with_context(|| format!("no such model: {id} — see `ariadne models ls`"))
}

fn print_card(m: &ModelDto) {
    print_kv(&card_pairs(m));
}

/// The key/value pairs `models show` prints, in the order it prints them —
/// pulled out of [`print_card`] so the card's own content is testable
/// without printing anything.
///
/// `tier` stays as plain as it is in `models ls`: the table gives that column
/// no colour of its own, and a card is not the place to invent one.
fn card_pairs(m: &ModelDto) -> Vec<(&'static str, Kv)> {
    let indent = format!("\n{}", " ".repeat(SHOW_KEY_WIDTH + 2));
    vec![
        ("id", Kv::id(m.id.clone())),
        ("tier", m.tier.as_str().to_string().into()),
        ("cost", band(m.cost).into()),
        ("speed", band(m.speed).into()),
        (
            "description",
            m.description.clone().unwrap_or_else(|| "-".into()).into(),
        ),
        ("best_for", shapes(&m.best_for, &indent).into()),
        ("avoid_for", shapes(&m.avoid_for, &indent).into()),
        ("efforts", efforts_block(&m.efforts, &indent).into()),
    ]
}

/// The catalogue narrowed to one agent CLI, or all of it.
fn of_agent(models: Vec<ModelDto>, agent: Option<AgentKind>) -> Vec<ModelDto> {
    models
        .into_iter()
        .filter(|m| agent.is_none_or(|kind| m.agent_kind == kind))
        .collect()
}

/// `cost` or `speed` as a table or card cell: the band out of five, or `-`
/// where nothing knows it.
fn band(n: Option<u8>) -> String {
    match n {
        Some(n) => format!("{n}/5"),
        None => "-".into(),
    }
}

/// The `efforts` cell of `models ls`: every effort this entry takes,
/// cheapest first, the default one starred — `low, medium*, high` — or `-`
/// for a model with no effort control at all.
fn effort_cell(efforts: &[EffortDto]) -> String {
    match efforts.is_empty() {
        true => "-".into(),
        false => efforts
            .iter()
            .map(|e| match e.default {
                true => format!("{}*", e.id),
                false => e.id.clone(),
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// A `best_for` / `avoid_for` list as `models show` prints it: one shape per
/// line, or `-` where nothing knows any.
fn shapes(shapes: &[String], indent: &str) -> String {
    match shapes.is_empty() {
        true => "-".into(),
        false => shapes.join(indent),
    }
}

/// Every effort `models show` prints, cheapest first: its id, what it buys,
/// and `(default)` on the one the agent CLI runs the model at when none is
/// pinned.
fn efforts_block(efforts: &[EffortDto], indent: &str) -> String {
    match efforts.is_empty() {
        true => "-".into(),
        false => efforts
            .iter()
            .map(|e| {
                let description = e.description.as_deref().unwrap_or("-");
                match e.default {
                    true => format!("{} — {description} (default)", e.id),
                    false => format!("{} — {description}", e.id),
                }
            })
            .collect::<Vec<_>>()
            .join(indent),
    }
}

#[cfg(test)]
mod tests {
    use ariadne_core::ModelTier;

    use super::*;
    use crate::output::{View, render_table};

    fn model(id: &str, agent_kind: AgentKind) -> ModelDto {
        ModelDto {
            id: id.to_string(),
            agent_kind,
            description: None,
            tier: ModelTier::Unknown,
            cost: None,
            speed: None,
            best_for: Vec::new(),
            avoid_for: Vec::new(),
            efforts: Vec::new(),
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

    /// A band is the digit and its ceiling, or a dash for a model nothing
    /// has ranked.
    #[test]
    fn a_band_is_out_of_five_or_a_dash() {
        assert_eq!(band(Some(3)), "3/5");
        assert_eq!(band(Some(5)), "5/5");
        assert_eq!(band(None), "-");
    }

    fn effort(id: &str, description: Option<&str>, default: bool) -> EffortDto {
        EffortDto {
            id: id.to_string(),
            description: description.map(str::to_string),
            default,
        }
    }

    /// The default effort is starred in the list it sits in, the rest are
    /// bare, and a model with no efforts at all is a dash.
    #[test]
    fn the_effort_cell_stars_the_default_and_dashes_when_there_is_none() {
        assert_eq!(
            effort_cell(&[
                effort("low", None, false),
                effort("medium", None, true),
                effort("high", None, false),
            ]),
            "low, medium*, high"
        );
        assert_eq!(effort_cell(&[]), "-");
    }

    /// `models show` lists every effort with what it buys and marks the
    /// default one, and a model with none is a dash.
    #[test]
    fn the_efforts_block_describes_each_one_and_marks_the_default() {
        assert_eq!(
            efforts_block(
                &[
                    effort("low", Some("lighter reasoning"), false),
                    effort("high", Some("greater depth"), true),
                ],
                " | ",
            ),
            "low — lighter reasoning | high — greater depth (default)"
        );
        assert_eq!(
            efforts_block(&[effort("low", None, false)], " | "),
            "low — -"
        );
        assert_eq!(efforts_block(&[], " | "), "-");
    }

    /// A curated model, an entry with no bands at all, and one whose default
    /// effort is marked — the shape a daemon built from the merged base
    /// actually serves.
    fn fixture() -> Vec<ModelDto> {
        vec![
            ModelDto {
                id: "codex:gpt-5.6-luna".into(),
                agent_kind: AgentKind::Codex,
                description: Some("balanced coding model".into()),
                tier: ModelTier::Balanced,
                cost: Some(3),
                speed: Some(3),
                best_for: vec!["well-specified fixes".into()],
                avoid_for: vec!["cross-subsystem design".into()],
                efforts: vec![
                    EffortDto {
                        id: "low".into(),
                        description: Some("lighter reasoning".into()),
                        default: false,
                    },
                    EffortDto {
                        id: "medium".into(),
                        description: Some("balanced reasoning".into()),
                        default: true,
                    },
                ],
            },
            model("opencode:llama3", AgentKind::Opencode),
        ]
    }

    /// `models ls` carries the tier, cost and speed of a curated entry, `-`
    /// for the bands of one nothing curates, and the default effort starred.
    #[test]
    fn a_row_carries_the_bands_and_stars_the_default_effort() {
        assert_eq!(
            row(&fixture()[0]),
            [
                "codex",
                "codex:gpt-5.6-luna",
                "balanced",
                "3/5",
                "3/5",
                "low, medium*",
                "balanced coding model",
            ]
        );
        assert_eq!(
            row(&fixture()[1]),
            ["opencode", "opencode:llama3", "unknown", "-", "-", "-", "-"]
        );
    }

    /// `tier`, `cost` and `speed` are the first three columns dropped on a
    /// narrow terminal — they say nothing the description does not already
    /// say — and `--model`/`--effort`, the choice a reader came to make,
    /// are the two kept longest.
    #[test]
    fn the_bands_drop_before_efforts_and_description_do() {
        let rows: Vec<Vec<String>> = fixture().iter().map(row).collect();
        let headers = |view: &View| -> Vec<String> {
            render_table(LS, &rows, view)
                .expect("renders")
                .lines()
                .next()
                .expect("a header line")
                .split_whitespace()
                .map(str::to_string)
                .collect()
        };
        assert_eq!(
            headers(&View::plain()),
            [
                "AGENT",
                "MODEL",
                "TIER",
                "COST",
                "SPEED",
                "EFFORTS",
                "DESCRIPTION"
            ]
        );
        let narrow = headers(&View::at(40));
        assert!(
            !narrow.contains(&"TIER".to_string())
                && !narrow.contains(&"COST".to_string())
                && !narrow.contains(&"SPEED".to_string()),
            "{narrow:?}"
        );
        assert!(
            narrow.contains(&"AGENT".to_string()) && narrow.contains(&"MODEL".to_string()),
            "agent and model never drop: {narrow:?}"
        );
    }

    /// `models show` finds the entry whose id matches exactly, and refuses
    /// one that names no entry at all — pointing at `models ls` rather than
    /// guessing at a prefix.
    #[test]
    fn show_finds_the_exact_id_or_refuses_by_name() {
        let found = find(fixture(), "codex:gpt-5.6-luna").expect("found");
        assert_eq!(found.id, "codex:gpt-5.6-luna");

        let err = find(fixture(), "codex:nope").expect_err("not in the catalogue");
        let err = err.to_string();
        assert!(err.contains("codex:nope"), "{err}");
        assert!(err.contains("models ls"), "{err}");
    }

    /// A bare-CLI id (`codex`) is found by the same exact match — it is one
    /// entry in the catalogue like any other.
    #[test]
    fn show_finds_a_bare_cli_entry_too() {
        let bare = model("codex", AgentKind::Codex);
        let models = vec![bare.clone(), fixture()[0].clone()];
        assert_eq!(find(models, "codex").expect("found").id, bare.id);
    }

    /// The card carries every field the acceptance criteria name, in order:
    /// id, tier, cost, speed, description, `best_for`, `avoid_for`, then
    /// every effort with what it buys and the default one marked.
    #[test]
    fn the_card_carries_every_field_and_marks_the_default_effort() {
        let indent = format!("\n{}", " ".repeat(SHOW_KEY_WIDTH + 2));
        assert_eq!(
            card_pairs(&fixture()[0]),
            vec![
                ("id", Kv::id("codex:gpt-5.6-luna")),
                ("tier", "balanced".into()),
                ("cost", "3/5".into()),
                ("speed", "3/5".into()),
                ("description", "balanced coding model".into()),
                ("best_for", "well-specified fixes".into()),
                ("avoid_for", "cross-subsystem design".into()),
                (
                    "efforts",
                    format!("low — lighter reasoning{indent}medium — balanced reasoning (default)")
                        .into(),
                ),
            ]
        );
    }

    /// An entry nothing curates has nothing to show either: every band and
    /// every list is a dash, the way `models ls` shows the same entry.
    #[test]
    fn the_card_dashes_what_nothing_knows() {
        let bare = model("opencode:llama3", AgentKind::Opencode);
        assert_eq!(
            card_pairs(&bare),
            vec![
                ("id", Kv::id("opencode:llama3")),
                ("tier", "unknown".into()),
                ("cost", "-".into()),
                ("speed", "-".into()),
                ("description", "-".into()),
                ("best_for", "-".into()),
                ("avoid_for", "-".into()),
                ("efforts", "-".into()),
            ]
        );
    }

    /// `--format json` is the daemon's own payload and never touches the
    /// card; `--format table` (the default) renders it and nothing else.
    #[test]
    fn show_renders_the_card_for_table_and_skips_it_for_json() {
        let m = fixture()[0].clone();
        let mut card_calls = 0;
        print(Format::Json, &m, || card_calls += 1).expect("json");
        assert_eq!(card_calls, 0, "json prints the payload, not the card");
        print(Format::Table, &m, || card_calls += 1).expect("table");
        assert_eq!(card_calls, 1, "table renders the card exactly once");
    }
}
