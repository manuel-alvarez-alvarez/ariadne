//! `ariadne profile ...`

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Subcommand;

use ariadne_api::profiles::{
    CreateProfileRequest, ProfileDto, ProfilePromptDto, UpdateProfileRequest,
};
use ariadne_client::Client;
use ariadne_core::{AgentKind, PromptKind, Role};
use serde_json::json;

use super::confirm;
use crate::output::{
    Column, Format, UNCAPPED, local_time, note, print_json, print_kv, print_table,
};
use crate::query::path_segment;

/// Columns of `profile ls`.
const LS: &[Column] = &[
    ("id", UNCAPPED),
    ("name", 32),
    ("role", UNCAPPED),
    ("agent", UNCAPPED),
    ("model", 28),
];

/// Columns of `profile prompts`.
const PROMPTS: &[Column] = &[
    ("kind", UNCAPPED),
    ("status", UNCAPPED),
    ("updated", UNCAPPED),
    ("content", 48),
];

/// How the system prompt is spelled on a command line.
const SYSTEM: &str = "system";

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// Create a profile
    ///
    /// Prompts are set by kind: `--prompt <kind>=<text>` for text on the
    /// command line, `--prompt-file <kind>=<path>` to read one from a file.
    /// Both are repeatable and take each kind once. `<kind>` is `system` for
    /// the profile's own system prompt, or one of the briefings its role owns
    /// (planner: planner_briefing; engineer: engineer_briefing,
    /// changes_requested, landing_direct, landing_pull_request; reviewer:
    /// reviewer_briefing, reviewer_resume). Whatever is not given starts as the role default.
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
        #[arg(long, value_enum)]
        role: Role,
        /// claude_code | codex | opencode (omit for auto: first installed CLI)
        #[arg(long, value_parser = parse_agent, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_kinds))]
        agent: Option<AgentKind>,
        /// Model the agent CLI runs (default: the agent's own default)
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// Set one prompt from the command line: <kind>=<text>, repeatable
        #[arg(long = "prompt", value_name = "KIND=TEXT", value_parser = parse_prompt_text, add = clap_complete::engine::ArgValueCompleter::new(crate::complete::prompt_assignment))]
        prompts: Vec<PromptAssignment>,
        /// Set one prompt from a file: <kind>=<path>, repeatable
        #[arg(long = "prompt-file", value_name = "KIND=PATH", value_parser = parse_prompt_file, add = clap_complete::engine::ArgValueCompleter::new(crate::complete::prompt_file_assignment))]
        prompt_files: Vec<PromptAssignment>,
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
    ///
    /// Prompts are replaced by kind, with the same `--prompt <kind>=<text>`
    /// and `--prompt-file <kind>=<path>` flags `profile create` takes:
    /// `system` for the profile's own system prompt, or one of the briefings
    /// its role owns (planner: planner_briefing; engineer: engineer_briefing,
    /// changes_requested, landing_direct, landing_pull_request; reviewer:
    /// reviewer_briefing, reviewer_resume). Both are repeatable and take each kind once; a prompt
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
        /// claude_code | codex | opencode, or "auto" to resolve the first
        /// installed CLI at spawn time
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::agent_kinds_or_auto))]
        agent: Option<String>,
        /// Model name, or "default" to clear back to the agent's default
        #[arg(long, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
        model: Option<String>,
        /// Replace one prompt with this text: <kind>=<text>, repeatable
        #[arg(long = "prompt", value_name = "KIND=TEXT", value_parser = parse_prompt_text, add = clap_complete::engine::ArgValueCompleter::new(crate::complete::prompt_assignment))]
        prompts: Vec<PromptAssignment>,
        /// Replace one prompt with a file's contents: <kind>=<path>, repeatable
        #[arg(long = "prompt-file", value_name = "KIND=PATH", value_parser = parse_prompt_file, add = clap_complete::engine::ArgValueCompleter::new(crate::complete::prompt_file_assignment))]
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
        /// Print cells in full instead of cutting them to the column width
        #[arg(long)]
        no_trunc: bool,
    },
    /// Show, pipe out or reset one prompt (set them with create|update
    /// --prompt)
    Prompt {
        #[command(subcommand)]
        command: PromptCommand,
    },
}

/// `ariadne profile prompt ...` — one prompt at a time.
///
/// The kind is one of the prompts the profile's role owns (planner:
/// `planner_briefing`; engineer: `engineer_briefing`, `changes_requested`,
/// `landing_direct`, `landing_pull_request`; reviewer: `reviewer_briefing`,
/// `reviewer_resume`), or `system` for the profile's own system prompt.
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

/// What a `<kind>` argument names: one of the briefings the profile's role
/// owns, or the profile's own system prompt.
///
/// The two live in different places — briefings in `profile_prompts`, the
/// system prompt on the profile row — and the daemon serves them under
/// different endpoints, but from the terminal they are one list of prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptArg {
    System,
    Briefing(PromptKind),
}

