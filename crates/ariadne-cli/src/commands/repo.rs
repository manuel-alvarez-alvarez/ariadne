//! `ariadne repo ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::repositories::{CreateRepositoryRequest, RepositoryDto, UpdateRepositoryRequest};
use ariadne_client::Client;
use ariadne_core::MergeStrategy;
use serde_json::json;

use super::confirm;
use crate::output::{
    Column, Format, UNCAPPED, local_time, note, print_json, print_kv, print_table,
};

/// Columns of `repo ls`.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("path", 48),
    ("branch", 24),
    ("merge", UNCAPPED),
    ("description", 40),
];

#[derive(Subcommand)]
pub enum RepoCommand {
    /// Register a repository
    Add {
        /// Absolute path of the checkout
        path: String,
        /// Base branch tasks branch off (default: the repo's current branch)
        #[arg(long)]
        branch: Option<String>,
        /// What this repository is, in a line
        #[arg(long)]
        description: Option<String>,
        /// How an approved task lands on the base branch: squashed straight
        /// onto it, or published as a pull/merge request for a human to merge
        #[arg(long, value_enum, default_value_t = MergeStrategy::Direct)]
        merge_strategy: MergeStrategy,
    },
    /// List repositories
    Ls {
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
    },
    /// Show a repository
    Inspect {
        /// Repository id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        id: String,
    },
    /// Update a repository
    Edit {
        /// Repository id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        id: String,
        /// New absolute path of the checkout
        #[arg(long)]
        path: Option<String>,
        /// New base branch
        #[arg(long)]
        branch: Option<String>,
        /// New description, or "" to clear it
        #[arg(long)]
        description: Option<String>,
        /// How an approved task lands on the base branch
        #[arg(long, value_enum)]
        merge_strategy: Option<MergeStrategy>,
    },
    /// Delete a repository
    Rm {
        /// Repository id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

pub async fn run(client: &Client, cmd: RepoCommand, format: Format) -> Result<()> {
    match cmd {
        RepoCommand::Add {
            path,
            branch,
            description,
            merge_strategy,
        } => {
            let repo: RepositoryDto = client
                .post_json(
                    "/v1/repositories",
                    &CreateRepositoryRequest {
                        path,
                        base_branch: branch,
                        description,
                        merge_strategy: Some(merge_strategy),
                    },
                )
                .await?;
            match format {
                Format::Json => print_json(&repo)?,
                Format::Table => println!("{}", repo.id),
            }
        }
        RepoCommand::Ls { no_trunc } => {
            let repos: Vec<RepositoryDto> = client.get_json("/v1/repositories").await?;
            match format {
                Format::Json => print_json(&repos)?,
                Format::Table => {
                    print_table(
                        LS,
                        &repos
                            .iter()
                            .map(|r| {
                                vec![
                                    r.id.clone(),
                                    r.path.clone(),
                                    r.base_branch.clone(),
                                    r.merge_strategy.as_str().into(),
                                    r.description.clone().unwrap_or_else(|| "-".into()),
                                ]
                            })
                            .collect::<Vec<_>>(),
                        no_trunc,
                    );
                    if repos.is_empty() {
                        note("no repositories yet — add one with: ariadne repo add <path>");
                    }
                }
            }
        }
        RepoCommand::Inspect { id } => {
            let r: RepositoryDto = client.get_json(&format!("/v1/repositories/{id}")).await?;
            match format {
                Format::Json => print_json(&r)?,
                Format::Table => print_kv(&[
                    ("id", r.id),
                    ("path", r.path),
                    ("branch", r.base_branch),
                    ("merge_strategy", r.merge_strategy.as_str().into()),
                    ("description", r.description.unwrap_or_else(|| "-".into())),
                    ("created", local_time(&r.created_at)),
                    ("updated", local_time(&r.updated_at)),
                ]),
            }
        }
        RepoCommand::Edit {
            id,
            path,
            branch,
            description,
            merge_strategy,
        } => {
            let r: RepositoryDto = client
                .put_json(
                    &format!("/v1/repositories/{id}"),
                    &UpdateRepositoryRequest {
                        path,
                        base_branch: branch,
                        description,
                        merge_strategy,
                    },
                )
                .await?;
            match format {
                Format::Json => print_json(&r)?,
                Format::Table => println!("{}", r.id),
            }
        }
        RepoCommand::Rm { id, yes } => {
            let r: RepositoryDto = client.get_json(&format!("/v1/repositories/{id}")).await?;
            confirm(&rm_question(&r), yes)?;
            client
                .send_no_content::<()>(
                    http::Method::DELETE,
                    &format!("/v1/repositories/{id}"),
                    None,
                )
                .await?;
            match format {
                // The repository is gone, so there is no DTO left to print:
                // what the caller asked about, and that it happened.
                Format::Json => print_json(&json!({"repository": id, "deleted": true}))?,
                Format::Table => println!("deleted {id}"),
            }
        }
    }
    Ok(())
}

/// What `repo rm` asks before it deletes: the checkout on disk is untouched,
/// so the question names the registration it is about to drop.
fn rm_question(r: &RepositoryDto) -> String {
    format!(
        "Delete the repository {} [{}] ({})?",
        r.path, r.base_branch, r.id
    )
}
