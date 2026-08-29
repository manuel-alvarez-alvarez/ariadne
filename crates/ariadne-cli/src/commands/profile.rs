//! `ariadne profile ...`

mod flags;
mod prompts;

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use ariadne_api::profiles::{CreateProfileRequest, ProfileDto, UpdateProfileRequest};
use ariadne_client::{Client, ClientError};
use ariadne_core::Role;

use super::resolve::{self, Kind};
use super::{
    Subject, confirm, parse_effort, parse_effort_or_default, parse_model, parse_model_or_default,
    path_segment,
};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, UNCAPPED, age, col, moment, note, print, print_kv, print_list,
};

pub use flags::{PromptAssignment, read_prompts};
use flags::{owned_prompts, split_system, write_briefings};
use prompts::{Owner, PromptArg, parse_prompt_arg};

/// Columns of `profile ls`. A profile is its name and its role; how old it is
/// is the least of what one asks a profile, and the effort goes before the
/// model it belongs to — on a narrow terminal, what an agent runs on is the
/// half worth keeping.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("name", 32).title(),
    col("role", UNCAPPED).rank(4),
    col("model", 32).rank(3),
    col("effort", UNCAPPED).rank(2),
    col("age", UNCAPPED).rank(1),
];

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// Create a profile
    ///
    /// Prompts are set by kind: `--prompt <kind>=<text>` for text on the
    /// command line, `--prompt-file <kind>=<path>` to read one from a file.
    /// Both are repeatable and take each kind once. `<kind>` is `system` for
    /// the profile's own system prompt, or one of the briefings its role owns
    /// (planner: planner-briefing; engineer: engineer-briefing,
    /// changes-requested, landing-direct, landing-pull-request; reviewer:
    /// reviewer-briefing, reviewer-resume). Whatever is not given starts as the role default.
    ///
    /// A briefing may use only the `{placeholder}` tokens its kind fills in;
    /// one that names another is refused, with the allowed ones listed.
    ///
    /// `ariadne profile prompt` is the other half of the story: it prints,
    /// pipes and resets the prompts of a profile that already exists.
    Create {
        /// Profile name, unique: what every other command calls it by
        #[arg(long)]
        name: String,
        /// What this profile is spawned as
        #[arg(long, value_parser = Spelling::<Role>::new())]
        role: Role,
        /// What this profile runs on: AGENT[:MODEL] — an agent CLI
        /// (claude_code | codex | opencode) on its own default model, or one
        /// model of it after the colon (codex:gpt-5.3-codex). Omit for auto:
        /// the first installed CLI, resolved at spawn time
        #[arg(long, value_name = "MODEL", value_parser = parse_model, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// The reasoning effort that model is run at: one of the efforts
        /// `ariadne models ls` lists for it. Omit to run it at whatever the
        /// agent CLI runs it at
        #[arg(long, value_name = "EFFORT", value_parser = parse_effort, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::efforts))]
        effort: Option<String>,
        /// Set one prompt from the command line: <kind>=<text>, repeatable
        #[arg(long = "prompt", value_name = "KIND=TEXT", value_parser = flags::parse_prompt_text, add = clap_complete::engine::ArgValueCompleter::new(crate::complete::prompt_assignment))]
        prompts: Vec<PromptAssignment>,
        /// Set one prompt from a file: <kind>=<path>, repeatable
        #[arg(long = "prompt-file", value_name = "KIND=PATH", value_parser = flags::parse_prompt_file, add = clap_complete::engine::ArgValueCompleter::new(crate::complete::prompt_file_assignment))]
        prompt_files: Vec<PromptAssignment>,
    },
    /// List profiles
    Ls {
        /// Filter by role
        #[arg(long, value_parser = Spelling::<Role>::new())]
        role: Option<Role>,
    },
    /// Show a profile (by id or name)
    Inspect {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
    },
    /// Update a profile
    ///
    /// Prompts are replaced by kind, with the same `--prompt <kind>=<text>`
    /// and `--prompt-file <kind>=<path>` flags `profile create` takes:
    /// `system` for the profile's own system prompt, or one of the briefings
    /// its role owns (planner: planner-briefing; engineer: engineer-briefing,
    /// changes-requested, landing-direct, landing-pull-request; reviewer:
    /// reviewer-briefing, reviewer-resume). Both are repeatable and take each kind once; a prompt
    /// nobody names is left exactly as it is.
    ///
    /// A briefing may use only the `{placeholder}` tokens its kind fills in;
    /// one that names another is refused, with the allowed ones listed.
    ///
    /// `ariadne profile prompt` is the other half of the story: it prints,
    /// pipes and resets the prompts a profile already has.
    Update {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
        /// New profile name
        #[arg(long)]
        name: Option<String>,
        /// What this profile runs on: AGENT[:MODEL] — an agent CLI
        /// (claude_code | codex | opencode) on its own default model, or one
        /// model of it after the colon (codex:gpt-5.3-codex); "default" puts
        /// it back on auto, the first installed CLI at spawn time
        #[arg(long, value_name = "MODEL|default", value_parser = parse_model_or_default, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models_or_default))]
        model: Option<String>,
        /// The reasoning effort that model is run at: one of the efforts
        /// `ariadne models ls` lists for it; "default" runs it at whatever
        /// the agent CLI runs it at
        #[arg(long, value_name = "EFFORT|default", value_parser = parse_effort_or_default, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::efforts_or_default))]
        effort: Option<String>,
        /// Replace one prompt with this text: <kind>=<text>, repeatable
        #[arg(long = "prompt", value_name = "KIND=TEXT", value_parser = flags::parse_prompt_text, add = clap_complete::engine::ArgValueCompleter::new(crate::complete::prompt_assignment))]
        prompts: Vec<PromptAssignment>,
        /// Replace one prompt with a file's contents: <kind>=<path>, repeatable
        #[arg(long = "prompt-file", value_name = "KIND=PATH", value_parser = flags::parse_prompt_file, add = clap_complete::engine::ArgValueCompleter::new(crate::complete::prompt_file_assignment))]
        prompt_files: Vec<PromptAssignment>,
    },
    /// Delete a profile
    Rm {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// List a profile's prompts (contents in --format json)
    Prompts {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
    },
    /// Show, pipe out or reset one prompt (set them with create|update
    /// --prompt)
    Prompt {
        #[command(subcommand)]
        command: PromptCommand,
    },
}

