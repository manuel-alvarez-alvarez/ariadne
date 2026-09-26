//! `ariadne repo ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::repositories::{CreateRepositoryRequest, RepositoryDto, UpdateRepositoryRequest};
use ariadne_client::Client;
use ariadne_core::PermissionMode;
use serde_json::json;

use super::resolve::{self, Kind};
use super::{Subject, confirm};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, Kv, UNCAPPED, age, col, empty_state, moment, ok_id_line, print, print_kv,
    print_list, view,
};

/// Columns of `repo ls`. The path is what a repository is, so it stays
/// whatever the terminal's width; the description is the first thing to go.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 48).title(),
    col("age", UNCAPPED).rank(4),
    col("branch", 24).rank(3),
    col("permissions", UNCAPPED).rank(2),
    col("description", 40).rank(1),
];

#[derive(Subcommand)]
pub(crate) enum RepoCommand {
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
        /// How its agents' ACP permission requests are answered: auto
        /// approves, ask waits for a console answer, learn remembers
        /// approvals, ai lets the model decide (default: auto)
        #[arg(long, value_parser = Spelling::<PermissionMode>::new())]
        permission_mode: Option<PermissionMode>,
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
        /// New permission mode: auto, ask, learn, or ai (the AI permission model decides)
        #[arg(long, value_parser = Spelling::<PermissionMode>::new())]
        permission_mode: Option<PermissionMode>,
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

pub(crate) async fn run(client: &Client, cmd: RepoCommand, format: Format) -> Result<()> {
    match cmd {
        RepoCommand::Add {
            path,
            branch,
            description,
            permission_mode,
        } => {
            let repo: RepositoryDto = client
                .post_json(
                    "/v1/repositories",
                    &CreateRepositoryRequest {
                        path,
                        base_branch: branch,
                        description,
                        permission_mode,
                    },
                )
                .await?;
            print(format, &repo, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "created", &repo.id)
                )
            })?;
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
                        r.permission_mode.as_str().into(),
                        r.description.clone().unwrap_or_else(|| "-".into()),
                    ]
                },
                empty_state("No repositories yet.", Some("ariadne repo add <path>")),
            )?;
        }
        RepoCommand::Inspect { id } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client.get_json(&repo_path(&id)).await?;
            print(format, &r, || {
                print_kv(&[
                    ("id", Kv::id(r.id.clone())),
                    ("path", r.path.clone().into()),
                    ("branch", r.base_branch.clone().into()),
                    ("permissions", r.permission_mode.as_str().into()),
                    (
                        "description",
                        r.description.clone().unwrap_or_else(|| "-".into()).into(),
                    ),
                    ("created", Kv::meta(moment(&r.created_at))),
                    ("updated", Kv::meta(moment(&r.updated_at))),
                ]);
            })?;
        }
        RepoCommand::Update {
            id,
            path,
            branch,
            description,
            permission_mode,
        } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client
                .put_json(
                    &repo_path(&id),
                    &UpdateRepositoryRequest {
                        path,
                        base_branch: branch,
                        description,
                        permission_mode,
                    },
                )
                .await?;
            print(format, &r, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "updated", &r.id)
                )
            })?;
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
                println!("{}", ok_id_line(view().color, view().quiet, "deleted", &id))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repository_subject_column_is_title() {
        let table = crate::output::render_table(
            LS,
            &[vec![String::new(); LS.len()]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.contains("TITLE"), "{table}");
        assert!(!header.contains("PATH"), "{table}");
    }
}
