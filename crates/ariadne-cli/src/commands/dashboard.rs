//! `ariadne dashboard ...`
use super::path_segment;
use crate::output::{Format, UNCAPPED, col, print_json, print_table};
use anyhow::Result;
use ariadne_api::dashboards::{DashboardDto, UpdateDashboardRequest};
use ariadne_client::Client;
use clap::Subcommand;
use std::io::Read;
use std::path::PathBuf;
#[derive(Subcommand)]
pub(crate) enum DashboardCommand {
    Ls,
    Get {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::dashboard_names))]
        name: String,
    },
    Set {
        name: String,
        file: PathBuf,
    },
    Reset {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::dashboard_names))]
        name: String,
    },
    Rm {
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::dashboard_names))]
        name: String,
    },
}
pub(crate) async fn run(client: &Client, cmd: DashboardCommand, format: Format) -> Result<()> {
    match cmd {
        DashboardCommand::Ls => {
            let d: Vec<DashboardDto> = client.get_json("/v1/dashboards").await?;
            if format == Format::Json {
                return print_json(&d);
            }
            let rows = d
                .iter()
                .map(|x| {
                    vec![
                        x.name.clone(),
                        x.builtin.to_string(),
                        x.document_is_default.to_string(),
                    ]
                })
                .collect::<Vec<_>>();
            print_table(
                &[
                    col("name", 24).title(),
                    col("builtin", UNCAPPED),
                    col("default", UNCAPPED),
                ],
                &rows,
            )?
        }
        DashboardCommand::Get { name } => {
            let d: DashboardDto = client.get_json(&path(&name)).await?;
            print_json(&d.document)?
        }
        DashboardCommand::Set { name, file } => {
            let text = if file == std::path::Path::new("-") {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                s
            } else {
                std::fs::read_to_string(file)?
            };
            let document = serde_json::from_str(&text)?;
            let _: DashboardDto = client
                .put_json(&path(&name), &UpdateDashboardRequest { document })
                .await?;
        }
        DashboardCommand::Reset { name } => {
            let _: DashboardDto = client.post_empty(&format!("{}/reset", path(&name))).await?;
        }
        DashboardCommand::Rm { name } => {
            client
                .send_no_content::<()>(http::Method::DELETE, &path(&name), None)
                .await?;
        }
    };
    let _ = format;
    Ok(())
}
fn path(name: &str) -> String {
    format!("/v1/dashboards/{}", path_segment(name))
}
