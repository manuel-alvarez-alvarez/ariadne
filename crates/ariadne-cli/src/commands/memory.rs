//! `ariadne memory ...`

use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use ariadne_api::memories::MemoryDto;
use ariadne_client::Client;

use super::resolve::{self, Kind};
use super::{Subject, confirm, path_segment};
use crate::output::{
    Column, Format, UNCAPPED, age, col, empty_state, moment, ok_id_line, print, print_list,
    short_id, view,
};

const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 64).title(),
    col("age", UNCAPPED).rank(4),
    col("expires", UNCAPPED).rank(3),
    col("source", 64).rank(2),
];

#[derive(Subcommand)]
pub enum MemoryCommand {
    /// List active entries
    Ls {
        /// Repository id or path
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
    },
    /// Search active entries
    Search {
        /// Text to find, without case sensitivity
        query: String,
        /// Repository id or path
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
    },
    /// Delete an entry
    Delete {
        /// Memory id
        id: String,
        /// Repository id or path
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        repo: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

pub async fn run(client: &Client, command: MemoryCommand, format: Format) -> Result<()> {
    match command {
        MemoryCommand::Ls { repo } => {
            let repository_id = resolve::id(client, Kind::Repo, &repo).await?;
            let memories: Vec<MemoryDto> = client.get_json(&list_path(&repository_id)).await?;
            print_memories(
                format,
                &memories,
                empty_state("No active memories for this repository.", None),
            )?;
        }
        MemoryCommand::Search { query, repo } => {
            let repository_id = resolve::id(client, Kind::Repo, &repo).await?;
            let query = serde_urlencoded::to_string([("q", query)])?;
            let memories: Vec<MemoryDto> = client
                .get_json(&format!("{}/search?{query}", list_path(&repository_id)))
                .await?;
            print_memories(
                format,
                &memories,
                empty_state("No matching memories.", None),
            )?;
        }
        MemoryCommand::Delete { id, repo, yes } => {
            let repository_id = resolve::id(client, Kind::Repo, &repo).await?;
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
                    &format!("{}/{}", list_path(&repository_id), path_segment(&id)),
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

fn list_path(repository_id: &str) -> String {
    format!("/v1/repositories/{repository_id}/memories")
}

fn print_memories(format: Format, memories: &[MemoryDto], empty: String) -> Result<()> {
    let now = chrono::Utc::now();
    print_list(
        format,
        memories,
        LS,
        |memory| {
            let task = memory
                .source_task_id
                .as_deref()
                .map(short_id)
                .unwrap_or_else(|| "-".into());
            vec![
                memory.id.clone(),
                memory.text.clone(),
                age(&memory.created_at, now),
                moment(&memory.expires_at),
                format!(
                    "session {} · task {} · goal {}",
                    short_id(&memory.source_session_id),
                    task,
                    short_id(&memory.source_goal_id)
                ),
            ]
        },
        empty,
    )
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
}