impl PromptArg {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            PromptArg::System => SYSTEM,
            PromptArg::Briefing(kind) => kind.as_str(),
        }
    }
}

fn parse_agent(s: &str) -> Result<AgentKind, String> {
    // Accept both claude_code and claude-code spellings.
    s.replace('-', "_").parse()
}

/// A `<kind>` argument, without knowing yet which profile it is for: whether
/// this role owns that prompt is decided by [`owned_by`], once the profile has
/// been fetched and its role can be named in the error.
fn parse_prompt_arg(s: &str) -> Result<PromptArg, String> {
    if s == SYSTEM {
        return Ok(PromptArg::System);
    }
    s.parse().map(PromptArg::Briefing).map_err(|_| {
        format!(
            "unknown prompt kind: {s} (expected one of {})",
            spelled(every_kind())
        )
    })
}

/// One `<kind>=<value>` flag: which prompt it is for, and the text it carries
/// — or, from `--prompt-file`, the path that text is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptAssignment {
    pub(crate) kind: PromptArg,
    pub(crate) value: String,
}

/// `--prompt <kind>=<text>`.
fn parse_prompt_text(s: &str) -> Result<PromptAssignment, String> {
    parse_assignment(s, "<text>")
}

/// `--prompt-file <kind>=<path>`. The file is read later, by
/// [`read_prompts`]: a path is a path whether or not it exists yet, and
/// nothing is read until the whole line is known to be good.
fn parse_prompt_file(s: &str) -> Result<PromptAssignment, String> {
    parse_assignment(s, "<path>")
}

/// A `<kind>=<value>` flag, split at the first `=` and its kind checked
/// against every kind there is. Whether the profile's own role owns it is
/// [`Owner::owns`]'s call — that one needs a role, and clap has none.
fn parse_assignment(s: &str, value: &str) -> Result<PromptAssignment, String> {
    let (kind, text) = s.split_once('=').ok_or_else(|| {
        format!(
            "missing <kind>=: write {SYSTEM}={value} to set the system prompt — the kinds are: {}",
            spelled(every_kind())
        )
    })?;
    Ok(PromptAssignment {
        kind: parse_prompt_arg(kind)?,
        value: text.to_string(),
    })
}

/// Every prompt kind there is, of every role, `system` first: what an error
/// lists when no profile has said which role it is about.
fn every_kind() -> impl Iterator<Item = PromptArg> {
    std::iter::once(PromptArg::System)
        .chain(PromptKind::ALL.iter().map(|k| PromptArg::Briefing(*k)))
}

/// The roles a prompt kind belongs to, as an error names them: one for every
/// kind that briefs a role through its own lifecycle, and all four for the
/// notice an addressed agent is woken with.
fn owners(kind: PromptKind) -> String {
    kind.roles()
        .iter()
        .map(|role| role.as_str())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every prompt a profile of `role` owns: its system prompt first — the one an
/// agent of any role always runs on — then the briefings in briefing order.
fn owned(role: Role) -> Vec<PromptArg> {
    std::iter::once(PromptArg::System)
        .chain(
            PromptKind::for_role(role)
                .iter()
                .map(|k| PromptArg::Briefing(*k)),
        )
        .collect()
}

/// Whose prompts a `<kind>` is about: a profile that already exists, or the
/// role of one `profile create` is about to make. Both know which kinds they
/// own; only one of them has a name to be pointed at in an error.
pub(crate) enum Owner<'a> {
    Role(Role),
    Profile(&'a ProfileDto),
}

impl Owner<'_> {
    fn role(&self) -> Role {
        match self {
            Owner::Role(role) => *role,
            Owner::Profile(p) => p.role,
        }
    }

    /// `arg` if these prompts include it, otherwise an error naming the ones
    /// they do: prompt kinds belong to exactly one role, and a reviewer
    /// profile asked for `engineer_briefing` has nothing to show.
    fn owns(&self, arg: PromptArg) -> Result<PromptArg> {
        let PromptArg::Briefing(kind) = arg else {
            // Whatever the role, it runs on a system prompt.
            return Ok(arg);
        };
        if kind.owned_by(self.role()) {
            return Ok(arg);
        }
        let (owner, kind) = (owners(kind), kind.as_str());
        let prompts = spelled(owned(self.role()).into_iter());
        match self {
            Owner::Role(role) => bail!(
                "{} profiles have no {kind} prompt ({owner} owns it) — their prompts are: {prompts}",
                role.as_str()
            ),
            Owner::Profile(p) => bail!(
                "{} ({}) is a {} profile and has no {kind} prompt ({owner} owns it) — its prompts are: {prompts}",
                p.name,
                p.id,
                p.role.as_str()
            ),
        }
    }
}

