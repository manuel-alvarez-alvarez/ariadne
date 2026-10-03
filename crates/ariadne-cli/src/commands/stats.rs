//! `ariadne stats <family>` — how the tool and the models perform, read off
//! the daemon's stats ledger.

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::stats::{
    ModelStatDto, ModelStatsResponse, OutcomeStatDto, OutcomeStatsDto, OutcomeTotalsDto,
    PermissionStatDto, ReviewStatsDto, StatsQuery, SwitchStatDto, SwitchStatsResponse,
    ToolModelStatDto, ToolStatDto, ToolStatsDto,
};
use ariadne_client::Client;

use super::query_path;
use super::resolve::{self, Kind};
use crate::output::{
    Column, Format, UNCAPPED, col, dash, duration, empty_state, note, pct, print, print_json,
    print_list, print_table, usage_cell,
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
const REVIEW_AUTHORS: &[Column] = &[
    col("model", UNCAPPED),
    col("approvals", UNCAPPED),
    col("mean rounds", UNCAPPED),
    col("median rounds", UNCAPPED),
    col("first pass", UNCAPPED),
];
const REVIEWERS: &[Column] = &[
    col("model", UNCAPPED),
    col("verdicts", UNCAPPED),
    col("approve share", UNCAPPED),
    col("mean latency", UNCAPPED),
];
const REVIEW_MESSAGES: &[Column] = &[
    col("kind", UNCAPPED),
    col("from", UNCAPPED),
    col("total", UNCAPPED),
    col("mean per task", UNCAPPED),
];

const TOOLS: &[Column] = &[
    col("tool", UNCAPPED).title(),
    col("calls", UNCAPPED),
    col("errors", UNCAPPED),
    col("median", UNCAPPED),
    col("p90", UNCAPPED),
];
const TOOL_MODELS: &[Column] = &[
    col("model", UNCAPPED).title(),
    col("calls", UNCAPPED),
    col("mean", UNCAPPED),
];
const PERMISSIONS: &[Column] = &[
    col("decided by", UNCAPPED).title(),
    col("answer", UNCAPPED),
    col("total", UNCAPPED),
    col("mean wait", UNCAPPED),
];

/// Columns of `stats switches`. The model never drops; the rest rank by how
/// much they explain where the switches went.
const SWITCHES: &[Column] = &[
    col("model", UNCAPPED).title(),
    col("switches", UNCAPPED),
    col("exhausted", UNCAPPED),
    col("automatic", UNCAPPED),
    col("arrivals", UNCAPPED),
];

/// Columns of `stats outcomes`. The model and its endings never drop; the
/// contest figures are the first to go on a narrow terminal, since most tasks
/// run with one author and never enter one.
const OUTCOMES: &[Column] = &[
    col("model", UNCAPPED).title(),
    col("finished", UNCAPPED),
    col("failed", UNCAPPED),
    col("cancelled", UNCAPPED),
    col("finish_rate", UNCAPPED).rank(3),
    col("lead_time", UNCAPPED).rank(2),
    col("reviews", UNCAPPED).rank(2),
    col("contests", UNCAPPED).rank(1),
    col("win_rate", UNCAPPED).rank(1),
];

#[derive(Subcommand)]
pub(crate) enum StatsCommand {
    /// How each model did in each seat: sessions, failures, stalls, tokens
    Models,
    /// How reviews and their messages flowed
    Reviews,
    /// How often each tool ran, how long it took, and how permissions answered.
    Tools,
    /// How often a session left each model, and why
    Switches,
    /// How tasks end per author model: finish rate, lead time, and the
    /// contests several authors ran against each other
    Outcomes,
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
        StatsCommand::Reviews => reviews(client, &query, format).await,
        StatsCommand::Tools => tools(client, &query, format).await,
        StatsCommand::Switches => switches(client, &query, format).await,
        StatsCommand::Outcomes => outcomes(client, &query, format).await,
    }
}

async fn reviews(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let response: ReviewStatsDto = client
        .get_json(&query_path("/v1/stats/reviews", query)?)
        .await?;
    print(format, &response, || {
        let authors = response
            .authors
            .iter()
            .map(|r| {
                vec![
                    r.model.clone(),
                    r.approvals.to_string(),
                    format!("{:.1}", r.mean_rounds),
                    format!("{:.1}", r.median_rounds),
                    format!("{:.1}%", r.first_pass_rate * 100.0),
                ]
            })
            .collect::<Vec<_>>();
        let reviewers = response
            .reviewers
            .iter()
            .map(|r| {
                vec![
                    r.model.clone(),
                    r.verdicts.to_string(),
                    format!("{:.1}%", r.approve_share * 100.0),
                    duration(r.mean_latency_secs.round() as u64),
                ]
            })
            .collect::<Vec<_>>();
        let messages = response
            .messages
            .iter()
            .map(|r| {
                vec![
                    r.kind.clone(),
                    r.from_actor.clone(),
                    r.total.to_string(),
                    format!("{:.1}", r.mean_per_task),
                ]
            })
            .collect::<Vec<_>>();
        println!("Authors");
        print_table(REVIEW_AUTHORS, &authors).expect("print authors");
        println!("Reviewers");
        print_table(REVIEWERS, &reviewers).expect("print reviewers");
        println!("Messages");
        print_table(REVIEW_MESSAGES, &messages).expect("print messages");
    })
}

