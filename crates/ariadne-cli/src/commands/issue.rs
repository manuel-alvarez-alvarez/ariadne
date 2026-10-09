//! `ariadne issue ls`.

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::issues::IssueDto;
use ariadne_client::Client;

use super::resolve::{self, Kind};
use crate::output::{Column, Format, UNCAPPED, col, print_list};

const COLUMNS: &[Column] = &[
    col("number", UNCAPPED).id(),
    col("title", 60).title(),
    col("labels", 24),
    col("assignees", 24),
    col("updated", UNCAPPED),
];

#[derive(Subcommand)]
pub(crate) enum IssueCommand {
    /// List issues assigned to your forge login
    Ls {
        /// Enabled repository id or registered path
        #[arg(long)]
        repo: String,
        /// List every open issue
        #[arg(long)]
        all: bool,
    },
}

pub(crate) async fn run(client: &Client, command: IssueCommand, format: Format) -> Result<()> {
    let IssueCommand::Ls { repo, all } = command;
    let id = resolve::id(client, Kind::Repo, &repo).await?;
    let assigned = if all { "all" } else { "me" };
    let issues: Vec<IssueDto> = client
        .get_json(&format!("/v1/repositories/{id}/issues?assigned={assigned}"))
        .await?;
    print_list(
        format,
        &issues,
        COLUMNS,
        |issue| {
            vec![
                issue.number.to_string(),
                issue.title.clone(),
                issue.labels.join(", "),
                issue.assignees.join(", "),
                issue.updated_at.clone(),
            ]
        },
        "No open issues",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn issue_ls_uses_the_repository_and_assignment_filter() {
        let paths = Arc::new(Mutex::new(Vec::<String>::new()));
        let called = paths.clone();
        let app = Router::new()
            .route(
                "/v1/repositories",
                get(|| async {
                    axum::Json(vec![crate::commands::fixtures::repository(
                        "repo-1",
                        "/work/widgets",
                        "main",
                    )])
                }),
            )
            .route(
                "/v1/repositories/repo-1/issues",
                get(move |uri: axum::http::Uri| {
                    let called = called.clone();
                    async move {
                        called.lock().unwrap().push(uri.to_string());
                        axum::Json(Vec::<IssueDto>::new())
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Client::tcp(format!("http://{address}"));
        run(
            &client,
            IssueCommand::Ls {
                repo: "repo-1".into(),
                all: false,
            },
            Format::Json,
        )
        .await
        .unwrap();
        run(
            &client,
            IssueCommand::Ls {
                repo: "repo-1".into(),
                all: true,
            },
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();
        assert_eq!(
            *paths.lock().unwrap(),
            [
                "/v1/repositories/repo-1/issues?assigned=me",
                "/v1/repositories/repo-1/issues?assigned=all",
            ]
        );
    }
}
