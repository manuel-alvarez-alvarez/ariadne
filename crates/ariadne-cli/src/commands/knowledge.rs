//! `ariadne knowledge ...`

use anyhow::Result;
use clap::Subcommand;
use std::collections::BTreeMap;
use std::fmt::Write;

use ariadne_api::knowledge::{
    KnowledgeDetail, KnowledgeEdgeDto, KnowledgeEndpointDto, KnowledgeGraphDto,
    KnowledgeGraphQuery, KnowledgeHitDto, KnowledgeImpactCallerDto, KnowledgeImpactDto,
    KnowledgeImpactQuery, KnowledgeInteractionGroupDto, KnowledgeInteractionsQuery,
    KnowledgeMapDto, KnowledgeMapQuery, KnowledgeOutlineEntryDto, KnowledgeOutlineQuery,
    KnowledgePathDto, KnowledgePathQuery, KnowledgeRelatedDto, KnowledgeSearchQuery,
    KnowledgeState, KnowledgeStatusDto, KnowledgeSymbolDto, KnowledgeSymbolQuery,
};
#[cfg(test)]
use ariadne_api::knowledge::{KnowledgeGraphEdgeDto, KnowledgeGraphNodeDto};
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
/// is, what it is, and how sure the call is — the confidence and the step
/// that answered the name. The location leads, as it does on a search, so
/// `-q` prints `path:line`.
const IMPACT: &[Column] = &[
    col("location", UNCAPPED),
    col("depth", UNCAPPED),
    col("title", 40).title(),
    col("confidence", UNCAPPED).rank(1),
    col("step", UNCAPPED).rank(1),
    col("changed", 40).rank(2),
    col("repo", UNCAPPED).id().rank(3),
];

/// Columns of `knowledge path`: each definition, followed by the directed
/// edge that enters it. The first column is what `-q` prints.
const PATH: &[Column] = &[
    col("location", UNCAPPED),
    col("kind", UNCAPPED).rank(2),
    col("title", 40).title(),
    col("edge", UNCAPPED).rank(1),
    col("confidence", UNCAPPED).rank(1),
    col("repo", UNCAPPED).id().rank(3),
];

