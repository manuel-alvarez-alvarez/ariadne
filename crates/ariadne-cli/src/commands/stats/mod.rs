//! `ariadne stats <family>` — what the work did, read off the daemon's stats
//! ledger. Each family answers one question from a file of its own; this
//! file registers the six and holds what they share.

mod attention;
mod models;
mod spend;
mod time;
mod tools;
mod work;

use anyhow::Result;
use clap::Subcommand;
use serde::Serialize;

use ariadne_api::stats::StatsQuery;
use ariadne_client::Client;

use super::resolve::{self, Kind};
use crate::output::{Format, note, print, style, view};

/// The six families, in the order the screen shows them.
#[derive(Subcommand)]
pub(crate) enum StatsCommand {
    /// What got done
    Work,
    /// How long it takes
    Time,
    /// What it spent
    Spend,
    /// Which model does the job
    Models(models::Args),
    /// How much it needed you
    Attention,
    /// What the agents do
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
    match cmd.unwrap_or(StatsCommand::Work) {
        StatsCommand::Work => work::run(client, &query, format).await,
        StatsCommand::Time => time::run(client, &query, format).await,
        StatsCommand::Spend => spend::run(client, &query, format).await,
        StatsCommand::Models(args) => models::run(client, &query, format, args).await,
        StatsCommand::Attention => attention::run(client, &query, format).await,
        StatsCommand::Tools => tools::run(client, &query, format).await,
    }
}

/// Print a family: the DTO whole with `--format json`, and for a person,
/// one muted sentence while the family holds nothing to show.
fn print_family<T: Serialize>(format: Format, stats: &T, empty: &str) -> Result<()> {
    print(format, stats, || {
        note(&style::paint(view().color, style::META, empty));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    use axum::Router;
    use axum::extract::State;
    use axum::http::Uri;
    use axum::routing::get;

    /// Every path and query a call sent, in order.
    type Seen = Arc<Mutex<Vec<String>>>;

    /// A daemon that answers every stats route, and keeps the path and query
    /// of each call.
    async fn serve() -> (Client, Seen) {
        async fn handler(State(seen): State<Seen>, uri: Uri) -> axum::Json<serde_json::Value> {
            seen.lock().unwrap().push(uri.to_string());
            let body = if uri.path() == "/v1/stats/time" {
                serde_json::json!({
                    "tasks": 0,
                    "lead_time": {"median_secs": 0, "p90_secs": 0, "mean_secs": 0},
                    "in_status": [],
                    "waiting_on_person": {"prompts": 0, "total_secs": 0, "median_secs": 0}
                })
            } else {
                serde_json::json!({})
            };
            axum::Json(body)
        }
        let seen = Seen::default();
        let app = Router::new()
            .route("/v1/stats/{family}", get(handler))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::tcp(format!("http://{address}")), seen)
    }

    /// Each family reads its own route with the filters given, and prints
    /// its DTO with `--format json`.
    #[tokio::test]
    async fn each_family_reads_its_route_with_the_filters_given() {
        let (client, seen) = serve().await;
        for family in [
            StatsCommand::Work,
            StatsCommand::Time,
            StatsCommand::Spend,
            StatsCommand::Models(models::Args::default()),
            StatsCommand::Attention,
            StatsCommand::Tools,
        ] {
            let filters = Filters {
                since: Some("7d".into()),
                repo: None,
            };
            run(&client, Some(family), filters, Format::Json)
                .await
                .unwrap();
        }
        assert_eq!(
            *seen.lock().unwrap(),
            [
                "/v1/stats/work?since=7d",
                "/v1/stats/time?since=7d",
                "/v1/stats/spend?since=7d",
                "/v1/stats/models?since=7d",
                "/v1/stats/attention?since=7d",
                "/v1/stats/tools?since=7d",
            ]
        );
    }

    /// `ariadne stats` alone runs `work`, and its table of an empty family
    /// prints without a row.
    #[tokio::test]
    async fn stats_alone_runs_work() {
        let (client, seen) = serve().await;
        let none = Filters {
            since: None,
            repo: None,
        };
        run(&client, None, none, Format::Table).await.unwrap();
        assert_eq!(*seen.lock().unwrap(), ["/v1/stats/work"]);
    }
}
