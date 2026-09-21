//! `ariadne memory ...`

use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use ariadne_api::memories::{
    CreateMemoryRequest, MemoryDto, MemoryListQuery, MemoryScope, MemorySearchQuery,
    MemorySearchResult,
};
use ariadne_client::Client;

use super::resolve::{self, Kind};
use super::{Subject, confirm, path_segment, query_path};
use crate::output::{
    Column, Format, UNCAPPED, age, col, empty_state, moment, note, ok_id_line, print, print_json,
    print_list, short_id, view,
};

const MEMORIES: &str = "/v1/memories";

const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 64).title(),
    col("scope", UNCAPPED).id(),
    col("age", UNCAPPED).rank(4),
    col("expires", UNCAPPED).rank(3),
    col("source", 64).rank(2),
];

#[derive(Subcommand)]
pub(crate) enum MemoryCommand {
    /// Save a memory
    ///
    /// Names the one scope the fact belongs to: a repository, or every one
    /// with `--global`.
    #[command(group = clap::ArgGroup::new("add-scope").args(["repo", "global"]).required(true))]
    Add {
        /// The fact to save
        text: String,
        /// Repository id or path the fact is about
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: Option<String>,
        /// Save a fact true of every repository, not one
        #[arg(long)]
        global: bool,
        /// RFC 3339 time after which the fact stays hidden. Omit it for a
        /// fact that never expires
        #[arg(long, value_name = "TIME")]
        expires: Option<String>,
    },
    /// List active entries
    #[command(group = clap::ArgGroup::new("ls-scope").args(["repo", "global"]))]
    Ls {
        /// Repository id or path: read that repository and the global
        /// memories. Name neither this nor `--global` to read every scope
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: Option<String>,
        /// Read only the memories of no repository
        #[arg(long)]
        global: bool,
    },
    /// Search active entries
    #[command(group = clap::ArgGroup::new("search-scope").args(["repo", "global"]))]
    Search {
        /// Text to find, without case sensitivity
        query: String,
        /// Repository id or path: read that repository and the global
        /// memories. Name neither this nor `--global` to read every scope
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: Option<String>,
        /// Search only the memories of no repository
        #[arg(long)]
        global: bool,
    },
    /// Delete an entry
    Delete {
        /// Memory id
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

pub(crate) async fn run(client: &Client, command: MemoryCommand, format: Format) -> Result<()> {
    match command {
        MemoryCommand::Add {
            text,
            repo,
            global: _,
            expires,
        } => {
            let repository_id = match repo {
                Some(repo) => Some(resolve::id(client, Kind::Repo, &repo).await?),
                None => None,
            };
            let memory: MemoryDto = client
                .post_json(
                    MEMORIES,
                    &CreateMemoryRequest {
                        text,
                        repository_id,
                        expires_at: expires,
                    },
                )
                .await?;
            print(format, &memory, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "saved", &memory.id)
                )
            })?;
        }
        MemoryCommand::Ls { repo, global } => {
            let (repository, scope) = scope(client, repo, global).await?;
            let memories: Vec<MemoryDto> = client
                .get_json(&query_path(
                    MEMORIES,
                    &MemoryListQuery { repository, scope },
                )?)
                .await?;
            print_memories(format, &memories, empty_state("No active memories.", None))?;
        }
        MemoryCommand::Search {
            query,
            repo,
            global,
        } => {
            let (repository, scope) = scope(client, repo, global).await?;
            let result: MemorySearchResult = client
                .get_json(&query_path(
                    &format!("{MEMORIES}/search"),
                    &MemorySearchQuery {
                        q: query,
                        repository,
                        scope,
                    },
                )?)
                .await?;
            if format == Format::Json {
                return print_json(&result);
            }
            if result.fallback {
                note("No word matched. Showing the newest entries instead.");
            }
            print_memories(
                format,
                &result.hits,
                empty_state("No matching memories.", None),
            )?;
        }
        MemoryCommand::Delete { id, yes } => {
            let subject = Subject::new("memory", &id, &id);
            confirm(
                "delete",
                &subject,
                &format!("Delete memory {}?", subject.named()),
                yes,
            )?;
            client
                .send_no_content::<()>(
                    http::Method::DELETE,
                    &format!("{MEMORIES}/{}", path_segment(&id)),
                    None,
                )
                .await?;
            print(format, &json!({"memory": id, "deleted": true}), || {
                println!("{}", ok_id_line(view().color, view().quiet, "deleted", &id))
            })?;
        }
    }
    Ok(())
}

/// What `ls` and `search` read: the named repository (and the global
/// memories with it), only the global memories, or, named neither, every
/// memory of every scope.
async fn scope(
    client: &Client,
    repo: Option<String>,
    global: bool,
) -> Result<(Option<String>, Option<MemoryScope>)> {
    if global {
        return Ok((None, Some(MemoryScope::Global)));
    }
    match repo {
        Some(repo) => Ok((Some(resolve::id(client, Kind::Repo, &repo).await?), None)),
        None => Ok((None, None)),
    }
}

fn print_memories(format: Format, memories: &[MemoryDto], empty: String) -> Result<()> {
    let now = chrono::Utc::now();
    print_list(
        format,
        memories,
        LS,
        |memory| memory_row(memory, now),
        empty,
    )
}

fn memory_row(memory: &MemoryDto, now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    let short = |id: &Option<String>| id.as_deref().map(short_id).unwrap_or_else(|| "-".into());
    vec![
        memory.id.clone(),
        memory.text.clone(),
        memory
            .repository_id
            .clone()
            .unwrap_or_else(|| "global".into()),
        age(&memory.created_at, now),
        memory
            .expires_at
            .as_deref()
            .map(moment)
            .unwrap_or_else(|| "-".into()),
        format!(
            "session {} · task {} · goal {}",
            short(&memory.source_session_id),
            short(&memory.source_task_id),
            short(&memory.source_goal_id)
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memory_subject_column_is_title() {
        let table = crate::output::render_table(
            LS,
            &[vec![String::new(); LS.len()]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.contains("TITLE"), "{table}");
        assert!(!header.contains("TEXT"), "{table}");
    }

    #[test]
    fn the_search_result_reads_hits_and_fallback() {
        let result: MemorySearchResult =
            serde_json::from_str(r#"{"hits":[],"fallback":true}"#).expect("json");
        assert!(result.fallback);
        assert!(result.hits.is_empty());
    }

    fn memory(repository_id: Option<&str>) -> MemoryDto {
        MemoryDto {
            id: "01MEMORY".into(),
            repository_id: repository_id.map(str::to_string),
            text: "Run the parser fixture.".into(),
            source_session_id: None,
            source_task_id: None,
            source_goal_id: None,
            created_at: "2026-09-19T10:00:00.000Z".into(),
            expires_at: None,
        }
    }

    #[test]
    fn the_memory_list_names_its_scope() {
        let table = crate::output::render_table(
            LS,
            &[vec![String::new(); LS.len()]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.contains("SCOPE"), "{table}");

        let scope = LS.iter().position(|c| c.header == "scope").expect("scope");
        let now = chrono::Utc::now();
        assert_eq!(memory_row(&memory(Some("01REPO")), now)[scope], "01REPO");
        assert_eq!(memory_row(&memory(None), now)[scope], "global");
    }
}
