//! `ariadne repo ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::repositories::{CreateRepositoryRequest, RepositoryDto, UpdateRepositoryRequest};
use ariadne_client::Client;
use ariadne_core::MergeStrategy;
use serde_json::json;

use super::resolve::{self, Kind};
use super::{Subject, confirm};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, UNCAPPED, age, col, moment, ok_id_line, print, print_kv, print_list, view,
};

/// Columns of `repo ls`. The path is what a repository is, so it stays
/// whatever the terminal's width; the description is the first thing to go.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("path", 48).title(),
    col("age", UNCAPPED).rank(4),
    col("branch", 24).rank(3),
    col("merge", UNCAPPED).rank(2),
    col("description", 40).rank(1),
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
        #[arg(long, value_parser = Spelling::<MergeStrategy>::new(), default_value = "direct")]
        merge_strategy: MergeStrategy,
    },
    /// List repositories
    Ls,
    /// Show a repository
    Inspect {
        /// Repository id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        id: String,
    },
    /// Update a repository
    Update {
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
        #[arg(long, value_parser = Spelling::<MergeStrategy>::new())]
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
            print(format, &repo, || println!("{}", repo.id))?;
        }
        RepoCommand::Ls => {
            let repos: Vec<RepositoryDto> = client.get_json("/v1/repositories").await?;
            let now = chrono::Utc::now();
            print_list(
                format,
                &repos,
                LS,
                |r| {
                    vec![
                        r.id.clone(),
                        r.path.clone(),
                        age(&r.created_at, now),
                        r.base_branch.clone(),
                        r.merge_strategy.as_str().into(),
                        r.description.clone().unwrap_or_else(|| "-".into()),
                    ]
                },
                "no repositories yet — add one with: ariadne repo add <path>",
            )?;
        }
        RepoCommand::Inspect { id } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client.get_json(&repo_path(&id)).await?;
            print(format, &r, || {
                print_kv(&[
                    ("id", r.id.clone()),
                    ("path", r.path.clone()),
                    ("branch", r.base_branch.clone()),
                    ("merge_strategy", r.merge_strategy.as_str().into()),
                    (
                        "description",
                        r.description.clone().unwrap_or_else(|| "-".into()),
                    ),
                    ("created", moment(&r.created_at)),
                    ("updated", moment(&r.updated_at)),
                ])
            })?;
        }
        RepoCommand::Update {
            id,
            path,
            branch,
            description,
            merge_strategy,
        } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client
                .put_json(
                    &repo_path(&id),
                    &UpdateRepositoryRequest {
                        path,
                        base_branch: branch,
                        description,
                        merge_strategy,
                    },
                )
                .await?;
            print(format, &r, || println!("{}", r.id))?;
        }
        RepoCommand::Rm { id, yes } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client.get_json(&repo_path(&id)).await?;
            let subject = Subject::new("repository", &r.path, &r.id);
            confirm("delete", &subject, &rm_question(&r, &subject), yes)?;
            client
                .send_no_content::<()>(http::Method::DELETE, &repo_path(&id), None)
                .await?;
            // The repository is gone, so there is no DTO left to print: what
            // the caller asked about, and that it happened.
            print(format, &json!({"repository": id, "deleted": true}), || {
                println!("{}", ok_id_line(view().color, "deleted", &id))
            })?;
        }
    }
    Ok(())
}

fn repo_path(id: &str) -> String {
    format!("/v1/repositories/{id}")
}

/// What `repo rm` asks before it deletes: the checkout on disk is untouched,
/// so the question names the registration it is about to drop.
fn rm_question(r: &RepositoryDto, subject: &Subject) -> String {
    format!(
        "Delete the repository {} on {}?",
        subject.named(),
        r.base_branch
    )
}
