//! `ariadne stats <family>` — how the tool and the models perform, read off
//! the daemon's stats ledger.

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::stats::{ModelStatDto, ModelStatsResponse, StatsQuery};
use ariadne_client::Client;

use super::query_path;
use super::resolve::{self, Kind};
use crate::output::{
    Column, Format, UNCAPPED, col, dash, duration, empty_state, print_list, usage_cell,
};

/// Columns of `stats models`. The model and its seat are what a row is about,
/// and never drop; the skills are the first to go on a narrow terminal.
const MODELS: &[Column] = &[
    col("model", UNCAPPED).title(),
    col("seat", UNCAPPED),
    col("sessions", UNCAPPED),
    col("failed", UNCAPPED),
    col("stalled", UNCAPPED),
    col("tokens", UNCAPPED).rank(2),
    col("lifetime", UNCAPPED).rank(1),
    col("skills", 48).rank(0),
];

#[derive(Subcommand)]
pub(crate) enum StatsCommand {
    /// How each model did in each seat: sessions, failures, stalls, tokens
    Models,
}

/// The filters every family takes, as the command line gave them.
pub(crate) struct Filters {
    pub since: Option<String>,
    pub repo: Option<String>,
}

pub(crate) async fn run(
    client: &Client,
    cmd: Option<StatsCommand>,
    filters: Filters,
    format: Format,
) -> Result<()> {
    let repo = match filters.repo {
        Some(repo) => Some(resolve::id(client, Kind::Repo, &repo).await?),
        None => None,
    };
    let query = StatsQuery {
        since: filters.since,
        repo,
    };
    match cmd.unwrap_or(StatsCommand::Models) {
        StatsCommand::Models => models(client, &query, format).await,
    }
}

async fn models(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let response: ModelStatsResponse = client
        .get_json(&query_path("/v1/stats/models", query)?)
        .await?;
    print_list(
        format,
        &response.items,
        MODELS,
        model_row,
        empty_state(
            "No session has ended in that span.",
            Some("ariadne session ls"),
        ),
    )
}

/// A row of `stats models`, in the order [`MODELS`] declares its columns.
fn model_row(row: &ModelStatDto) -> Vec<String> {
    vec![
        row.model.clone(),
        dash(row.seat.as_deref()),
        row.sessions.to_string(),
        row.failed.to_string(),
        row.stalled.to_string(),
        usage_cell(&row.usage),
        duration(row.mean_lifetime_secs.round() as u64),
        match row.skills.is_empty() {
            true => "-".into(),
            false => row
                .skills
                .iter()
                .map(|skill| format!("{} {}", skill.name, skill.sessions))
                .collect::<Vec<_>>()
                .join(", "),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    use axum::extract::{RawQuery, State};
    use axum::routing::get;
    use axum::{Json, Router};

    use ariadne_api::stats::SkillCountDto;
    use ariadne_api::usage::TokenUsageDto;

    use crate::output::{View, render_table};

    fn stat() -> ModelStatDto {
        ModelStatDto {
            model: "stub:test-model".into(),
            seat: Some("author".into()),
            sessions: 4,
            failed: 1,
            stalled: 2,
            usage: TokenUsageDto {
                input_tokens: 1_200_000,
                cached_input_tokens: 1_069_200,
                output_tokens: 45_000,
            },
            cached_share: 0.891,
            mean_lifetime_secs: 260.0,
            skills: vec![
                SkillCountDto {
                    name: "coding".into(),
                    sessions: 4,
                },
                SkillCountDto {
                    name: "migration".into(),
                    sessions: 1,
                },
            ],
        }
    }

    /// A daemon that answers `GET /v1/stats/models` with [`stat`], and keeps
    /// the query string each call sent.
    async fn serve() -> (Client, Arc<Mutex<Vec<Option<String>>>>) {
        async fn handler(
            State(seen): State<Arc<Mutex<Vec<Option<String>>>>>,
            RawQuery(query): RawQuery,
        ) -> Json<ModelStatsResponse> {
            seen.lock().unwrap().push(query);
            Json(ModelStatsResponse {
                items: vec![stat()],
            })
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/stats/models", get(handler))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::tcp(format!("http://{address}")), seen)
    }

    /// `stats models --format json` reads the rows off the daemon with the
    /// filters it was given, and `stats` alone runs `models`.
    #[tokio::test]
    async fn stats_models_reads_the_rows_with_the_filters_given() {
        let (client, seen) = serve().await;
        let filters = Filters {
            since: Some("7d".into()),
            repo: None,
        };
        run(&client, Some(StatsCommand::Models), filters, Format::Json)
            .await
            .unwrap();
        let none = Filters {
            since: None,
            repo: None,
        };
        run(&client, None, none, Format::Table).await.unwrap();
        assert_eq!(*seen.lock().unwrap(), [Some("since=7d".to_string()), None]);
    }

    /// The table carries a header per column and the spend as a token cell.
    #[test]
    fn the_table_prints_headers_and_a_token_cell() {
        let table = render_table(MODELS, &[model_row(&stat())], &View::plain()).unwrap();
        let mut lines = table.lines();
        let headers: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
        assert_eq!(
            headers,
            [
                "MODEL", "SEAT", "SESSIONS", "FAILED", "STALLED", "TOKENS", "LIFETIME", "SKILLS"
            ]
        );
        let row = lines.next().unwrap();
        assert!(row.contains("↑1.2M 89.1% ↓45k"), "{row}");
        assert!(row.contains("4m 20s"), "{row}");
        assert!(row.contains("coding 4, migration 1"), "{row}");
    }
}