fn owned_by(profile: &ProfileDto, arg: PromptArg) -> Result<PromptArg> {
    Owner::Profile(profile).owns(arg)
}

/// Prompt kinds as a command line spells them, comma-separated.
fn spelled(args: impl Iterator<Item = PromptArg>) -> String {
    args.map(|a| a.as_str()).collect::<Vec<_>>().join(", ")
}

pub async fn run(client: &Client, cmd: ProfileCommand, format: Format) -> Result<()> {
    match cmd {
        ProfileCommand::Create {
            name,
            role,
            agent,
            model,
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
                        agent_kind: agent,
                        model,
                        system_prompt,
                    },
                )
                .await?;
            let written = write_briefings(client, &profile, briefings, false).await?;
            match format {
                Format::Json => print_json(&profile)?,
                Format::Table => {
                    println!("{}", profile.id);
                    if !written.is_empty() {
                        note(&format!("set {}", written.join(", ")));
                    }
                }
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
            let p: ProfileDto = client.get_json(&profile_path(&id)).await?;
            match format {
                Format::Json => print_json(&p)?,
                Format::Table => print_kv(&[
                    ("id", p.id),
                    ("name", p.name),
                    ("role", p.role.as_str().into()),
                    ("agent", p.agent_kind.map_or("auto", |k| k.as_str()).into()),
                    ("model", p.model.unwrap_or_else(|| "-".into())),
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
            prompts,
            prompt_files,
        } => {
            // Everything that can be settled without the daemon is settled
            // first, so a line that repeats a kind or names an unreadable
            // file sends nothing. Which prompts exist does depend on the
            // role, so that one check costs a GET — still before any write.
            let given = read_prompts(prompts, prompt_files)?;
            let given = match given.is_empty() {
                true => given,
                false => {
                    let profile = get_profile(client, &id).await?;
                    owned_prompts(Owner::Profile(&profile), given)?
                }
            };
            let (system_prompt, briefings) = split_system(given);
            let system = system_prompt.is_some();
            let p: ProfileDto = client
                .put_json(
                    &profile_path(&id),
                    &UpdateProfileRequest {
                        name,
                        // Accept dash spelling too (claude-code).
                        agent_kind: agent.map(|a| a.replace('-', "_")),
                        model,
                        system_prompt,
                    },
                )
                .await?;
            let written = write_briefings(client, &p, briefings, system).await?;
            match format {
                Format::Json => print_json(&p)?,
                Format::Table => {
                    println!("{}", p.id);
                    if !written.is_empty() {
                        note(&format!("set {}", written.join(", ")));
                    }
                }
            }
        }
        ProfileCommand::Rm { id, yes } => {
            let p = get_profile(client, &id).await?;
            confirm(&rm_question(&p), yes)?;
            client
                .send_no_content::<()>(http::Method::DELETE, &profile_path(&id), None)
                .await?;
            match format {
                // The profile is gone, so there is no DTO left to print: what
                // the caller asked about, and that it happened.
                Format::Json => print_json(&json!({"profile": id, "deleted": true}))?,
                Format::Table => println!("deleted {id}"),
            }
        }
        ProfileCommand::Prompts { id, no_trunc } => {
            let profile = get_profile(client, &id).await?;
            let prompts = prompt_set(client, &profile).await?;
            match format {
                // The whole text: a table shows a prompt is there and whether
                // it has been touched, json is how a script reads what it says.
                Format::Json => print_json(&prompts.iter().map(Prompt::json).collect::<Vec<_>>())?,
                Format::Table => print_table(
                    PROMPTS,
                    &prompts
                        .iter()
                        .map(|p| {
                            vec![
                                p.kind.as_str().into(),
                                p.status().into(),
                                p.updated(),
                                p.content.clone(),
                            ]
                        })
                        .collect::<Vec<_>>(),
                    no_trunc,
                ),
            }
        }
        ProfileCommand::Prompt { command } => match command {
            PromptCommand::Get { id, kind } => {
                let profile = get_profile(client, &id).await?;
                let prompt = fetch_prompt(client, &profile, owned_by(&profile, kind)?).await?;
                match format {
                    Format::Json => print_json(&prompt.json())?,
                    // Raw and unadorned, trailing newline included or not
                    // exactly as it is stored: `get > file` then `set --file`
                    // has to be a round trip.
                    Format::Table => print!("{}", prompt.content),
                }
            }
            PromptCommand::Set { id, kind, file } => {
                let profile = get_profile(client, &id).await?;
                let kind = owned_by(&profile, kind)?;
                let content = read_content(file)?;
                let prompt = write_prompt(client, &profile, kind, content).await?;
                match format {
                    Format::Json => print_json(&prompt.json())?,
                    Format::Table => println!(
                        "updated {} of {} ({})",
                        prompt.kind.as_str(),
                        profile.name,
                        profile.id
                    ),
                }
            }
            PromptCommand::Reset { id, kind, all, yes } => {
                let profile = get_profile(client, &id).await?;
                let kinds = match kind {
                    Some(kind) => vec![owned_by(&profile, kind)?],
                    // clap keeps this from happening: `kind` is required
                    // unless `--all` is there.
                    None => owned(profile.role),
                };
                confirm(&reset_question(&profile, &kinds, all), yes)?;
                let mut done = Vec::new();
                for kind in kinds {
                    done.push(reset_prompt(client, &profile, kind).await?);
                }
                match format {
                    // One kind was asked for, one object comes back; --all is
                    // the plural request, so it always answers with a list.
                    Format::Json if all => {
                        print_json(&done.iter().map(Prompt::json).collect::<Vec<_>>())?
                    }
                    Format::Json => print_json(&done[0].json())?,
                    Format::Table => {
                        for prompt in &done {
                            println!(
                                "reset {} of {} ({}) to the {} default",
                                prompt.kind.as_str(),
                                profile.name,
                                profile.id,
                                profile.role.as_str()
                            );
                        }
                    }
                }
            }
        },
    }
    Ok(())
}

/// One prompt as the CLI prints it, whichever half of the API it came from.
struct Prompt {
    kind: PromptArg,
    /// The text the profile is briefed with, which is the default of the kind
    /// while the profile holds none of its own.
    content: String,
    /// Whether that text is that default rather than one written here. The
    /// daemon says so; there is nothing to compare locally.
    is_default: bool,
    /// When the text written here was last saved; None while the default
    /// stands, which nothing dates.
    updated_at: Option<String>,
}

impl Prompt {
    /// The system prompt lives on the profile row, so it is dated by the
    /// profile itself — every other prompt carries its own timestamp.
    fn system(profile: &ProfileDto) -> Self {
        Self {
            kind: PromptArg::System,
            content: profile.system_prompt.clone(),
            is_default: profile.system_prompt_is_default,
            updated_at: (!profile.system_prompt_is_default).then(|| profile.updated_at.clone()),
        }
    }

    /// `default` or `customized`, for a table.
    fn status(&self) -> &'static str {
        match self.is_default {
            true => "default",
            false => "customized",
        }
    }

    /// The local time the prompt was written, or a dash for a default nothing
    /// wrote.
    fn updated(&self) -> String {
        self.updated_at
            .as_deref()
            .map_or_else(|| "-".to_string(), local_time)
    }

    fn json(&self) -> serde_json::Value {
        json!({
            "kind": self.kind.as_str(),
            "content": self.content,
            "is_default": self.is_default,
            "updated_at": self.updated_at,
        })
    }
}

impl From<ProfilePromptDto> for Prompt {
    fn from(dto: ProfilePromptDto) -> Self {
        Self {
            kind: PromptArg::Briefing(dto.kind),
            content: dto.content,
            is_default: dto.is_default,
            updated_at: dto.updated_at,
        }
    }
}

/// A profile by id or name — the lookup every prompt command starts with: it
/// is what tells a kind from a kind of another role, and what puts a name
/// beside the id in the output.
async fn get_profile(client: &Client, id: &str) -> Result<ProfileDto> {
    Ok(client.get_json(&profile_path(id)).await?)
}

/// The endpoint of one profile, named the way the caller named it: by id, or
/// by a name that may have anything in it — a space, in the case of a profile
/// someone named `My Reviewer`. See [`path_segment`].
fn profile_path(id_or_name: &str) -> String {
    format!("/v1/profiles/{}", path_segment(id_or_name))
}

/// Every prompt of a profile, in the order [`owned`] lists them.
async fn prompt_set(client: &Client, profile: &ProfileDto) -> Result<Vec<Prompt>> {
    let briefings = client.list_profile_prompts(&profile.id).await?;
    let mut out = vec![Prompt::system(profile)];
    out.extend(briefings.into_iter().map(Prompt::from));
    Ok(out)
}

async fn fetch_prompt(client: &Client, profile: &ProfileDto, kind: PromptArg) -> Result<Prompt> {
    match kind {
        PromptArg::System => Ok(Prompt::system(profile)),
        // Every kind of the role is listed, its default included, so the one
        // that was asked for is there.
        PromptArg::Briefing(k) => client
            .list_profile_prompts(&profile.id)
            .await?
            .into_iter()
            .find(|p| p.kind == k)
            .map(Prompt::from)
            .with_context(|| {
                format!(
                    "{} ({}) answered with no {} prompt",
                    profile.name,
                    profile.id,
                    k.as_str()
                )
            }),
    }
}

/// Set one prompt: a briefing goes to the prompts endpoint, the system prompt
/// to the profile it belongs to.
async fn write_prompt(
    client: &Client,
    profile: &ProfileDto,
    kind: PromptArg,
    content: String,
) -> Result<Prompt> {
    match kind {
        PromptArg::System => {
            let updated: ProfileDto = client
                .put_json(
                    &format!("/v1/profiles/{}", profile.id),
                    &UpdateProfileRequest {
                        system_prompt: Some(content),
                        ..Default::default()
                    },
                )
                .await?;
            Ok(Prompt::system(&updated))
        }
        PromptArg::Briefing(k) => Ok(client
            .update_profile_prompt(&profile.id, k, content)
            .await?
            .into()),
    }
}

async fn reset_prompt(client: &Client, profile: &ProfileDto, kind: PromptArg) -> Result<Prompt> {
    match kind {
        PromptArg::System => Ok(Prompt::system(
            &client.reset_system_prompt(&profile.id).await?,
        )),
        PromptArg::Briefing(k) => Ok(client.reset_profile_prompt(&profile.id, k).await?.into()),
    }
}

/// What `profile rm` asks before it deletes: the id alone does not say which
/// agent setup is about to go, so the question names the profile and its role.
fn rm_question(p: &ProfileDto) -> String {
    format!(
        "Delete the {} profile \"{}\" ({})?",
        p.role.as_str(),
        p.name,
        p.id
    )
}

/// What `prompt reset` asks before it overwrites: whatever the prompt says now
/// is gone, so the question names how much of the profile it is about to
/// replace.
fn reset_question(profile: &ProfileDto, kinds: &[PromptArg], all: bool) -> String {
    let (what, defaults) = match (all, kinds) {
        (false, [one]) => (format!("the {} prompt", one.as_str()), "default"),
        _ => (format!("all {} prompts", kinds.len()), "defaults"),
    };
    format!(
        "Reset {what} of {} ({}) to the {} {defaults}?",
        profile.name,
        profile.id,
        profile.role.as_str()
    )
}

/// The new text of a prompt: the file that was named, else stdin.
///
/// Whatever that holds is what gets sent — byte for byte, trailing newlines
/// and all, an empty file included: the CLI is a pipe here and not a censor,
/// and the text that goes in is the text `prompt get` prints back. What a
/// briefing may say is the daemon's call — dropping a placeholder is fine,
/// naming one its kind cannot fill in comes back as an error printed as it
/// was sent.
///
/// The one refusal is a terminal on stdin, where nobody piped anything in and
/// reading it would hang on a person who expected a prompt.
fn read_content(file: Option<PathBuf>) -> Result<String> {
    match file {
        Some(f) => std::fs::read_to_string(&f).with_context(|| format!("reading {}", f.display())),
        None => {
            if std::io::stdin().is_terminal() {
                bail!("no new text: pass --file <path>, or pipe the prompt in on stdin");
            }
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading the new prompt from stdin")?;
            Ok(buf)
        }
    }
}

/// The prompt flags of one command line, merged and read: `--prompt` and
/// `--prompt-file` are one list, each kind may be given once by either of
/// them, and the files are read here.
///
/// Nothing in this needs to know the role, so `create` and `update` both run
/// it before they ask the daemon anything: a line that repeats a kind or
/// names a file that will not read costs no request at all. What is left for
/// [`owned_prompts`] is the one check that does need a role.
pub(crate) fn read_prompts(
    texts: Vec<PromptAssignment>,
    files: Vec<PromptAssignment>,
) -> Result<Vec<(PromptArg, String)>> {
    let given = texts
        .into_iter()
        .map(|a| (a, false))
        .chain(files.into_iter().map(|a| (a, true)));
    let mut out: Vec<(PromptArg, String)> = Vec::new();
    for (assignment, from_file) in given {
        let kind = assignment.kind;
        if out.iter().any(|(k, _)| *k == kind) {
            bail!(
                "{} is set twice — --prompt and --prompt-file take each kind once",
                kind.as_str()
            );
        }
        let content = match from_file {
            true => std::fs::read_to_string(&assignment.value)
                .with_context(|| format!("reading {}", assignment.value))?,
            false => assignment.value,
        };
        out.push((kind, content));
    }
    Ok(out)
}

/// What [`read_prompts`] collected, checked against the prompts `owner` has
/// and put in the order it owns them — so what gets written, and what gets
/// reported, does not depend on the order the flags were typed in.
///
/// A briefing is also checked against the `{placeholder}`s its kind can fill
/// in, with the very function the daemon refuses a save with: `profile create`
/// writes its briefings after the profile exists, and a template that cannot
/// be saved must not leave one created behind it.
pub(crate) fn owned_prompts(
    owner: Owner<'_>,
    given: Vec<(PromptArg, String)>,
) -> Result<Vec<(PromptArg, String)>> {
    let mut out = Vec::with_capacity(given.len());
    for (kind, content) in given {
        let kind = owner.owns(kind)?;
        if let PromptArg::Briefing(k) = kind {
            k.validate_template(&content)
                .with_context(|| format!("the {} prompt", k.as_str()))?;
        }
        out.push((kind, content));
    }
    let order = owned(owner.role());
    out.sort_by_key(|(kind, _)| order.iter().position(|k| k == kind).unwrap_or(usize::MAX));
    Ok(out)
}

/// A collected list split the way the API takes it: the system prompt, which
/// travels with the profile itself, apart from the briefings, which do not.
fn split_system(given: Vec<(PromptArg, String)>) -> (Option<String>, Vec<(PromptKind, String)>) {
    let mut system = None;
    let mut briefings = Vec::new();
    for (kind, content) in given {
        match kind {
            PromptArg::System => system = Some(content),
            PromptArg::Briefing(k) => briefings.push((k, content)),
        }
    }
    (system, briefings)
}

/// The briefings of a `profile create` or `profile update`, one PUT each,
/// after the profile itself has been written. Answers with everything that was
/// written, the system prompt included when the profile body carried one.
///
/// A write that fails stops the rest: the profile is already part-way
/// changed, so the error says which prompt failed and which ones stand,
/// with the daemon's own sentence for why.
async fn write_briefings(
    client: &Client,
    profile: &ProfileDto,
    briefings: Vec<(PromptKind, String)>,
    system: bool,
) -> Result<Vec<&'static str>> {
    let mut written: Vec<&'static str> = system.then_some(SYSTEM).into_iter().collect();
    for (kind, content) in briefings {
        if let Err(e) = client
            .update_profile_prompt(&profile.id, kind, content)
            .await
        {
            bail!(
                "writing the {} prompt of {} ({}) failed{}: {}",
                kind.as_str(),
                profile.name,
                profile.id,
                match written.is_empty() {
                    true => String::new(),
                    false => format!(" (already written: {})", written.join(", ")),
                },
                e.human()
            );
        }
        written.push(kind.as_str());
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(role: Role) -> ProfileDto {
        ProfileDto {
            id: "01PROFILE".into(),
            name: "Engineer".into(),
            role,
            agent_kind: None,
            model: None,
            system_prompt: "you are an engineer".into(),
            system_prompt_is_default: false,
            created_at: "2026-08-17T08:00:00Z".into(),
            updated_at: "2026-08-17T09:00:00Z".into(),
        }
    }

    fn kinds(role: Role) -> Vec<&'static str> {
        owned(role).iter().map(|a| a.as_str()).collect()
    }

    /// A `--prompt`/`--prompt-file` flag as clap would have parsed it.
    fn assignment(kind: &str, value: &str) -> PromptAssignment {
        PromptAssignment {
            kind: parse_prompt_arg(kind).expect("a kind"),
            value: value.to_string(),
        }
    }

    #[test]
    fn a_profile_owns_its_system_prompt_and_the_briefings_of_its_role() {
        assert_eq!(
            kinds(Role::Planner),
            [
                "system",
                "planner_briefing",
                "planner_resume",
                "message_delivery"
            ]
        );
        assert_eq!(
            kinds(Role::Engineer),
            [
                "system",
                "engineer_briefing",
                "engineer_resume",
                "changes_requested",
                "landing_direct",
                "landing_pull_request",
                "message_delivery"
            ]
        );
        assert_eq!(
            kinds(Role::Reviewer),
            [
                "system",
                "reviewer_briefing",
                "reviewer_resume",
                "message_delivery"
            ]
        );
    }

    #[test]
    fn a_kind_is_spelled_as_the_daemon_spells_it_plus_system() {
        assert_eq!(parse_prompt_arg("system"), Ok(PromptArg::System));
        assert_eq!(
            parse_prompt_arg("changes_requested"),
            Ok(PromptArg::Briefing(PromptKind::ChangesRequested))
        );
    }

    /// A typo must not send the caller to `--help` to find the spelling.
    #[test]
    fn an_unknown_kind_lists_the_kinds_that_exist() {
        let err = parse_prompt_arg("engineer-briefing").expect_err("unknown");
        assert!(
            err.starts_with("unknown prompt kind: engineer-briefing"),
            "{err}"
        );
        for expected in ["system", "engineer_briefing", "reviewer_resume"] {
            assert!(err.contains(expected), "{err}");
        }
    }

    /// The kind exists, but not for this profile: the message has to say which
    /// prompts this one has, since the CLI knows and the caller does not.
    #[test]
    fn a_kind_of_another_role_names_the_prompts_this_profile_has() {
        let err = owned_by(
            &profile(Role::Reviewer),
            PromptArg::Briefing(PromptKind::EngineerBriefing),
        )
        .expect_err("wrong role");
        let err = err.to_string();
        assert!(err.contains("is a reviewer profile"), "{err}");
        assert!(err.contains("engineer_briefing"), "{err}");
        assert!(
            err.contains("its prompts are: system, reviewer_briefing, reviewer_resume"),
            "{err}"
        );
    }

    #[test]
    fn every_role_owns_a_system_prompt() {
        for role in Role::ALL {
            assert_eq!(
                owned_by(&profile(role), PromptArg::System).expect("system"),
                PromptArg::System
            );
        }
    }

    #[test]
    fn the_role_briefings_are_owned_by_their_own_role() {
        for kind in PromptKind::ALL {
            let arg = PromptArg::Briefing(kind);
            for role in kind.roles() {
                assert!(owned_by(&profile(*role), arg).is_ok(), "{kind:?}");
            }
        }
    }

    #[test]
    fn the_reset_question_names_the_one_prompt_it_is_about_to_replace() {
        let q = reset_question(
            &profile(Role::Engineer),
            &[PromptArg::Briefing(PromptKind::ChangesRequested)],
            false,
        );
        assert_eq!(
            q,
            "Reset the changes_requested prompt of Engineer (01PROFILE) to the engineer default?"
        );
    }

    /// `--all` is the plural request, so the question counts them — including
    /// the system prompt it takes with it.
    #[test]
    fn the_reset_all_question_counts_every_prompt_it_takes() {
        let p = profile(Role::Engineer);
        let q = reset_question(&p, &owned(p.role), true);
        assert_eq!(
            q,
            "Reset all 7 prompts of Engineer (01PROFILE) to the engineer defaults?"
        );
    }

    /// A briefing naming a `{placeholder}` its kind cannot fill in never
    /// reaches the daemon: `profile create` writes its briefings after the
    /// profile exists, so the refusal has to come before anything is created.
    #[test]
    fn a_placeholder_the_kind_cannot_fill_in_is_refused_before_anything_is_sent() {
        let err = owned_prompts(
            Owner::Role(Role::Engineer),
            read_prompts(
                vec![assignment("changes_requested", "Fix {feedbcak}.")],
                vec![],
            )
            .expect("read"),
        )
        .expect_err("unfillable");
        let err = format!("{err:#}");
        assert!(err.contains("the changes_requested prompt"), "{err}");
        assert!(
            err.contains("{feedbcak}") && err.contains("{feedback}"),
            "{err}"
        );
    }

    /// The flags are one list: text and files together, each kind once, in
    /// the order the profile owns them rather than the order they were typed.
    #[test]
    fn the_prompt_flags_are_collected_in_the_order_the_profile_owns_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("changes.md");
        std::fs::write(&path, "fix it\n").expect("write");
        let collected = owned_prompts(
            Owner::Role(Role::Engineer),
            read_prompts(
                vec![
                    assignment("engineer_briefing", "brief"),
                    assignment("system", "you are"),
                ],
                vec![assignment("changes_requested", &path.display().to_string())],
            )
            .expect("read"),
        )
        .expect("collected");
        assert_eq!(
            collected,
            [
                (PromptArg::System, "you are".to_string()),
                (
                    PromptArg::Briefing(PromptKind::EngineerBriefing),
                    "brief".to_string()
                ),
                (
                    PromptArg::Briefing(PromptKind::ChangesRequested),
                    "fix it\n".to_string()
                ),
            ]
        );
    }

    /// Two values for one prompt is nobody's intention, whichever flags spell
    /// it — and it is caught before a single request goes out.
    #[test]
    fn a_kind_given_twice_is_refused() {
        let twice = |texts: Vec<PromptAssignment>, files: Vec<PromptAssignment>| {
            read_prompts(texts, files)
                .expect_err("duplicate")
                .to_string()
        };
        for err in [
            twice(
                vec![assignment("system", "a"), assignment("system", "b")],
                vec![],
            ),
            twice(
                vec![assignment("system", "a")],
                vec![assignment("system", "/tmp/x.md")],
            ),
        ] {
            assert!(err.starts_with("system is set twice"), "{err}");
            assert!(err.contains("--prompt-file"), "{err}");
        }
    }

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
            agent: None,
            model: None,
            prompts,
            prompt_files: files,
        }
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

    /// A file only matters once the rest of the line is good: the kind is
    /// wrong here, so nothing is read and nothing is sent.
    #[test]
    fn a_kind_of_another_role_stops_a_create_line() {
        let err = owned_prompts(
            Owner::Role(Role::Engineer),
            read_prompts(vec![assignment("planner_briefing", "plan")], vec![]).expect("read"),
        )
        .expect_err("wrong role")
        .to_string();
        assert!(
            err.starts_with("engineer profiles have no planner_briefing prompt"),
            "{err}"
        );
        assert!(err.contains("(planner owns it)"), "{err}");
        assert!(
            err.contains(
                "their prompts are: system, engineer_briefing, engineer_resume, changes_requested"
            ),
            "{err}"
        );
    }

    #[test]
    fn a_prompt_file_that_is_not_there_says_which_one() {
        let err = read_prompts(
            vec![],
            vec![assignment("engineer_briefing", "/no/such/brief.md")],
        )
        .expect_err("missing")
        .to_string();
        assert!(err.contains("/no/such/brief.md"), "{err}");
    }

    /// The system prompt rides along with the profile; the briefings do not.
    #[test]
    fn the_system_prompt_is_split_off_from_the_briefings() {
        let (system, briefings) = split_system(vec![
            (PromptArg::System, "you are".into()),
            (
                PromptArg::Briefing(PromptKind::EngineerBriefing),
                "brief".into(),
            ),
        ]);
        assert_eq!(system.as_deref(), Some("you are"));
        assert_eq!(
            briefings,
            [(PromptKind::EngineerBriefing, "brief".to_string())]
        );
    }

    #[test]
    fn a_line_with_no_prompt_flags_sets_no_prompts() {
        let (system, briefings) = split_system(
            owned_prompts(
                Owner::Role(Role::Planner),
                read_prompts(vec![], vec![]).expect("none"),
            )
            .expect("none"),
        );
        assert_eq!(system, None);
        assert!(briefings.is_empty());
    }

    /// `prompt get > file` then `prompt set --file file` has to round-trip, so
    /// nothing is trimmed, wrapped or added on the way in.
    #[test]
    fn a_file_is_read_byte_for_byte() {
        let raw = "  Brief the agent.\n\nEnd.\n";
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("prompt.md");
        std::fs::write(&path, raw).expect("write");
        assert_eq!(read_content(Some(path)).expect("content"), raw);
    }

    /// The daemon takes any content, so emptying a prompt is the caller's call
    /// to make and not the CLI's to refuse.
    #[test]
    fn an_empty_file_empties_the_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.md");
        std::fs::write(&path, "").expect("write");
        assert_eq!(read_content(Some(path)).expect("content"), "");
    }

    #[test]
    fn a_file_that_is_not_there_says_which_one() {
        let err = read_content(Some("/no/such/prompt.md".into())).expect_err("missing");
        assert!(err.to_string().contains("/no/such/prompt.md"), "{err}");
    }

    /// Whether a prompt is the default is the daemon's word, not a comparison
    /// made here: the CLI prints the flag it was sent, for the system prompt
    /// as for a briefing.
    #[test]
    fn the_status_of_a_prompt_is_the_flag_the_daemon_sent() {
        let mut p = profile(Role::Engineer);
        assert_eq!(Prompt::system(&p).status(), "customized");
        p.system_prompt_is_default = true;
        assert_eq!(Prompt::system(&p).status(), "default");

        let briefing: Prompt = ProfilePromptDto {
            kind: PromptKind::EngineerBriefing,
            content: "brief {task_title}".into(),
            is_default: true,
            updated_at: None,
        }
        .into();
        assert_eq!(briefing.status(), "default");
    }

    /// A default was written by nobody, so there is no date to print for it —
    /// the system prompt's own date is the profile's, and goes the same way.
    #[test]
    fn a_default_has_no_timestamp_to_show() {
        let mut p = profile(Role::Engineer);
        assert_eq!(
            Prompt::system(&p).updated_at.as_deref(),
            Some(p.updated_at.as_str())
        );
        p.system_prompt_is_default = true;
        assert_eq!(Prompt::system(&p).updated(), "-");
    }

    #[test]
    fn json_output_carries_the_kind_the_content_the_status_and_the_timestamp() {
        let p = profile(Role::Engineer);
        assert_eq!(
            Prompt::system(&p).json(),
            json!({
                "kind": "system",
                "content": "you are an engineer",
                "is_default": false,
                "updated_at": "2026-08-17T09:00:00Z",
            })
        );
        let briefing: Prompt = ProfilePromptDto {
            kind: PromptKind::EngineerBriefing,
            content: "brief {task_title}".into(),
            is_default: true,
            updated_at: None,
        }
        .into();
        assert_eq!(
            briefing.json(),
            json!({
                "kind": "engineer_briefing",
                "content": "brief {task_title}",
                "is_default": true,
                "updated_at": null,
            })
        );
    }

    /// `-y` answers for the caller: nothing is read, nothing blocks.
    #[test]
    fn yes_skips_the_confirmation() {
        assert!(confirm("Reset?", true).is_ok());
    }

    /// The question is the last thing between the caller and a deleted
    /// profile, so it says which one by name and role, not by the id typed.
    #[test]
    fn the_rm_question_names_the_profile_and_its_role() {
        assert_eq!(
            rm_question(&profile(Role::Engineer)),
            "Delete the engineer profile \"Engineer\" (01PROFILE)?"
        );
    }
}
