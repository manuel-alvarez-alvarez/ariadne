//! `ariadne stats <family>` — how the tool and the models perform, read off
//! the daemon's stats ledger.

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::stats::{ModelStatDto, ModelStatsResponse, ReviewStatsDto, StatsQuery};
use ariadne_api::stats::{
    ModelStatDto, ModelStatsResponse, PermissionStatDto, StatsQuery, ToolModelStatDto, ToolStatDto,
    ToolStatsDto,
};
use ariadne_client::Client;

use super::query_path;
use super::resolve::{self, Kind};
use crate::output::{
    Column, Format, UNCAPPED, col, dash, duration, empty_state, print, print_list, print_table,
    usage_cell,
    Column, Format, UNCAPPED, col, dash, duration, empty_state, print_json, print_list,
    print_table, usage_cell,
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

#[derive(Subcommand)]
pub(crate) enum StatsCommand {
    /// How each model did in each seat: sessions, failures, stalls, tokens
    Models,
    /// How reviews and their messages flowed
    Reviews,
    /// How often each tool ran, how long it took, and how permissions answered.
    Tools,
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
        StatsCommand::Tools => tools(client, &query, format).await,
    }
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
}
