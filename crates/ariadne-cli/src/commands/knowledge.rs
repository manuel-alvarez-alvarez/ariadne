//! `ariadne knowledge ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::knowledge::{
    KnowledgeDetail, KnowledgeHitDto, KnowledgeImpactCallerDto, KnowledgeImpactDto,
    KnowledgeImpactQuery, KnowledgeOutlineEntryDto, KnowledgeOutlineQuery, KnowledgeRelatedDto,
    KnowledgeSearchQuery, KnowledgeState, KnowledgeStatusDto, KnowledgeSymbolDto,
    KnowledgeSymbolQuery,
};
use ariadne_client::Client;

use super::query_path;
use super::resolve::{self, Kind};
use crate::output::{
    Column, Format, Kv, UNCAPPED, col, empty_state, moment, note, ok_id_line, print, print_json,
    print_kv, print_list, short_id, view,
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

/// Columns of `knowledge impact`: where the caller is, then how far away it
/// is and what it is. The location leads, as it does on a search, so `-q`
/// prints `path:line`.
const IMPACT: &[Column] = &[
    col("location", UNCAPPED),
    col("depth", UNCAPPED),
    col("title", 40).title(),
    col("confidence", UNCAPPED).rank(1),
    col("changed", 40).rank(2),
    col("repo", UNCAPPED).id().rank(3),
];

/// How much `knowledge symbol` shows. A local spelling of
/// [`KnowledgeDetail`], because clap derives the flag's values from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Detail {
    /// Where the definition is, and its signature.
    Outline,
    /// Its text too.
    Source,
    /// Its callers, callees, implementations and tests too.
    Context,
}

impl From<Detail> for KnowledgeDetail {
    fn from(detail: Detail) -> KnowledgeDetail {
        match detail {
            Detail::Outline => KnowledgeDetail::Outline,
            Detail::Source => KnowledgeDetail::Source,
            Detail::Context => KnowledgeDetail::Context,
        }
    }
}

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
    /// Read one definition by name, with its context
    Symbol {
        /// The name of the definition
        name: String,
        /// Only this repository, by id or path (default: every repository)
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repository: Option<String>,
        /// The branch to read (default: the base branch of each repository)
        #[arg(long = "ref", value_name = "REF")]
        git_ref: Option<String>,
        /// How much to show: outline, source or context
        #[arg(long, value_enum, default_value_t = Detail::Outline)]
        detail: Detail,
    },
    /// List the callers a change reaches, by how far away they are
    Impact {
        /// Repository id or path
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repository: String,
        /// The name of the definition that changed
        #[arg(long, conflicts_with = "diff")]
        symbol: Option<String>,
        /// Every definition a diff changed, as <BASE>..<HEAD>
        #[arg(long, conflicts_with = "symbol")]
        diff: Option<String>,
        /// The branch to read (default: the base branch)
        #[arg(long = "ref", value_name = "REF")]
        git_ref: Option<String>,
        /// How far to walk the callers (default 2, max 4)
        #[arg(long)]
        depth: Option<i64>,
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
        KnowledgeCommand::Symbol {
            name,
            repository,
            git_ref,
            detail,
        } => {
            let repository = match repository {
                Some(repo) => Some(resolve::id(client, Kind::Repo, &repo).await?),
                None => None,
            };
            let request = KnowledgeSymbolQuery {
                name,
                repository,
                git_ref,
                detail: Some(detail.into()),
            };
            let found: Vec<KnowledgeSymbolDto> = client
                .get_json(&query_path("/v1/knowledge/symbol", &request)?)
                .await?;
            print(format, &found, || print_symbols(&found))?;
        }
        KnowledgeCommand::Impact {
            repository,
            symbol,
            diff,
            git_ref,
            depth,
        } => {
            let repository = resolve::id(client, Kind::Repo, &repository).await?;
            let request = KnowledgeImpactQuery {
                repository,
                git_ref,
                symbol,
                diff,
                depth,
            };
            let found: Vec<KnowledgeImpactDto> = client
                .get_json(&query_path("/v1/knowledge/impact", &request)?)
                .await?;
            // The daemon's own objects for a script: one row per caller is
            // the table's shape, not the answer's.
            if let Format::Json = format {
                return print_json(&found);
            }
            // One row per caller, each naming the changed definition it
            // was reached from.
            let rows: Vec<(&KnowledgeImpactDto, &KnowledgeImpactCallerDto)> = found
                .iter()
                .flat_map(|impact| impact.callers.iter().map(move |caller| (impact, caller)))
                .collect();
            print_list(
                format,
                &rows,
                IMPACT,
                |(impact, caller)| {
                    vec![
                        format!("{}:{}", caller.path, caller.line),
                        caller.depth.to_string(),
                        caller.name.clone(),
                        caller.confidence.clone(),
                        impact.symbol.name.clone(),
                        caller.repository_id.clone(),
                    ]
                },
                empty_state("Nothing calls what changed.", None),
            )?;
            // A note, so `-q` stays a list of locations and nothing else.
            for name in found.iter().flat_map(|impact| &impact.stopped) {
                note(&format!(
                    "{name} has more than 200 callers: the walk stopped there."
                ));
            }
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

/// Every definition of a name, one block each: where it is, what it is, and
/// the lists `--detail` asked for.
fn print_symbols(found: &[KnowledgeSymbolDto]) {
    if found.is_empty() {
        println!("No definition matches.");
        return;
    }
    for definition in found {
        let mut rows = vec![
            (
                "location",
                Kv::from(format!(
                    "{}:{}-{}",
                    definition.path, definition.start_line, definition.end_line
                )),
            ),
            ("kind", definition.kind.clone().into()),
            ("name", definition.name.clone().into()),
            ("signature", definition.signature.clone().into()),
            (
                "doc",
                definition.doc.clone().unwrap_or_else(|| "-".into()).into(),
            ),
            ("repository", Kv::id(definition.repository_id.clone())),
        ];
        if let Some(context) = &definition.context {
            for (name, ends) in [
                ("callers", &context.callers),
                ("callees", &context.callees),
                ("implementations", &context.implementations),
                ("tests", &context.tests),
            ] {
                rows.push((name, ends_line(ends).into()));
            }
        }
        print_kv(&rows);
        if let Some(source) = &definition.source {
            println!("{source}");
        }
    }
}

/// One line for a list of edge ends, or `-` for an empty one.
fn ends_line(ends: &[KnowledgeRelatedDto]) -> String {
    match ends.is_empty() {
        true => "-".to_string(),
        false => ends
            .iter()
            .map(|end| {
                format!(
                    "{}:{} {} ({})",
                    end.path, end.line, end.name, end.confidence
                )
            })
            .collect::<Vec<_>>()
            .join("; "),
    }
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

    /// An impact row leads with the location too, so `-q` prints
    /// `path:line`, and it carries how far away the caller is.
    #[test]
    fn an_impact_row_leads_with_its_location_and_says_how_far_away_it_is() {
        let table = crate::output::render_table(
            IMPACT,
            &[vec![
                "src/a.rs:3".into(),
                "1".into(),
                "a".into(),
                "exact".into(),
                "b".into(),
                "01REPO".into(),
            ]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.starts_with("LOCATION"), "{table}");
        assert!(header.contains("DEPTH"), "{table}");
    }
}