async fn tools(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let response: ToolStatsDto = client
        .get_json(&query_path("/v1/stats/tools", query)?)
        .await?;
    if format == Format::Json {
        return print_json(&response);
    }
    println!("Tools");
    print_table(
        TOOLS,
        &response.tools.iter().map(tool_row).collect::<Vec<_>>(),
    )?;
    println!("\nModels");
    print_table(
        TOOL_MODELS,
        &response
            .models
            .iter()
            .map(tool_model_row)
            .collect::<Vec<_>>(),
    )?;
    println!("\nPermissions");
    print_table(
        PERMISSIONS,
        &response
            .permissions
            .iter()
            .map(permission_row)
            .collect::<Vec<_>>(),
    )
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

fn milliseconds(value: f64) -> String {
    format!("{}ms", value.round())
}

fn tool_row(row: &ToolStatDto) -> Vec<String> {
    vec![
        row.tool_name.clone(),
        row.calls.to_string(),
        row.errors.to_string(),
        milliseconds(row.median_duration_ms),
        milliseconds(row.p90_duration_ms),
    ]
}

fn tool_model_row(row: &ToolModelStatDto) -> Vec<String> {
    vec![
        row.model.clone(),
        row.calls.to_string(),
        milliseconds(row.mean_duration_ms),
    ]
}

fn permission_row(row: &PermissionStatDto) -> Vec<String> {
    vec![
        row.decided_by.clone(),
        row.answer.clone(),
        row.permissions.to_string(),
        milliseconds(row.mean_wait_ms),
    ]
}

async fn switches(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let response: SwitchStatsResponse = client
        .get_json(&query_path("/v1/stats/switches", query)?)
        .await?;
    print_list(
        format,
        &response.items,
        SWITCHES,
        switch_row,
        empty_state(
            "No session has switched in that span.",
            Some("ariadne session ls"),
        ),
    )
}

/// A row of `stats switches`, in the order [`SWITCHES`] declares its columns.
fn switch_row(row: &SwitchStatDto) -> Vec<String> {
    vec![
        row.model.clone(),
        row.switches.to_string(),
        row.exhaustions.to_string(),
        format!("{:.1}%", row.automatic_share * 100.0),
        row.arrivals.to_string(),
    ]
}

async fn outcomes(client: &Client, query: &StatsQuery, format: Format) -> Result<()> {
    let response: OutcomeStatsDto = client
        .get_json(&query_path("/v1/stats/outcomes", query)?)
        .await?;
    print(format, &response, || {
        let mut rows: Vec<Vec<String>> = response.items.iter().map(outcome_row).collect();
        rows.push(totals_row(&response.totals));
        let _ = crate::output::print_table(OUTCOMES, &rows);
        if response.items.is_empty() {
            note(&empty_state(
                "No task has ended in that span.",
                Some("ariadne task ls"),
            ));
        }
    })
}

/// The mean and the median of a lead time, as one cell: `6m (5m)`.
fn lead_time_cell(mean_secs: f64, median_secs: f64) -> String {
    format!(
        "{} ({})",
        duration(mean_secs.round() as u64),
        duration(median_secs.round() as u64)
    )
}

/// A row of `stats outcomes`, in the order [`OUTCOMES`] declares its columns.
fn outcome_row(row: &OutcomeStatDto) -> Vec<String> {
    vec![
        row.model.clone(),
        row.finished.to_string(),
        row.failed.to_string(),
        row.cancelled.to_string(),
        pct(row.finish_rate),
        lead_time_cell(row.mean_lead_time_secs, row.median_lead_time_secs),
        format!("{:.1}", row.mean_review_requests),
        format!("{}/{}", row.contests_won, row.contests_entered),
        pct(row.win_rate),
    ]
}

/// The totals row of `stats outcomes`, in the same shape as [`outcome_row`].
fn totals_row(totals: &OutcomeTotalsDto) -> Vec<String> {
    vec![
        "Totals".into(),
        totals.finished.to_string(),
        totals.failed.to_string(),
        totals.cancelled.to_string(),
        pct(totals.finish_rate),
        lead_time_cell(totals.mean_lead_time_secs, totals.median_lead_time_secs),
        format!("{:.1}", totals.mean_review_requests),
        format!("{}/{}", totals.contests_won, totals.contests_entered),
        pct(totals.win_rate),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    use axum::extract::{RawQuery, State};
    use axum::routing::get;
    use axum::{Json, Router};

    use ariadne_api::stats::{SkillCountDto, SwitchReasonCountDto};
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

    fn tool_stats() -> ToolStatsDto {
        ToolStatsDto {
            tools: vec![ToolStatDto {
                tool_name: "Bash".into(),
                calls: 4,
                errors: 1,
                median_duration_ms: 25.0,
                p90_duration_ms: 100.0,
            }],
            models: vec![ToolModelStatDto {
                model: "stub:test-model".into(),
                calls: 4,
                mean_duration_ms: 40.0,
            }],
            permissions: vec![PermissionStatDto {
                decided_by: "console".into(),
                answer: "allow".into(),
                permissions: 1,
                mean_wait_ms: 20.0,
            }],
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

    #[test]
    fn review_tables_print_the_three_groups_and_rows() {
        let view = View::plain();
        let authors = render_table(
            REVIEW_AUTHORS,
            &[vec![
                "stub:author".into(),
                "1".into(),
                "2.0".into(),
                "2.0".into(),
                "0.0%".into(),
            ]],
            &view,
        )
        .unwrap();
        let reviewers = render_table(
            REVIEWERS,
            &[vec![
                "stub:reviewer".into(),
                "2".into(),
                "50.0%".into(),
                "4s".into(),
            ]],
            &view,
        )
        .unwrap();
        let messages = render_table(
            REVIEW_MESSAGES,
            &[vec![
                "approve".into(),
                "reviewer".into(),
                "1".into(),
                "1.0".into(),
            ]],
            &view,
        )
        .unwrap();
        let printed = format!("Authors\n{authors}\nReviewers\n{reviewers}\nMessages\n{messages}");
        for value in [
            "Authors",
            "stub:author",
            "Reviewers",
            "stub:reviewer",
            "Messages",
            "approve",
        ] {
            assert!(printed.contains(value), "{printed}");
        }
    }

    /// `stats tools` reads its DTO as JSON, and its three tables name the
    /// tool, model, and permission groups.
    #[tokio::test]
    async fn tools_json_prints_the_dto_and_the_table_groups_its_rows() {
        async fn handler() -> Json<ToolStatsDto> {
            Json(tool_stats())
        }
        let app = Router::new().route("/v1/stats/tools", get(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Client::tcp(format!("http://{address}"));
        run(
            &client,
            Some(StatsCommand::Tools),
            Filters {
                since: None,
                repo: None,
            },
            Format::Json,
        )
        .await
        .unwrap();
        let tool =
            render_table(TOOLS, &[tool_row(&tool_stats().tools[0])], &View::plain()).unwrap();
        let model = render_table(
            TOOL_MODELS,
            &[tool_model_row(&tool_stats().models[0])],
            &View::plain(),
        )
        .unwrap();
        let permission = render_table(
            PERMISSIONS,
            &[permission_row(&tool_stats().permissions[0])],
            &View::plain(),
        )
        .unwrap();
        assert!(tool.contains("TOOL") && tool.contains("Bash"));
        assert!(model.contains("MODEL") && model.contains("stub:test-model"));
        assert!(permission.contains("DECIDED BY") && permission.contains("console"));
    }

    fn switch_stat() -> SwitchStatDto {
        SwitchStatDto {
            model: "stub:a".into(),
            switches: 3,
            by_reason: vec![SwitchReasonCountDto {
                reason: "exhausted".into(),
                switches: 2,
            }],
            exhaustions: 2,
            automatic_share: 2.0 / 3.0,
            arrivals: 1,
        }
    }

    /// A daemon that answers `GET /v1/stats/switches` with [`switch_stat`],
    /// and keeps the query string each call sent.
    async fn serve_switches() -> (Client, Arc<Mutex<Vec<Option<String>>>>) {
        async fn handler(
            State(seen): State<Arc<Mutex<Vec<Option<String>>>>>,
            RawQuery(query): RawQuery,
        ) -> Json<SwitchStatsResponse> {
            seen.lock().unwrap().push(query);
            Json(SwitchStatsResponse {
                items: vec![switch_stat()],
                switches: 3,
                exhaustions: 2,
            })
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/stats/switches", get(handler))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::tcp(format!("http://{address}")), seen)
    }

    /// `stats switches --format json` prints the rows the daemon answered,
    /// with the filters it was given.
    #[tokio::test]
    async fn stats_switches_reads_the_rows_with_the_filters_given() {
        let (client, seen) = serve_switches().await;
        let filters = Filters {
            since: Some("7d".into()),
            repo: None,
        };
        run(&client, Some(StatsCommand::Switches), filters, Format::Json)
            .await
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), [Some("since=7d".to_string())]);
    }

    /// The table carries a header and a row per model: switches out,
    /// exhausted, automatic and arrivals.
    #[test]
    fn the_switches_table_prints_a_row_per_model() {
        let table = render_table(SWITCHES, &[switch_row(&switch_stat())], &View::plain()).unwrap();
        let mut lines = table.lines();
        let headers: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
        assert_eq!(
            headers,
            ["MODEL", "SWITCHES", "EXHAUSTED", "AUTOMATIC", "ARRIVALS"]
        );
        let row = lines.next().unwrap();
        assert!(row.contains("stub:a"), "{row}");
        assert!(row.contains('3'), "{row}");
        assert!(row.contains("66.7%"), "{row}");
        assert!(row.contains('1'), "{row}");
    }

    fn outcome_stat() -> OutcomeStatDto {
        OutcomeStatDto {
            model: "stub:test-model".into(),
            finished: 7,
            failed: 2,
            cancelled: 1,
            finish_rate: 0.7,
            median_lead_time_secs: 310.0,
            mean_lead_time_secs: 400.0,
            mean_review_requests: 2.5,
            contests_entered: 3,
            contests_won: 2,
            win_rate: 2.0 / 3.0,
        }
    }

    fn outcome_totals() -> OutcomeTotalsDto {
        OutcomeTotalsDto {
            finished: 7,
            failed: 2,
            cancelled: 1,
            finish_rate: 0.7,
            median_lead_time_secs: 310.0,
            mean_lead_time_secs: 400.0,
            mean_review_requests: 2.5,
            contests_entered: 3,
            contests_won: 2,
            win_rate: 2.0 / 3.0,
        }
    }

    /// A daemon that answers `GET /v1/stats/outcomes` with [`outcome_stat`]
    /// and [`outcome_totals`], and keeps the query string each call sent.
    async fn serve_outcomes() -> (Client, Arc<Mutex<Vec<Option<String>>>>) {
        async fn handler(
            State(seen): State<Arc<Mutex<Vec<Option<String>>>>>,
            RawQuery(query): RawQuery,
        ) -> Json<OutcomeStatsDto> {
            seen.lock().unwrap().push(query);
            Json(OutcomeStatsDto {
                items: vec![outcome_stat()],
                totals: outcome_totals(),
            })
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/stats/outcomes", get(handler))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::tcp(format!("http://{address}")), seen)
    }

    /// `stats outcomes --format json` reads the daemon's DTO whole, with the
    /// filters it was given.
    #[tokio::test]
    async fn stats_outcomes_format_json_reads_the_dto_with_the_filters_given() {
        let (client, seen) = serve_outcomes().await;
        let filters = Filters {
            since: Some("30d".into()),
            repo: None,
        };
        run(&client, Some(StatsCommand::Outcomes), filters, Format::Json)
            .await
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), [Some("since=30d".to_string())]);
    }

    /// The table carries a header per column, a row per model and a totals
    /// row after it.
    #[test]
    fn the_table_prints_headers_a_model_row_and_a_totals_row() {
        let rows = [outcome_row(&outcome_stat()), totals_row(&outcome_totals())];
        let table = render_table(OUTCOMES, &rows, &View::plain()).unwrap();
        let mut lines = table.lines();
        let headers: Vec<&str> = lines.next().unwrap().split_whitespace().collect();
        assert_eq!(
            headers,
            [
                "MODEL",
                "FINISHED",
                "FAILED",
                "CANCELLED",
                "FINISH_RATE",
                "LEAD_TIME",
                "REVIEWS",
                "CONTESTS",
                "WIN_RATE"
            ]
        );
        let model_row = lines.next().unwrap();
        assert!(model_row.contains("stub:test-model"), "{model_row}");
        assert!(model_row.contains("70.0%"), "{model_row}");
        assert!(model_row.contains("6m 40s (5m 10s)"), "{model_row}");
        assert!(model_row.contains("2/3"), "{model_row}");
        let totals = lines.next().unwrap();
        assert!(totals.contains("Totals"), "{totals}");
        assert!(totals.contains("66.7%"), "{totals}");
        assert!(
            lines.next().is_none(),
            "exactly one model row and one totals row"
        );
    }
}
