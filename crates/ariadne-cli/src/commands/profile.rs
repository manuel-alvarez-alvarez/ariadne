//! `ariadne profile ...`

use anyhow::{Context, Result};
use clap::Subcommand;

use ariadne_api::profiles::{CreateProfileRequest, ProfileDto, UpdateProfileRequest};
use ariadne_client::Client;
use ariadne_core::{AgentKind, Role};
use serde_json::json;

use crate::output::{
    Column, Format, UNCAPPED, local_time, note, print_json, print_kv, print_table,
};

/// Columns of `profile ls`.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("name", 32),
    ("role", UNCAPPED),
    ("agent", UNCAPPED),
    ("model", 28),
];

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// Create a profile
    Create {
        /// Profile name, unique: what every other command calls it by
        #[arg(long)]
        name: String,
        /// What this profile is spawned as
        #[arg(long, value_enum)]
        role: Role,
        /// claude_code | codex | opencode (omit for auto: first installed CLI)
        #[arg(long, value_parser = parse_agent, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_kinds))]
        agent: Option<AgentKind>,
        /// Model the agent CLI runs (default: the agent's own default)
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// Inline system prompt
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        /// Read the system prompt from a file
        #[arg(long)]
        prompt_file: Option<std::path::PathBuf>,
        /// Extra argv flag appended when spawning (repeatable)
        #[arg(long = "flag")]
        flags: Vec<String>,
    },
    /// List profiles
    Ls {
        /// Filter by role
        #[arg(long, value_enum)]
        role: Option<Role>,
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
    },
    /// Show a profile (by id or name)
    Inspect {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
    },
    /// Update a profile
    Update {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
        /// New profile name
        #[arg(long)]
        name: Option<String>,
        /// claude_code | codex | opencode, or "auto" to resolve the first
        /// installed CLI at spawn time
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_kinds_or_auto))]
        agent: Option<String>,
        /// Model name, or "default" to clear back to the agent's default
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// Replace the system prompt with this text
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        /// Replace the system prompt with the contents of this file
        #[arg(long)]
        prompt_file: Option<std::path::PathBuf>,
    },
    /// Delete a profile
    Rm {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
    },
}

fn parse_agent(s: &str) -> Result<AgentKind, String> {
    // Accept both claude_code and claude-code spellings.
    s.replace('-', "_").parse()
}

pub async fn run(client: &Client, cmd: ProfileCommand, format: Format) -> Result<()> {
    match cmd {
        ProfileCommand::Create {
            name,
            role,
            agent,
            model,
            prompt,
            prompt_file,
            flags,
        } => {
            let system_prompt = read_prompt(prompt, prompt_file)?;
            let profile: ProfileDto = client
                .post_json(
                    "/v1/profiles",
                    &CreateProfileRequest {
                        name,
                        role,
                        agent_kind: agent,
                        model,
                        system_prompt,
                        extra_flags: flags,
                    },
                )
                .await?;
            match format {
                Format::Json => print_json(&profile)?,
                Format::Table => println!("{}", profile.id),
            }
        }
        ProfileCommand::Ls { role, no_trunc } => {
            let path = match role {
                Some(r) => format!("/v1/profiles?role={}", r.as_str()),
                None => "/v1/profiles".to_string(),
            };
            let profiles: Vec<ProfileDto> = client.get_json(&path).await?;
            match format {
                Format::Json => print_json(&profiles)?,
                Format::Table => {
                    print_table(
                        LS,
                        &profiles
                            .iter()
                            .map(|p| {
                                vec![
                                    p.id.clone(),
                                    p.name.clone(),
                                    p.role.as_str().into(),
                                    p.agent_kind.map_or("auto", |k| k.as_str()).into(),
                                    p.model.clone().unwrap_or_else(|| "-".into()),
                                ]
                            })
                            .collect::<Vec<_>>(),
                        no_trunc,
                    );
                    if profiles.is_empty() {
                        note("no profiles yet — create one with: ariadne profile create");
                    }
                }
            }
        }
        ProfileCommand::Inspect { id } => {
            let p: ProfileDto = client.get_json(&format!("/v1/profiles/{id}")).await?;
            match format {
                Format::Json => print_json(&p)?,
                Format::Table => print_kv(&[
                    ("id", p.id),
                    ("name", p.name),
                    ("role", p.role.as_str().into()),
                    ("agent", p.agent_kind.map_or("auto", |k| k.as_str()).into()),
                    ("model", p.model.unwrap_or_else(|| "-".into())),
                    ("flags", format!("{:?}", p.extra_flags)),
                    ("created", local_time(&p.created_at)),
                    ("prompt", format!("\n---\n{}", p.system_prompt)),
                ]),
            }
        }
        ProfileCommand::Update {
            id,
            name,
            agent,
            model,
            prompt,
            prompt_file,
        } => {
            let system_prompt = match (prompt, prompt_file) {
                (None, None) => None,
                (p, f) => Some(read_prompt(p, f)?),
            };
            let p: ProfileDto = client
                .put_json(
                    &format!("/v1/profiles/{id}"),
                    &UpdateProfileRequest {
                        name,
                        // Accept dash spelling too (claude-code).
                        agent_kind: agent.map(|a| a.replace('-', "_")),
                        model,
                        system_prompt,
                        extra_flags: None,
                    },
                )
                .await?;
            match format {
                Format::Json => print_json(&p)?,
                Format::Table => println!("{}", p.id),
            }
        }
        ProfileCommand::Rm { id } => {
            client
                .send_no_content::<()>(http::Method::DELETE, &format!("/v1/profiles/{id}"), None)
                .await?;
            match format {
                // The profile is gone, so there is no DTO left to print: what
                // the caller asked about, and that it happened.
                Format::Json => print_json(&json!({"profile": id, "deleted": true}))?,
                Format::Table => println!("deleted {id}"),
            }
        }
    }
    Ok(())
}

fn read_prompt(prompt: Option<String>, file: Option<std::path::PathBuf>) -> Result<String> {
    match (prompt, file) {
        (Some(p), _) => Ok(p),
        (None, Some(f)) => {
            std::fs::read_to_string(&f).with_context(|| format!("reading {}", f.display()))
        }
        (None, None) => anyhow::bail!("provide --prompt or --prompt-file"),
    }
}
