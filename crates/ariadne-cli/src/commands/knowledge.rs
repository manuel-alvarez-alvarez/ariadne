//! `ariadne knowledge ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::knowledge::{
    KnowledgeHitDto, KnowledgeOutlineEntryDto, KnowledgeOutlineQuery, KnowledgeSearchQuery,
    KnowledgeState, KnowledgeStatusDto,
};
use ariadne_client::Client;

use super::query_path;
use super::resolve::{self, Kind};
use crate::output::{
    Column, Format, Kv, UNCAPPED, col, empty_state, moment, ok_id_line, print, print_kv,
    print_list, short_id, view,
};

/// Columns of `knowledge search`: where the definition is, then what it is.
/// The location is what `-q` prints, and what an editor opens.
const SEARCH: &[Column] = &[
    col("location", UNCAPPED),
    col("kind", UNCAPPED).rank(2),
    col("title", 40).title(),
    col("signature", 80).rank(1),
    col("repo", UNCAPPED).id().rank(3),
];

/// Columns of `knowledge outline`: the path is the same on every row, so a
/// row is its line range and what sits there.
const OUTLINE: &[Column] = &[
    col("lines", UNCAPPED),
    col("kind", UNCAPPED).rank(2),
    col("title", 40).title(),
    col("signature", 80).rank(1),
];

#[derive(Subcommand)]
pub enum KnowledgeCommand {
    /// Show where a repository's index stands
    Status {
        /// Repository id or path
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
    },
    /// Drop a repository's index and build it again
    Reindex {
        /// Repository id or path
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
    },
    /// Find definitions by name
    ///
    /// Words, camelCase and snake_case parts all match, each as a prefix:
    /// `add_worktree`, `addWork` and `worktree` all find `add_worktree`.
    Search {
        /// The identifier to find
        query: String,
        /// Only this repository, by id or path (default: every repository)
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repository: Option<String>,
        /// The branch to read (default: the base branch of each repository)
        #[arg(long = "ref", value_name = "REF")]
        git_ref: Option<String>,
        /// Only this kind: function, method, class, module, interface,
        /// macro, constant, test, heading
        #[arg(long)]
        kind: Option<String>,
        /// Only paths that contain this text
        #[arg(long)]
        path: Option<String>,
        /// How many results at most (default 20, max 50)
        #[arg(long)]
        limit: Option<i64>,
    },
    /// List the definitions of one file with their line ranges
    Outline {
        /// Repository id or path
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
        /// The file, relative to the repository root
        path: String,
        /// The branch to read (default: the base branch)
        #[arg(long = "ref", value_name = "REF")]
        git_ref: Option<String>,
    },
}

pub async fn run(client: &Client, command: KnowledgeCommand, format: Format) -> Result<()> {
    match command {
        KnowledgeCommand::Status { repo } => {
            let repository_id = resolve::id(client, Kind::Repo, &repo).await?;
            let status: KnowledgeStatusDto = client.get_json(&status_path(&repository_id)).await?;
            print(format, &status, || print_status(&status))?;
        }
        KnowledgeCommand::Reindex { repo } => {
            let repository_id = resolve::id(client, Kind::Repo, &repo).await?;
            let status: KnowledgeStatusDto = client
                .post_empty(&format!("{}/reindex", status_path(&repository_id)))
                .await?;
            print(format, &status, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "reindexing", &repository_id)
                )
            })?;
        }
        KnowledgeCommand::Search {
            query,
            repository,
            git_ref,
            kind,
            path,
            limit,
        } => {
            let repository = match repository {
                Some(repo) => Some(resolve::id(client, Kind::Repo, &repo).await?),
                None => None,
            };
            let request = KnowledgeSearchQuery {
                q: query,
                repository,
                all: None,
                git_ref,
                kind,
                path,
                limit,
            };
            let hits: Vec<KnowledgeHitDto> = client
                .get_json(&query_path("/v1/knowledge/search", &request)?)
                .await?;
            print_list(
                format,
                &hits,
                SEARCH,
                |hit| {
                    vec![
                        format!("{}:{}", hit.path, hit.line),
                        hit.kind.clone(),
                        hit.name.clone(),
                        hit.signature.clone(),
                        hit.repository_id.clone(),
                    ]
                },
                empty_state(
                    "No definition matches.",
                    Some("ariadne knowledge status <repo>"),
                ),
            )?;
        }
        KnowledgeCommand::Outline {
            repo,
            path,
            git_ref,
        } => {
            let repository = resolve::id(client, Kind::Repo, &repo).await?;
            let request = KnowledgeOutlineQuery {
                repository,
                path,
                git_ref,
            };
            let entries: Vec<KnowledgeOutlineEntryDto> = client
                .get_json(&query_path("/v1/knowledge/outline", &request)?)
                .await?;
            print_list(
                format,
                &entries,
                OUTLINE,
                |entry| {
                    vec![
                        format!("{}-{}", entry.start_line, entry.end_line),
                        entry.kind.clone(),
                        entry.name.clone(),
                        entry.signature.clone(),
                    ]
                },
                empty_state("The file defines nothing the index reads.", None),
            )?;
        }
    }
    Ok(())
}

fn status_path(repository_id: &str) -> String {
    format!("/v1/repositories/{repository_id}/knowledge")
}

fn print_status(status: &KnowledgeStatusDto) {
    let state = match status.state {
        KnowledgeState::Idle => "idle",
        KnowledgeState::Indexing => "indexing",
        KnowledgeState::Failed => "failed",
        KnowledgeState::Disabled => "disabled",
    };
    let refs = match status.refs.is_empty() {
        true => "-".to_string(),
        false => status
            .refs
            .iter()
            .map(|r| {
                format!(
                    "{} @ {} · {} files · {} symbols · {}",
                    r.git_ref,
                    short_id(&r.commit),
                    r.files,
                    r.symbols,
                    moment(&r.indexed_at)
                )
            })
            .collect::<Vec<_>>()
            .join("; "),
    };
    let languages = match status.languages.is_empty() {
        true => "-".to_string(),
        false => status
            .languages
            .iter()
            .map(|l| format!("{} {}", l.language, l.files))
            .collect::<Vec<_>>()
            .join(" · "),
    };
    print_kv(&[
        ("repository", Kv::id(status.repository_id.clone())),
        ("state", Kv::status(state)),
        ("refs", refs.into()),
        ("files", status.files.to_string().into()),
        ("symbols", status.symbols.to_string().into()),
        ("languages", languages.into()),
        (
            "error",
            status.error.clone().unwrap_or_else(|| "-".into()).into(),
        ),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The subject column of a search row is `title`, like every other
    /// table's, and the location leads so `-q` prints `path:line`.
    #[test]
    fn a_search_row_leads_with_its_location_and_titles_the_symbol() {
        let table = crate::output::render_table(
            SEARCH,
            &[vec![
                "src/lib.rs:12".into(),
                "function".into(),
                "add".into(),
                "pub fn add()".into(),
                "01REPO".into(),
            ]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.starts_with("LOCATION"), "{table}");
        assert!(header.contains("TITLE"), "{table}");
        assert!(!header.contains("NAME"), "{table}");
    }
}
