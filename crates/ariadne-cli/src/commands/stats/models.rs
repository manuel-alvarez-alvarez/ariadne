//! `ariadne stats models`: comparisons within each seat.

use anyhow::Result;
use clap::ValueEnum;

use ariadne_api::stats::{ModelStatsDto, StatsQuery};
use ariadne_client::Client;

use crate::commands::query_path;
use crate::output::table::heading;
use crate::output::{Format, UNCAPPED, View, col, duration, render_table, style, tokens, view};

#[derive(clap::Args, Default)]
pub(crate) struct Args {
    /// Show only this seat's table (default: all seats)
    #[arg(long, value_enum)]
    seat: Option<ModelSeat>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ModelSeat {
    Orchestrator,
    Author,
    Reviewer,
}

pub(super) async fn run(
    client: &Client,
    query: &StatsQuery,
    format: Format,
    args: Args,
) -> Result<()> {
    let stats: ModelStatsDto = client
        .get_json(&query_path("/v1/stats/models", query)?)
        .await?;
    println!("{}", output(&stats, format, args.seat, view())?);
    Ok(())
}

fn output(
    stats: &ModelStatsDto,
    format: Format,
    seat: Option<ModelSeat>,
    view: &View,
) -> Result<String> {
    match format {
        Format::Json => Ok(serde_json::to_string_pretty(stats)?),
        Format::Table => render(stats, seat, view),
    }
}

fn render(stats: &ModelStatsDto, seat: Option<ModelSeat>, view: &View) -> Result<String> {
    let selected = seat.map(|s| match s {
        ModelSeat::Orchestrator => "orchestrator",
        ModelSeat::Author => "author",
        ModelSeat::Reviewer => "reviewer",
    });
    let mut groups = Vec::new();
    let percent = |rate: f64| format!("{:.1}%", rate * 100.0);
    // A seatless session remains in JSON and uses the session columns in the table.
    for seat in [Some("orchestrator"), Some("author"), Some("reviewer"), None] {
        if selected.is_some() && selected != seat {
            continue;
        }
        let items: Vec<_> = stats
            .items
            .iter()
            .filter(|r| r.seat.as_deref() == seat)
            .collect();
        if items.is_empty() {
            continue;
        }
        let headers = match seat {
            Some("author") => vec![
                "model",
                "tasks",
                "finish_rate",
                "first_pass",
                "rounds",
                "win_rate",
                "tokens/task",
                "failed",
                "exhausted",
            ],
            Some("reviewer") => vec![
                "model",
                "verdicts",
                "approve",
                "latency",
                "failed",
                "exhausted",
            ],
            _ => vec![
                "model",
                "sessions",
                "tokens",
                "lifetime",
                "failed",
                "exhausted",
            ],
        };
        let columns: Vec<_> = headers
            .into_iter()
            .map(|header| col(header, UNCAPPED))
            .collect();
        let rows: Vec<Vec<String>> = items
            .into_iter()
            .map(|r| {
                let mut cells = vec![r.model.clone()];
                match seat {
                    Some("author") => {
                        let a = r.author.clone().unwrap_or_default();
                        cells.extend([
                            (a.tasks_finished + a.tasks_failed + a.tasks_cancelled).to_string(),
                            percent(a.finish_rate),
                            percent(a.first_pass_rate),
                            format!("{:.1}", a.mean_review_rounds),
                            percent(a.win_rate),
                            tokens(a.tokens_per_finished_task.round() as u64),
                        ]);
                    }
                    Some("reviewer") => {
                        let review = r.reviewer.clone().unwrap_or_default();
                        cells.extend([
                            review.verdicts.to_string(),
                            percent(review.approve_share),
                            duration(review.mean_latency_secs as u64),
                        ]);
                    }
                    _ => cells.extend([
                        r.sessions.to_string(),
                        tokens(r.usage.input_tokens.saturating_add(r.usage.output_tokens)),
                        duration(r.mean_lifetime_secs as u64),
                    ]),
                }
                cells.extend([r.failed_sessions.to_string(), r.exhaustions.to_string()]);
                cells
            })
            .collect();
        groups.push(format!(
            "{}\n{}",
            heading(seat.unwrap_or("none"), view.color),
            render_table(&columns, &rows, view)?
        ));
    }
    if groups.is_empty() {
        return Ok(style::paint(
            view.color,
            style::META,
            "No model ran in that span.",
        ));
    }
    Ok(groups.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariadne_api::stats::{AuthorModelStatDto, ModelStatDto, ReviewerModelStatDto};
    use clap::Parser;

    fn stats() -> ModelStatsDto {
        ModelStatsDto {
            items: vec![
                ModelStatDto {
                    model: "planner".into(),
                    seat: Some("orchestrator".into()),
                    sessions: 3,
                    ..Default::default()
                },
                ModelStatDto {
                    model: "writer".into(),
                    seat: Some("author".into()),
                    author: Some(AuthorModelStatDto {
                        tasks_finished: 2,
                        tasks_failed: 1,
                        tasks_cancelled: 1,
                        finish_rate: 0.5,
                        first_pass_rate: 0.75,
                        mean_review_rounds: 1.5,
                        win_rate: 0.25,
                        tokens_per_finished_task: 1200.0,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ModelStatDto {
                    model: "judge".into(),
                    seat: Some("reviewer".into()),
                    reviewer: Some(ReviewerModelStatDto {
                        verdicts: 8,
                        approve_share: 0.625,
                        mean_latency_secs: 120.0,
                    }),
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn tables_group_models_by_seat_and_format_the_figures() {
        let text = render(&stats(), None, &View::plain()).unwrap();
        for heading in ["AUTHOR", "REVIEWER", "ORCHESTRATOR"] {
            assert!(text.contains(heading), "{text}");
        }
        for figure in [
            "writer", "judge", "planner", "50.0%", "75.0%", "25.0%", "62.5%", "1.2k", "2m",
        ] {
            assert!(text.contains(figure), "{figure}: {text}");
        }
        for header in [
            "MODEL",
            "TASKS",
            "FINISH_RATE",
            "FIRST_PASS",
            "ROUNDS",
            "WIN_RATE",
            "TOKENS/TASK",
            "FAILED",
            "EXHAUSTED",
            "VERDICTS",
            "APPROVE",
            "LATENCY",
            "SESSIONS",
            "TOKENS",
            "LIFETIME",
        ] {
            assert!(text.contains(header), "{header}: {text}");
        }
    }

    #[test]
    fn the_seat_option_belongs_only_to_models_and_prints_one_table() {
        let cli =
            crate::cli::Cli::try_parse_from(["ariadne", "stats", "models", "--seat", "author"])
                .unwrap();
        let crate::cli::Command::Stats {
            command: Some(super::super::StatsCommand::Models(args)),
            ..
        } = cli.command
        else {
            panic!("models command")
        };
        let text = render(&stats(), args.seat, &View::plain()).unwrap();
        assert!(text.contains("writer"));
        assert!(!text.contains("judge"));
        assert!(!text.contains("planner"));
        assert!(
            crate::cli::Cli::try_parse_from(["ariadne", "stats", "work", "--seat", "author"])
                .is_err()
        );
        assert!(
            crate::cli::Cli::try_parse_from(["ariadne", "stats", "models", "--seat", "unknown"])
                .is_err()
        );
    }

    #[tokio::test]
    async fn json_reads_the_complete_dto_even_with_a_seat_selected() {
        use axum::{Router, routing::get};
        let payload = stats();
        let expected = payload.clone();
        let app = Router::new().route(
            "/v1/stats/models",
            get(move || async move { axum::Json(payload) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Client::tcp(format!("http://{address}"));
        run(
            &client,
            &StatsQuery::default(),
            Format::Json,
            Args {
                seat: Some(ModelSeat::Author),
            },
        )
        .await
        .unwrap();
        let rendered = output(
            &expected,
            Format::Json,
            Some(ModelSeat::Author),
            &View::plain(),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(
            serde_json::from_value::<ModelStatsDto>(value.clone()).unwrap(),
            expected
        );
        assert_eq!(value["items"].as_array().unwrap().len(), 3);
        assert_eq!(value["items"][1]["author"]["first_pass_rate"], 0.75);
        server.abort();
    }
}