/// Columns of `knowledge interactions`: where the edge starts, its kind,
/// what the two ends are, how sure the match is and what joined them. The
/// from end leads, so `-q` prints `path:line`.
const INTERACTIONS: &[Column] = &[
    col("from", UNCAPPED),
    col("kind", UNCAPPED).rank(2),
    col("title", 40).title(),
    col("to", UNCAPPED),
    col("confidence", UNCAPPED).rank(1),
    col("step", UNCAPPED).rank(1),
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
pub(crate) enum KnowledgeCommand {
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
        /// type, macro, constant, test, heading
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
    /// List the shortest directed path between two definitions
    Path {
        /// The name of every starting definition
        from: String,
        /// The name of every ending definition
        to: String,
        /// Repository id or path
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repository: String,
        /// The branch to read (default: the base branch)
        #[arg(long = "ref", value_name = "REF")]
        git_ref: Option<String>,
        /// How far to walk the directed edges (default 6, max 10)
        #[arg(long)]
        depth: Option<i64>,
    },
    /// List what joins a repository to the others: the packages it depends
    /// on, the names it references, the routes it calls, the variables it
    /// sets, and the same the other way round
    Interactions {
        /// Repository id or path
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
        /// The branch to read (default: the base branch)
        #[arg(long = "ref", value_name = "REF")]
        git_ref: Option<String>,
    },
    /// Rank the files of a repository and name the definitions in them
    ///
    /// The ranking is a PageRank over the references between the files, so
    /// the files most of the repository names come first. `--path` ranks
    /// the neighbors of one file first instead.
    Map {
        /// Repository id or path
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
        /// Rank the files around this one first
        #[arg(long)]
        path: Option<String>,
        /// How long the map may be, in tokens (default 1000, max 4000)
        #[arg(long)]
        budget: Option<i64>,
        /// The branch to read (default: the base branch)
        #[arg(long = "ref", value_name = "REF")]
        git_ref: Option<String>,
    },
    /// Show the files of a repository ref and the edges between them
    Graph {
        /// Repository id or path
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
        /// The branch to read (default: the base branch)
        #[arg(long = "ref", value_name = "REF")]
        git_ref: Option<String>,
        /// How many file nodes to return (default 2000, max 10000)
        #[arg(long)]
        limit: Option<i64>,
        /// Print the full graph as JSON
        #[arg(long)]
        json: bool,
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

pub(crate) async fn run(client: &Client, command: KnowledgeCommand, format: Format) -> Result<()> {
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
                        caller.step.clone(),
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
        KnowledgeCommand::Path {
            from,
            to,
            repository,
            git_ref,
            depth,
        } => {
            let repository = resolve::id(client, Kind::Repo, &repository).await?;
            let request = KnowledgePathQuery {
                repository,
                from,
                to,
                git_ref,
                depth,
            };
            let found: KnowledgePathDto = client
                .get_json(&query_path("/v1/knowledge/path", &request)?)
                .await?;
            if let Format::Json = format {
                return print_json(&found);
            }
            print_list(
                format,
                &found.hops,
                PATH,
                |hop| {
                    vec![
                        format!("{}:{}", hop.path, hop.line),
                        hop.kind.clone(),
                        hop.name.clone(),
                        hop.edge_kind.clone().unwrap_or_else(|| "-".into()),
                        hop.confidence.clone().unwrap_or_else(|| "-".into()),
                        hop.repository_id.clone(),
                    ]
                },
                empty_state("No path exists within the depth.", None),
            )?;
        }
        KnowledgeCommand::Interactions { repo, git_ref } => {
            let repository = resolve::id(client, Kind::Repo, &repo).await?;
            let request = KnowledgeInteractionsQuery {
                repository,
                git_ref,
            };
            let found: Vec<KnowledgeInteractionGroupDto> = client
                .get_json(&query_path("/v1/knowledge/interactions", &request)?)
                .await?;
            // The daemon's own groups for a script; one row per edge for
            // the table.
            if let Format::Json = format {
                return print_json(&found);
            }
            let rows: Vec<(&KnowledgeInteractionGroupDto, &KnowledgeEdgeDto)> = found
                .iter()
                .flat_map(|group| group.edges.iter().map(move |edge| (group, edge)))
                .collect();
            print_list(
                format,
                &rows,
                INTERACTIONS,
                |(group, edge)| {
                    vec![
                        end_location(&edge.from),
                        group.kind.clone(),
                        edge.from.symbol.clone(),
                        format!("{} {}", end_location(&edge.to), edge.to.symbol),
                        edge.confidence.clone(),
                        edge.step.clone(),
                    ]
                },
                empty_state("No interaction with another repository.", None),
            )?;
        }
        KnowledgeCommand::Map {
            repo,
            path,
            budget,
            git_ref,
        } => {
            let repository = resolve::id(client, Kind::Repo, &repo).await?;
            let request = KnowledgeMapQuery {
                repository,
                git_ref,
                path,
                budget,
            };
            let map: KnowledgeMapDto = client
                .get_json(&query_path("/v1/knowledge/map", &request)?)
                .await?;
            // The map is one text, so it is printed as it came: a file per
            // heading, its definitions under it.
            print(format, &map, || print!("{}", map_text(&map)))?;
        }
        KnowledgeCommand::Graph {
            repo,
            git_ref,
            limit,
            json,
        } => {
            let repository = resolve::id(client, Kind::Repo, &repo).await?;
            let request = KnowledgeGraphQuery {
                repository,
                git_ref,
                limit,
            };
            let graph: KnowledgeGraphDto = client
                .get_json(&query_path("/v1/knowledge/graph", &request)?)
                .await?;
            if json || matches!(format, Format::Json) {
                return print_json(&graph);
            }
            print!("{}", graph_text(&graph));
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
            for (name, ends, left) in [
                ("callers", &context.callers, context.more.callers),
                ("callees", &context.callees, context.more.callees),
                (
                    "implementations",
                    &context.implementations,
                    context.more.implementations,
                ),
                ("references", &context.references, context.more.references),
                ("tests", &context.tests, context.more.tests),
            ] {
                rows.push((
                    name,
                    ends_line(ends, &definition.repository_id, left).into(),
                ));
            }
        }
        print_kv(&rows);
        if let Some(source) = &definition.source {
            println!("{source}");
        }
    }
}

/// One line for a list of edge ends, or `-` for an empty one. An end in
/// another repository than `repository_id` is led by that repository's id,
/// and each carries what the edge rests on: its confidence, the step that
/// answered the name, and how many definitions matched there. `left` past 0
/// ends the line with how many more matched the cap.
fn ends_line(ends: &[KnowledgeRelatedDto], repository_id: &str, left: i64) -> String {
    let mut parts: Vec<String> = ends
        .iter()
        .map(|end| {
            let location = format!("{}:{}", end.path, end.line);
            let location = match end.repository_id == repository_id {
                true => location,
                false => format!("{}:{location}", end.repository_id),
            };
            let resolution = match (&end.step, end.candidates) {
                (None, _) => String::new(),
                (Some(step), 2..) => format!(" via {step}, {} candidates", end.candidates),
                (Some(step), _) => format!(" via {step}"),
            };
            format!("{location} {} ({}{resolution})", end.name, end.confidence)
        })
        .collect();
    if left > 0 {
        parts.push(format!("{left} more. Narrow the query."));
    }
    match parts.is_empty() {
        true => "-".to_string(),
        false => parts.join("; "),
    }
}

/// The map's text as it came, or, where the budget held none of it, how
/// many ranked files were left out — a budget too small even for that line
/// still names them, rather than reading as an empty repository.
fn map_text(map: &KnowledgeMapDto) -> String {
    match (map.text.is_empty(), map.files_left > 0) {
        (false, _) => map.text.clone(),
        (true, true) => format!(
            "{} files left. Raise the budget or name a path.\n",
            map.files_left
        ),
        (true, false) => "The repository holds nothing the index read.\n".to_string(),
    }
}

fn graph_text(graph: &KnowledgeGraphDto) -> String {
    let mut by_kind: BTreeMap<&str, i64> = BTreeMap::new();
    let mut degree: BTreeMap<&str, i64> = graph
        .nodes
        .iter()
        .map(|node| (node.path.as_str(), 0))
        .collect();
    for edge in &graph.edges {
        *by_kind.entry(&edge.kind).or_default() += 1;
        *degree.entry(&edge.from).or_default() += 1;
        *degree.entry(&edge.to).or_default() += 1;
    }
    let edge_count: i64 = by_kind.values().sum();
    let mut text = match graph.truncated {
        true => format!(
            "{} of {} nodes, {edge_count} edges\n",
            graph.nodes.len(),
            graph.total_nodes
        ),
        false => format!("{} nodes, {edge_count} edges\n", graph.nodes.len()),
    };
    for (kind, count) in by_kind {
        writeln!(text, "{kind}: {count}").expect("writing to a string cannot fail");
    }
    text.push_str("\nTop files by degree\n");
    let mut ranked: Vec<(&str, i64)> = degree.into_iter().collect();
    ranked.sort_by(|(path_a, degree_a), (path_b, degree_b)| {
        degree_b.cmp(degree_a).then_with(|| path_a.cmp(path_b))
    });
    for (path, degree) in ranked.into_iter().take(10) {
        writeln!(text, "{degree}  {path}").expect("writing to a string cannot fail");
    }
    text
}

/// `repository:path:line`, as an interaction's end is printed: either end
/// may be in the other repository.
fn end_location(end: &KnowledgeEndpointDto) -> String {
    format!("{}:{}:{}", end.repository_id, end.path, end.line)
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
        ("failures", failures_text(status).into()),
    ]);
}