/// The kind is one of the prompts the profile's role owns (planner:
/// `planner-briefing`; engineer: `engineer-briefing`, `changes-requested`,
/// `landing-direct`, `landing-pull-request`; reviewer: `reviewer-briefing`,
/// `reviewer-resume`), or `system` for the profile's own system prompt.
#[derive(Subcommand)]
pub enum PromptCommand {
    /// Print a prompt's content raw, ready to be piped to a file
    Get {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
        /// Prompt kind, or "system"
        #[arg(value_parser = parse_prompt_arg, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::prompt_kinds))]
        kind: PromptArg,
    },
    /// Replace a prompt with the contents of a file, or with stdin
    Set {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
        /// Prompt kind, or "system"
        #[arg(value_parser = parse_prompt_arg, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::prompt_kinds))]
        kind: PromptArg,
        /// Read the new text from this file (default: stdin)
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Put a prompt back to the default of the profile's role
    Reset {
        /// Profile id or name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::profile_names))]
        id: String,
        /// Prompt kind, or "system" (omit with --all)
        #[arg(value_parser = parse_prompt_arg, required_unless_present = "all", conflicts_with = "all", add = clap_complete::engine::ArgValueCandidates::new(crate::complete::prompt_kinds))]
        kind: Option<PromptArg>,
        /// Reset every prompt the profile owns, its system prompt included
        #[arg(long)]
        all: bool,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

pub async fn run(client: &Client, cmd: ProfileCommand, format: Format) -> Result<()> {
    match cmd {
        ProfileCommand::Create {
            name,
            role,
            model,
            effort,
            prompts,
            prompt_files,
        } => {
            let given = read_prompts(prompts, prompt_files)?;
            let (system_prompt, briefings) = split_system(owned_prompts(Owner::Role(role), given)?);
            // A prompt nobody named is not sent at all: what the profile then
            // runs on is the default of its role, which is where it stays
            // until somebody writes one.
            let profile: ProfileDto = client
                .post_json(
                    "/v1/profiles",
                    &CreateProfileRequest {
                        name,
                        role,
                        model,
                        effort,
                        system_prompt,
                    },
                )
                .await?;
            let written = write_briefings(client, &profile, briefings, false).await?;
            print_written(&profile, &written, format)?;
        }
        ProfileCommand::Ls { role } => {
            let path = match role {
                Some(r) => format!("/v1/profiles?role={}", r.as_str()),
                None => "/v1/profiles".to_string(),
            };
            let profiles: Vec<ProfileDto> = client.get_json(&path).await?;
            let now = chrono::Utc::now();
            print_list(
                format,
                &profiles,
                LS,
                |p| {
                    vec![
                        p.id.clone(),
                        p.name.clone(),
                        p.role.as_str().into(),
                        model_label(p.model.as_deref()),
                        effort_label(p.effort.as_deref()),
                        age(&p.created_at, now),
                    ]
                },
                "no profiles yet — create one with: ariadne profile create",
            )?;
        }
        ProfileCommand::Inspect { id } => {
            let p = get_profile(client, &id).await?;
            print(format, &p, || {
                print_kv(&[
                    ("id", p.id.clone()),
                    ("name", p.name.clone()),
                    ("role", p.role.as_str().into()),
                    ("model", model_label(p.model.as_deref())),
                    ("effort", effort_label(p.effort.as_deref())),
                    ("created", moment(&p.created_at)),
                    ("prompt", format!("\n---\n{}", p.system_prompt)),
                ])
            })?;
        }
        ProfileCommand::Update {
            id,
            name,
            model,
            effort,
            prompts,
            prompt_files,
        } => {
            // Everything that can be settled without the daemon is settled
            // first, so a line that repeats a kind or names an unreadable
            // file sends nothing. What the profile is — which id the name or
            // short id names, and which prompts its role owns — does take a
            // GET, still before any write.
            let given = read_prompts(prompts, prompt_files)?;
            let profile = get_profile(client, &id).await?;
            let given = match given.is_empty() {
                true => given,
                false => owned_prompts(Owner::Profile(&profile), given)?,
            };
            let (system_prompt, briefings) = split_system(given);
            let system = system_prompt.is_some();
            let p: ProfileDto = client
                .put_json(
                    &profile_path(&profile.id),
                    &UpdateProfileRequest {
                        name,
                        model,
                        effort,
                        system_prompt,
                    },
                )
                .await?;
            let written = write_briefings(client, &p, briefings, system).await?;
            print_written(&p, &written, format)?;
        }
        ProfileCommand::Rm { id, yes } => {
            let p = get_profile(client, &id).await?;
            let subject = Subject::new("profile", &p.name, &p.id);
            confirm("delete", &subject, &rm_question(&p, &subject), yes)?;
            client
                .send_no_content::<()>(http::Method::DELETE, &profile_path(&p.id), None)
                .await?;
            // The profile is gone, so there is no DTO left to print: what the
            // caller asked about, and that it happened.
            let id = p.id;
            print(format, &json!({"profile": id, "deleted": true}), || {
                println!("deleted {id}")
            })?;
        }
        ProfileCommand::Prompts { id } => prompts::list(client, &id, format).await?,
        ProfileCommand::Prompt { command } => prompts::run(client, command, format).await?,
    }
    Ok(())
}

/// A profile by id or name — the lookup every prompt command starts with: it
/// is what tells a kind from a kind of another role, and what puts a name
/// beside the id in the output.
///
/// A name and a whole id are what `/v1/profiles/{id}` itself resolves, so
/// they are asked outright and cost the one request they always did; only
/// something it does not know is looked for among the profiles as the short
/// id it may be.
async fn get_profile(client: &Client, id: &str) -> Result<ProfileDto> {
    match client.get_json(&profile_path(id)).await {
        Ok(profile) => Ok(profile),
        Err(ClientError::Api { status, .. }) if status == http::StatusCode::NOT_FOUND => {
            let id = resolve::id(client, Kind::Profile, id).await?;
            Ok(client.get_json(&profile_path(&id)).await?)
        }
        Err(e) => Err(e.into()),
    }
}

/// The endpoint of one profile, named the way the caller named it: by id, or
/// by a name that may have anything in it — a space, in the case of a profile
/// someone named `My Reviewer`. See [`path_segment`].
fn profile_path(id_or_name: &str) -> String {
    format!("/v1/profiles/{}", path_segment(id_or_name))
}

/// What a profile runs on, for the one column that shows it: the whole
/// `<agent_kind>[:<model>]` it is pinned to, which is what `--model` takes
/// back. Nothing pinned is `auto` — the first installed CLI, resolved at spawn
/// time, on its own default model.
fn model_label(model: Option<&str>) -> String {
    model.unwrap_or("auto").to_string()
}

/// The effort that model is reasoned at, for the cell beside it: one of the
/// efforts `ariadne models ls` lists for the model, which is what `--effort`
/// takes back. Nothing pinned is `auto` — whatever the agent CLI runs the
/// model at when nothing is passed.
fn effort_label(effort: Option<&str>) -> String {
    effort.unwrap_or("auto").to_string()
}

/// What `profile create` and `profile update` answer with: the profile, and —
/// for a person — the prompts written along with it.
fn print_written(p: &ProfileDto, written: &[&str], format: Format) -> Result<()> {
    print(format, p, || {
        println!("{}", p.id);
        if !written.is_empty() {
            note(&format!("set {}", written.join(", ")));
        }
    })
}

/// What `profile rm` asks before it deletes: the id alone does not say which
/// agent setup is about to go, so the question names the profile and its role.
fn rm_question(p: &ProfileDto, subject: &Subject) -> String {
    format!(
        "Delete the {} profile {}?",
        p.role.as_str(),
        subject.named()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A daemon that counts what reaches it and answers everything with an
    /// empty 500: enough to tell a command that sent a request from one that
    /// decided it had nothing to ask.
    async fn counting_daemon() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let endpoint = format!("http://{}", listener.local_addr().expect("addr"));
        let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = socket
                    .write_all(b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\n\r\n")
                    .await;
            }
        });
        (endpoint, seen)
    }

    fn update(prompts: Vec<PromptAssignment>, files: Vec<PromptAssignment>) -> ProfileCommand {
        ProfileCommand::Update {
            id: "Engineer".into(),
            name: None,
            model: None,
            effort: None,
            prompts,
            prompt_files: files,
        }
    }

    fn assignment(kind: &str, value: &str) -> PromptAssignment {
        flags::parse_prompt_text(&format!("{kind}={value}")).expect("a kind")
    }

    /// The whole point of checking the flags before the profile is fetched:
    /// a line that cannot be right sends nothing at all, not even the GET
    /// that would tell which prompts the profile has.
    #[tokio::test]
    async fn a_duplicate_kind_on_an_update_sends_no_request() {
        let (endpoint, seen) = counting_daemon().await;
        let client = Client::resolve(Some(&endpoint), None);
        let err = run(
            &client,
            update(
                vec![assignment("system", "a")],
                vec![assignment("system", "/tmp/x.md")],
            ),
            Format::Table,
        )
        .await
        .expect_err("duplicate")
        .to_string();
        assert!(err.starts_with("system is set twice"), "{err}");
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0);

        // And the counter is not blind: the same update without the clash
        // does reach the daemon.
        let _ = run(
            &client,
            update(vec![assignment("system", "a")], vec![]),
            Format::Table,
        )
        .await;
        assert!(seen.load(std::sync::atomic::Ordering::SeqCst) > 0);
    }

    /// The question is the last thing between the caller and a deleted
    /// profile, so it says which one by name and role, not by the id typed.
    #[test]
    fn the_rm_question_names_the_profile_and_its_role() {
        let p = ProfileDto {
            id: "01m0prof0000000000000abcde".into(),
            ..super::super::fixtures::profile("Engineer", Role::Engineer)
        };
        let subject = Subject::new("profile", &p.name, &p.id);
        assert_eq!(
            rm_question(&p, &subject),
            "Delete the engineer profile \"Engineer\" (…000abcde)?"
        );
        // A profile pinned to nothing runs on the first installed CLI; one
        // that is pinned shows the whole string it was pinned with.
        assert_eq!(model_label(p.model.as_deref()), "auto");
        assert_eq!(model_label(Some("codex")), "codex", "codex's own default");
        assert_eq!(model_label(Some("codex:o3")), "codex:o3");
        // And the effort beside it reads the same way: `auto` is whatever the
        // agent CLI reasons that model at when nothing was pinned.
        assert_eq!(effort_label(p.effort.as_deref()), "auto");
        assert_eq!(effort_label(Some("xhigh")), "xhigh");
    }
}
