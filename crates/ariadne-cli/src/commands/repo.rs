//! `ariadne repo ...`

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Subcommand;

use ariadne_api::repositories::{CreateRepositoryRequest, RepositoryDto, UpdateRepositoryRequest};
use ariadne_client::Client;
use ariadne_core::MergeStrategy;
use serde_json::json;

use super::resolve::{self, Kind};
use super::{Subject, confirm};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, Kv, UNCAPPED, age, col, moment, ok_id_line, print, print_kv, print_list, style,
    view,
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
        /// The landing briefing this repository hands its engineer, as text
        /// on the line. Omit for the default of --merge-strategy
        #[arg(long, value_name = "TEXT", conflicts_with = "landing_prompt_file")]
        landing_prompt: Option<String>,
        /// The same, read from a file
        #[arg(long, value_name = "PATH")]
        landing_prompt_file: Option<PathBuf>,
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
        /// New landing briefing text, replacing whatever it holds
        #[arg(long, value_name = "TEXT", conflicts_with_all = ["landing_prompt_file", "reset_landing_prompt"])]
        landing_prompt: Option<String>,
        /// The same, read from a file
        #[arg(long, value_name = "PATH", conflicts_with = "reset_landing_prompt")]
        landing_prompt_file: Option<PathBuf>,
        /// Put the landing briefing back on the merge strategy's default
        #[arg(long)]
        reset_landing_prompt: bool,
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
    /// Show, pipe out or reset the landing briefing (set it with add|update
    /// --landing-prompt)
    Prompt {
        #[command(subcommand)]
        command: RepoPromptCommand,
    },
}

/// `ariadne repo prompt ...` — the other half of the landing briefing story:
/// where `repo add`/`repo update` write it, this prints, pipes and resets it
/// for a repository that already exists.
#[derive(Subcommand)]
pub enum RepoPromptCommand {
    /// Print the landing briefing raw, ready to be piped to a file
    Get {
        /// Repository id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        id: String,
    },
    /// Replace the landing briefing with the contents of a file, or with stdin
    Set {
        /// Repository id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        id: String,
        /// Read the new text from this file (default: stdin)
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Put the landing briefing back on the merge strategy's default
    Reset {
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
            landing_prompt,
            landing_prompt_file,
        } => {
            let repo: RepositoryDto = client
                .post_json(
                    "/v1/repositories",
                    &CreateRepositoryRequest {
                        path,
                        base_branch: branch,
                        description,
                        merge_strategy: Some(merge_strategy),
                        landing_prompt: read_landing_prompt(landing_prompt, landing_prompt_file)?,
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
                    ("id", Kv::id(r.id.clone())),
                    ("path", r.path.clone().into()),
                    ("branch", r.base_branch.clone().into()),
                    ("merge_strategy", r.merge_strategy.as_str().into()),
                    (
                        "description",
                        r.description.clone().unwrap_or_else(|| "-".into()).into(),
                    ),
                    ("landing_prompt", landing_status(&r).into()),
                    ("created", Kv::meta(moment(&r.created_at))),
                    ("updated", Kv::meta(moment(&r.updated_at))),
                ]);
                println!("\n---\n{}", r.landing_prompt);
            })?;
        }
        RepoCommand::Update {
            id,
            path,
            branch,
            description,
            merge_strategy,
            landing_prompt,
            landing_prompt_file,
            reset_landing_prompt,
        } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let landing_prompt = match reset_landing_prompt {
                true => Some(String::new()),
                false => read_landing_prompt(landing_prompt, landing_prompt_file)?,
            };
            let r: RepositoryDto = client
                .put_json(
                    &repo_path(&id),
                    &UpdateRepositoryRequest {
                        path,
                        base_branch: branch,
                        description,
                        merge_strategy,
                        landing_prompt,
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
        RepoCommand::Prompt { command } => run_prompt(client, command, format).await?,
    }
    Ok(())
}

async fn run_prompt(client: &Client, cmd: RepoPromptCommand, format: Format) -> Result<()> {
    match cmd {
        RepoPromptCommand::Get { id } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client.get_json(&repo_path(&id)).await?;
            // Raw and unadorned, trailing newline included or not exactly as
            // it is stored: `prompt get > file` then `prompt set --file` has
            // to round-trip.
            print(format, &r, || print!("{}", r.landing_prompt))?;
        }
        RepoPromptCommand::Set { id, file } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let content = read_content(file)?;
            let r: RepositoryDto = client
                .put_json(
                    &repo_path(&id),
                    &UpdateRepositoryRequest {
                        landing_prompt: Some(content),
                        ..Default::default()
                    },
                )
                .await?;
            print(format, &r, || {
                println!(
                    "{} landing prompt of {} ({})",
                    style::paint(view().color, style::OK, "updated"),
                    r.path,
                    style::paint(view().color, style::ID, &r.id)
                )
            })?;
        }
        RepoPromptCommand::Reset { id, yes } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client.get_json(&repo_path(&id)).await?;
            let subject = Subject::new("repository", &r.path, &r.id);
            confirm(
                "reset the landing prompt of",
                &subject,
                &reset_prompt_question(&r, &subject),
                yes,
            )?;
            let r: RepositoryDto = client
                .put_json(
                    &repo_path(&id),
                    &UpdateRepositoryRequest {
                        landing_prompt: Some(String::new()),
                        ..Default::default()
                    },
                )
                .await?;
            print(format, &r, || {
                println!(
                    "reset landing prompt of {} ({}) to the {} default",
                    r.path,
                    r.id,
                    r.merge_strategy.as_str()
                )
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

/// What `repo prompt reset` asks before it overwrites: whatever the landing
/// briefing says now is gone, so the question names the strategy default it
/// is about to be replaced by.
fn reset_prompt_question(r: &RepositoryDto, subject: &Subject) -> String {
    format!(
        "Reset the landing prompt of {} to the {} default?",
        subject.named(),
        r.merge_strategy.as_str()
    )
}

/// The `landing_prompt` field of a create or update: `--landing-prompt` text
/// as it was typed, `--landing-prompt-file`'s contents, or nothing when
/// neither flag was given — which is what leaves the repository on its
/// strategy's default. Clap keeps the two flags mutually exclusive, so at
/// most one of them ever carries a value.
fn read_landing_prompt(text: Option<String>, file: Option<PathBuf>) -> Result<Option<String>> {
    if let Some(text) = text {
        return Ok(Some(text));
    }
    let Some(file) = file else {
        return Ok(None);
    };
    Ok(Some(
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?,
    ))
}

/// The new landing prompt of `repo prompt set`: the file that was named, else
/// stdin. Whatever it holds is what gets sent — byte for byte, an empty file
/// included: the CLI is a pipe here and not a censor, and what goes in is
/// what `repo prompt get` prints back. The one refusal is a terminal on
/// stdin, where nobody piped anything in and reading it would hang on a
/// person.
fn read_content(file: Option<PathBuf>) -> Result<String> {
    let Some(file) = file else {
        if std::io::stdin().is_terminal() {
            bail!("no new text: pass --file <path>, or pipe the prompt in on stdin");
        }
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading the new landing prompt from stdin")?;
        return Ok(buf);
    };
    std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))
}

/// What the `landing_prompt` kv line of `repo inspect` says: the strategy's
/// own default names which strategy it is the default of, since a bare
/// "default" would leave the reader guessing; anything else is a text this
/// repository wrote for itself.
fn landing_status(r: &RepositoryDto) -> String {
    match r.landing_prompt_is_default {
        true => format!("default ({})", r.merge_strategy.as_str()),
        false => "custom".into(),
    }
}
