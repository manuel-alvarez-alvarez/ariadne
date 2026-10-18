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
    Agent,
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
        ModelSeat::Agent => "agent",
    });
    let mut groups = Vec::new();
    // A seatless session remains in JSON and uses the task columns in the table.
    for seat in [Some("orchestrator"), Some("agent"), None] {
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
        let mut headers = vec![
            "model",
            if seat == Some("orchestrator") {
                "goals"
            } else {
                "tasks"
            },
            "tokens",
            "time",
            "messages",
        ];
        if seat == Some("agent") {
            headers.push("finished");
        }
        let columns: Vec<_> = headers
            .into_iter()
            .map(|header| col(header, UNCAPPED))
            .collect();
        let rows: Vec<Vec<String>> = items
            .into_iter()
            .map(|r| {
                let mut cells = vec![
                    r.model.clone(),
                    if seat == Some("orchestrator") {
                        r.goals
                    } else {
                        r.tasks
                    }
                    .to_string(),
                    tokens(r.tokens),
                    duration(r.time_secs as u64),
                    r.messages.to_string(),
                ];
                if seat == Some("agent") {
                    cells.push(r.tasks_finished.to_string());
                }
                cells
            })
            .collect();
        groups.push(format!(
            "{}\n{}",
            heading(seat.unwrap_or("loose sessions"), view.color),
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
    use ariadne_api::stats::ModelStatDto;
    use clap::Parser;

    fn stats() -> ModelStatsDto {
        ModelStatsDto {
            items: vec![
                ModelStatDto {
                    model: "planner".into(),
                    seat: Some("orchestrator".into()),
                    tasks: 9,
                    goals: 3,
                    tokens: 2_500_000,
                    time_secs: 7_200.0,
                    messages: 12,
                    ..Default::default()
                },
                ModelStatDto {
                    model: "writer".into(),
                    seat: Some("agent".into()),
                    tasks: 4,
                    goals: 2,
                    tokens: 1_200,
                    time_secs: 1_800.0,
                    messages: 6,
                    tasks_finished: 3,
                },
                ModelStatDto {
                    model: "judge".into(),
                    seat: None,
                    tasks: 5,
                    goals: 2,
                    tokens: 800,
                    time_secs: 120.0,
                    messages: 7,
                    tasks_finished: 0,
                },
            ],
        }
    }

    /// The cells of a plain table line; a duration holds one space, a gap two.
    fn cells(line: &str) -> Vec<&str> {
        line.split("  ")
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect()
    }

    /// The heading, the header and the one row of a seat's table.
    fn table(seat: ModelSeat) -> (String, Vec<String>, Vec<String>) {
        let text = render(&stats(), Some(seat), &View::plain()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        let owned = |line: &str| cells(line).into_iter().map(str::to_owned).collect();
        (lines[0].trim().to_owned(), owned(lines[1]), owned(lines[2]))
    }

    #[test]
    fn each_seat_prints_its_own_columns_and_figures() {
        assert_eq!(
            table(ModelSeat::Agent),
            (
                "AGENT".into(),
                ["MODEL", "TASKS", "TOKENS", "TIME", "MESSAGES", "FINISHED"]
                    .map(String::from)
                    .to_vec(),
                ["writer", "4", "1.2k", "30m 0s", "6", "3"]
                    .map(String::from)
                    .to_vec(),
            )
        );
        assert_eq!(
            table(ModelSeat::Orchestrator),
            (
                "ORCHESTRATOR".into(),
                ["MODEL", "GOALS", "TOKENS", "TIME", "MESSAGES"]
                    .map(String::from)
                    .to_vec(),
                ["planner", "3", "2.5M", "2h 0m", "12"]
                    .map(String::from)
                    .to_vec(),
            )
        );
    }

    #[test]
    fn every_seat_prints_a_table_in_seat_order() {
        let text = render(&stats(), None, &View::plain()).unwrap();
        let headings: Vec<_> = text
            .lines()
            .filter(|l| ["ORCHESTRATOR", "AGENT", "LOOSE SESSIONS"].contains(&l.trim()))
            .collect();
        assert_eq!(
            headings,
            ["ORCHESTRATOR", "AGENT", "LOOSE SESSIONS"],
            "{text}"
        );
        let empty = render(&ModelStatsDto::default(), None, &View::plain()).unwrap();
        assert_eq!(empty, "No model ran in that span.");
    }

    /// The no-seat group titles itself `LOOSE SESSIONS`, not the seat's
    /// internal `None`: a session with no seat is a loose session, and the
    /// heading says so plainly.
    #[test]
    fn the_no_seat_group_titles_itself_loose_sessions() {
        let text = render(&stats(), None, &View::plain()).unwrap();
        assert!(text.contains("LOOSE SESSIONS"), "{text}");
        assert!(!text.contains("NONE"), "{text}");
    }

    #[test]
    fn the_seat_option_belongs_only_to_models_and_prints_one_table() {
        let cli =
            crate::cli::Cli::try_parse_from(["ariadne", "stats", "models", "--seat", "agent"])
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
            crate::cli::Cli::try_parse_from(["ariadne", "stats", "work", "--seat", "agent"])
                .is_err()
        );
        for gone in ["unknown", "author", "reviewer"] {
            assert!(
                crate::cli::Cli::try_parse_from(["ariadne", "stats", "models", "--seat", gone])
                    .is_err(),
                "{gone}"
            );
        }
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
                seat: Some(ModelSeat::Agent),
            },
        )
        .await
        .unwrap();
        let rendered = output(
            &expected,
            Format::Json,
            Some(ModelSeat::Agent),
            &View::plain(),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(
            serde_json::from_value::<ModelStatsDto>(value.clone()).unwrap(),
            expected
        );
        assert_eq!(
            value["items"][1],
            serde_json::json!({
                "model": "writer", "seat": "agent", "tasks": 4, "goals": 2, "tokens": 1200,
                "time_secs": 1800.0, "messages": 6, "tasks_finished": 3
            })
        );
        server.abort();
    }
}