/// The refs whose last run failed, each named with why: a good run of one
/// ref leaves the failure of another, so the ref is what tells them apart.
fn failures_text(status: &KnowledgeStatusDto) -> String {
    match status.failures.is_empty() {
        true => "-".to_string(),
        false => status
            .failures
            .iter()
            .map(|failure| format!("{}: {}", failure.git_ref, failure.error))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The status names each ref that failed beside its error, and a
    /// repository with no failure reads `-`.
    #[test]
    fn a_status_names_each_ref_that_failed() {
        let mut status = KnowledgeStatusDto {
            repository_id: "01REPO".into(),
            state: KnowledgeState::Failed,
            refs: Vec::new(),
            files: 0,
            symbols: 0,
            languages: Vec::new(),
            failures: Vec::new(),
        };
        assert_eq!(failures_text(&status), "-");
        status.failures = vec![
            ariadne_api::knowledge::KnowledgeFailureDto {
                git_ref: "main".into(),
                error: "resolving main in /work/api".into(),
            },
            ariadne_api::knowledge::KnowledgeFailureDto {
                git_ref: "fix-w1".into(),
                error: "database is locked".into(),
            },
        ];
        assert_eq!(
            failures_text(&status),
            "main: resolving main in /work/api; fix-w1: database is locked"
        );
    }

    /// The graph summary counts symbol edges by kind and ranks files by
    /// their degree.
    #[test]
    fn graph_summary_counts_edges_by_kind_and_ranks_files_by_degree() {
        let graph = KnowledgeGraphDto {
            repository_id: "01REPO".into(),
            git_ref: "main".into(),
            nodes: vec![
                KnowledgeGraphNodeDto {
                    path: "src/a.rs".into(),
                    language: "rust".into(),
                    symbols: 2,
                },
                KnowledgeGraphNodeDto {
                    path: "src/b.rs".into(),
                    language: "rust".into(),
                    symbols: 1,
                },
                KnowledgeGraphNodeDto {
                    path: "src/c.rs".into(),
                    language: "rust".into(),
                    symbols: 1,
                },
            ],
            edges: vec![
                KnowledgeGraphEdgeDto {
                    from: "src/a.rs".into(),
                    to: "src/b.rs".into(),
                    kind: "calls".into(),
                    count: 3,
                    confidence: ariadne_api::knowledge::KnowledgeGraphConfidence::Exact,
                },
                KnowledgeGraphEdgeDto {
                    from: "src/c.rs".into(),
                    to: "src/b.rs".into(),
                    kind: "references".into(),
                    count: 1,
                    confidence: ariadne_api::knowledge::KnowledgeGraphConfidence::Heuristic,
                },
            ],
            truncated: false,
            total_nodes: 3,
        };

        assert_eq!(
            graph_text(&graph),
            "3 nodes, 2 edges\n\
             calls: 1\n\
             references: 1\n\
             \n\
             Top files by degree\n\
             2  src/b.rs\n\
             1  src/a.rs\n\
             1  src/c.rs\n"
        );
    }

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
    /// `path:line`, and it carries how far away the caller is and the step
    /// that resolved the call.
    #[test]
    fn an_impact_row_leads_with_its_location_and_says_how_far_away_it_is() {
        let table = crate::output::render_table(
            IMPACT,
            &[vec![
                "src/a.rs:3".into(),
                "1".into(),
                "a".into(),
                "exact".into(),
                "file".into(),
                "b".into(),
                "01REPO".into(),
            ]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.starts_with("LOCATION"), "{table}");
        assert!(header.contains("DEPTH"), "{table}");
        assert!(header.contains("STEP"), "{table}");
    }

    /// An interaction row leads with its from end, so `-q` prints where the
    /// edge starts, and names its kind, its to end and the step that joined
    /// them.
    #[test]
    fn an_interaction_row_leads_with_its_from_end_and_names_its_kind() {
        let table = crate::output::render_table(
            INTERACTIONS,
            &[vec![
                "01WEB:src/client.ts:4".into(),
                "calls_route".into(),
                "fetchItem".into(),
                "01API:src/lib.rs:12 get_item".into(),
                "heuristic".into(),
                "route".into(),
            ]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.starts_with("FROM"), "{table}");
        assert!(header.contains("KIND"), "{table}");
        assert!(header.contains("TO"), "{table}");
        assert!(header.contains("STEP"), "{table}");
    }

    /// An end of a `knowledge symbol` block says what its edge rests on: the
    /// step that answered the name, and how many definitions matched there
    /// where the step held several.
    #[test]
    fn an_end_row_names_the_step_that_resolved_it() {
        let end = |name: &str, confidence: &str, step: &str, candidates: i64| KnowledgeRelatedDto {
            repository_id: "01REPO".into(),
            path: "src/a.rs".into(),
            line: 3,
            name: name.into(),
            confidence: confidence.into(),
            step: Some(step.into()),
            candidates,
        };
        let ends = [
            end("a", "exact", "file", 1),
            end("c", "heuristic", "repository", 2),
        ];
        assert_eq!(
            ends_line(&ends, "01REPO", 0),
            "src/a.rs:3 a (exact via file); \
             src/a.rs:3 c (heuristic via repository, 2 candidates)"
        );
    }

    /// A capped list ends with how many more matched the cap; a whole list
    /// does not.
    #[test]
    fn a_capped_list_ends_with_how_many_more_matched() {
        let ends = [KnowledgeRelatedDto {
            repository_id: "01REPO".into(),
            path: "src/a.rs".into(),
            line: 3,
            name: "a".into(),
            confidence: "exact".into(),
            step: Some("file".into()),
            candidates: 1,
        }];
        assert_eq!(
            ends_line(&ends, "01REPO", 5),
            "src/a.rs:3 a (exact via file); 5 more. Narrow the query."
        );
        assert_eq!(
            ends_line(&ends, "01REPO", 0),
            "src/a.rs:3 a (exact via file)"
        );
    }

    /// A budget too small to hold even the files-left line still names how
    /// many were left out, rather than reading as an empty repository. A
    /// repository truly holding nothing keeps its own note.
    #[test]
    fn map_text_says_how_many_files_are_left_when_the_budget_held_none() {
        let map = KnowledgeMapDto {
            repository_id: "01REPO".into(),
            git_ref: "main".into(),
            text: String::new(),
            tokens: 0,
            files: 0,
            files_left: 2,
        };
        assert_eq!(
            map_text(&map),
            "2 files left. Raise the budget or name a path.\n"
        );

        let empty = KnowledgeMapDto {
            files_left: 0,
            ..map
        };
        assert_eq!(
            map_text(&empty),
            "The repository holds nothing the index read.\n"
        );
    }
}
