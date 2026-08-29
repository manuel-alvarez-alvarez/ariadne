//! What one prompt of one profile is, and what a `profile prompt` line does
//! with it.
//!
//! Two halves of the API read as one list here — the briefings live in
//! `profile_prompts`, the system prompt on the profile row — but from the
//! terminal they are the prompts one profile runs on, spelled `system` and
//! `engineer-briefing` alike. Which of them a profile has depends on its role,
//! and that is the one check a command line cannot make: clap has no role to
//! hand, so [`parse_prompt_arg`] asks only whether the kind exists and
//! [`Owner::owns`] asks whose it is, once the profile can be named.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde_json::json;

use ariadne_api::profiles::{ProfileDto, ProfilePromptDto, UpdateProfileRequest};
use ariadne_client::Client;
use ariadne_core::{PromptKind, Role};

use super::PromptCommand;

use super::{get_profile, profile_path};
use crate::commands::confirm;
use crate::output::{Column, Format, UNCAPPED, local_time, print, print_table};

/// Columns of `profile prompts`.
const PROMPTS: &[Column] = &[
    ("kind", UNCAPPED),
    ("status", UNCAPPED),
    ("updated", UNCAPPED),
    ("content", 48),
];

/// How the system prompt is spelled on a command line.
pub(super) const SYSTEM: &str = "system";

/// What a `<kind>` argument names: one of the briefings the profile's role
/// owns, or the profile's own system prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptArg {
    System,
    Briefing(PromptKind),
}

impl PromptArg {
    /// The wire spelling: what the daemon stores, `profile prompts` prints
    /// and `--format json` answers with.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            PromptArg::System => SYSTEM,
            PromptArg::Briefing(kind) => kind.as_str(),
        }
    }

    /// How a command line spells it: kebab-case, the way the help lists it
    /// and the completions offer it. Both spellings are read back by
    /// [`parse_prompt_arg`], and this is the one a message quotes.
    pub(super) fn spelling(&self) -> String {
        self.as_str().replace('_', "-")
    }
}

/// Every prompt a profile of `role` owns — its system prompt first, then the
/// briefings in briefing order — or, with no role to go on, every kind there
/// is: what an error lists when no profile has said which role it is about.
pub(super) fn owned(role: Option<Role>) -> Vec<PromptArg> {
    let briefings: &[PromptKind] = match role {
        Some(role) => PromptKind::for_role(role),
        None => &PromptKind::ALL,
    };
    let briefings = briefings.iter().map(|k| PromptArg::Briefing(*k));
    std::iter::once(PromptArg::System).chain(briefings).collect()
}

/// Prompt kinds as a command line spells them, comma-separated.
pub(super) fn spelled(args: &[PromptArg]) -> String {
    args.iter()
        .map(PromptArg::spelling)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A `<kind>` argument, without knowing yet which profile it is for.
pub(super) fn parse_prompt_arg(s: &str) -> Result<PromptArg, String> {
    if s == SYSTEM {
        return Ok(PromptArg::System);
    }
    // Both spellings name the same prompt: the kebab-case one the help lists
    // and the completions offer, and the snake_case one the daemon stores and
    // `profile prompts` prints back — so a kind read off that table can be
    // typed straight back in.
    s.replace('-', "_")
        .parse()
        .map(PromptArg::Briefing)
        .map_err(|_| {
            format!(
                "unknown prompt kind: {s} (expected one of {})",
                spelled(&owned(None))
            )
        })
}

/// Whose prompts a `<kind>` is about: a profile that exists, or the role of
/// one `profile create` is about to make. Only one has a name for an error.
pub(crate) enum Owner<'a> {
    Role(Role),
    Profile(&'a ProfileDto),
}

impl Owner<'_> {
    pub(super) fn role(&self) -> Role {
        match self {
            Owner::Role(role) => *role,
            Owner::Profile(p) => p.role,
        }
    }

    /// `arg` if these prompts include it, otherwise an error naming the ones
    /// they do: prompt kinds belong to exactly one role, and a reviewer
    /// profile asked for `engineer-briefing` has nothing to show.
    pub(super) fn owns(&self, arg: PromptArg) -> Result<PromptArg> {
        let PromptArg::Briefing(kind) = arg else {
            // Whatever the role, it runs on a system prompt.
            return Ok(arg);
        };
        if kind.owned_by(self.role()) {
            return Ok(arg);
        }
        // Which role owns it is `roles`' word rather than this sentence's, so
        // the refusal stays right however many it answers with — one, for
        // every kind there is: each briefs a role through its own lifecycle.
        let owner = kind.roles().iter().map(|r| r.as_str()).collect::<Vec<_>>();
        let owner = owner.join("/");
        let (kind, prompts) = (arg.spelling(), spelled(&owned(Some(self.role()))));
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

/// One prompt as the CLI prints it, whichever half of the API it came from.
/// `content` is the default of the kind while the profile holds none of its
/// own, and whether it is one is the daemon's word rather than a comparison
/// made here.
struct Prompt {
    kind: PromptArg,
    content: String,
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

    /// The kind, whether it has been touched, when — a default was written by
    /// nobody, so it has no date — and as much of the text as fits.
    fn row(&self) -> Vec<String> {
        vec![
            self.kind.as_str().into(),
            match self.is_default {
                true => "default".into(),
                false => "customized".into(),
            },
            self.updated_at
                .as_deref()
                .map_or_else(|| "-".to_string(), local_time),
            self.content.clone(),
        ]
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

/// `ariadne profile prompts <id>`. A table shows that a prompt is there and
/// whether it has been touched; json is how a script reads what it says, so it
/// carries the whole text.
pub async fn list(client: &Client, id: &str, no_trunc: bool, format: Format) -> Result<()> {
    let profile = get_profile(client, id).await?;
    let prompts = all(client, &profile).await?;
    let rows: Vec<Vec<String>> = prompts.iter().map(Prompt::row).collect();
    print(format, &documents(&prompts), || {
        print_table(PROMPTS, &rows, no_trunc)
    })
}

/// Every prompt of a profile: its system prompt first, then its briefings.
async fn all(client: &Client, profile: &ProfileDto) -> Result<Vec<Prompt>> {
    let briefings = client.list_profile_prompts(&profile.id).await?;
    let mut out = vec![Prompt::system(profile)];
    out.extend(briefings.into_iter().map(Prompt::from));
    Ok(out)
}

fn documents(prompts: &[Prompt]) -> Vec<serde_json::Value> {
    prompts.iter().map(Prompt::json).collect()
}

pub async fn run(client: &Client, cmd: PromptCommand, format: Format) -> Result<()> {
    match cmd {
        PromptCommand::Get { id, kind } => {
            let profile = get_profile(client, &id).await?;
            let prompt = fetch(client, &profile, Owner::Profile(&profile).owns(kind)?).await?;
            // Raw and unadorned, trailing newline included or not exactly as
            // it is stored: `get > file` then `set --file` has to round-trip.
            print(format, &prompt.json(), || print!("{}", prompt.content))?;
        }
        PromptCommand::Set { id, kind, file } => {
            let profile = get_profile(client, &id).await?;
            let kind = Owner::Profile(&profile).owns(kind)?;
            let prompt = write(client, &profile, kind, Some(read_content(file)?)).await?;
            let what = format!("{} of {} ({})", kind.spelling(), profile.name, profile.id);
            print(format, &prompt.json(), || println!("updated {what}"))?;
        }
        PromptCommand::Reset { id, kind, all, yes } => {
            let profile = get_profile(client, &id).await?;
            // clap keeps `kind` required unless `--all` is there.
            let kinds = match kind {
                Some(kind) => vec![Owner::Profile(&profile).owns(kind)?],
                None => owned(Some(profile.role)),
            };
            confirm(&reset_question(&profile, &kinds, all), yes)?;
            let mut done = Vec::new();
            for kind in kinds {
                done.push(write(client, &profile, kind, None).await?);
            }
            // One kind was asked for, one object comes back; --all is the
            // plural request, so it always answers with a list.
            let payload = match all {
                true => json!(documents(&done)),
                false => done[0].json(),
            };
            let (name, role) = (&profile.name, profile.role.as_str());
            print(format, &payload, || {
                for p in &done {
                    let kind = p.kind.spelling();
                    println!(
                        "reset {kind} of {name} ({}) to the {role} default",
                        profile.id
                    );
                }
            })?;
        }
    }
    Ok(())
}

/// Every kind of the role is listed, its default included, so the one that was
/// asked for is there.
async fn fetch(client: &Client, profile: &ProfileDto, kind: PromptArg) -> Result<Prompt> {
    all(client, profile)
        .await?
        .into_iter()
        .find(|p| p.kind == kind)
        .with_context(|| {
            format!(
                "{} ({}) answered with no {} prompt",
                profile.name,
                profile.id,
                kind.spelling()
            )
        })
}

/// Replace one prompt, or — with no `content` — put it back to its role's
/// default. Which endpoint that is depends on which half of the API it is in.
async fn write(
    client: &Client,
    profile: &ProfileDto,
    kind: PromptArg,
    content: Option<String>,
) -> Result<Prompt> {
    let id = &profile.id;
    Ok(match (kind, content) {
        (PromptArg::System, Some(content)) => {
            let body = UpdateProfileRequest {
                system_prompt: Some(content),
                ..Default::default()
            };
            Prompt::system(&client.put_json(&profile_path(id), &body).await?)
        }
        (PromptArg::System, None) => Prompt::system(&client.reset_system_prompt(id).await?),
        (PromptArg::Briefing(k), Some(content)) => {
            client.update_profile_prompt(id, k, content).await?.into()
        }
        (PromptArg::Briefing(k), None) => client.reset_profile_prompt(id, k).await?.into(),
    })
}

/// What `prompt reset` asks before it overwrites: whatever the prompt says now
/// is gone, so the question names how much of the profile it is about to
/// replace.
fn reset_question(profile: &ProfileDto, kinds: &[PromptArg], all: bool) -> String {
    let (what, defaults) = match (all, kinds) {
        (false, [one]) => (format!("the {} prompt", one.spelling()), "default"),
        _ => (format!("all {} prompts", kinds.len()), "defaults"),
    };
    format!(
        "Reset {what} of {} ({}) to the {} {defaults}?",
        profile.name,
        profile.id,
        profile.role.as_str()
    )
}

/// The new text of a prompt: the file that was named, else stdin. Whatever it
/// holds is what gets sent — byte for byte, an empty file included: the CLI is
/// a pipe here and not a censor, and what goes in is what `prompt get` prints
/// back. The one refusal is a terminal on stdin, where nobody piped anything
/// in and reading it would hang on a person.
fn read_content(file: Option<PathBuf>) -> Result<String> {
    let Some(file) = file else {
        if std::io::stdin().is_terminal() {
            bail!("no new text: pass --file <path>, or pipe the prompt in on stdin");
        }
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading the new prompt from stdin")?;
        return Ok(buf);
    };
    std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(role: Role) -> ProfileDto {
        crate::commands::fixtures::profile("Engineer", role)
    }

    fn owns(role: Role, arg: PromptArg) -> Result<PromptArg> {
        Owner::Profile(&profile(role)).owns(arg)
    }

    fn briefing() -> Prompt {
        ProfilePromptDto {
            kind: PromptKind::EngineerBriefing,
            content: "brief {task_title}".into(),
            is_default: true,
            updated_at: None,
        }
        .into()
    }

    /// Its own system prompt first, then its role's briefings in briefing
    /// order. The engineer's and the reviewer's lists are also what the
    /// refusals below quote back.
    #[test]
    fn a_profile_owns_its_system_prompt_and_the_briefings_of_its_role() {
        let kinds = |role| spelled(&owned(Some(role)));
        assert_eq!(
            kinds(Role::Planner),
            "system, planner-briefing, planner-resume"
        );
        assert_eq!(
            kinds(Role::Engineer),
            "system, engineer-briefing, engineer-resume, changes-requested, \
             landing-direct, landing-pull-request"
        );
    }

    /// A kind is typed either way round — the kebab-case spelling the help
    /// lists and the completions offer, and the snake_case one the daemon
    /// stores and `profile prompts` prints back — plus `system`, which is one
    /// word either way.
    #[test]
    fn a_kind_is_spelled_in_kebab_or_in_snake() {
        assert_eq!(parse_prompt_arg(SYSTEM), Ok(PromptArg::System));
        for spelling in ["engineer-briefing", "engineer_briefing"] {
            assert_eq!(
                parse_prompt_arg(spelling),
                Ok(PromptArg::Briefing(PromptKind::EngineerBriefing)),
                "{spelling}"
            );
        }
        assert_eq!(
            parse_prompt_arg("changes_requested"),
            Ok(PromptArg::Briefing(PromptKind::ChangesRequested))
        );
        assert_eq!(
            parse_prompt_arg("landing-pull-request"),
            Ok(PromptArg::Briefing(PromptKind::LandingPullRequest)),
            "every underscore of a kind gives way, not just the first"
        );
    }

    /// A typo must not send the caller to `--help` to find the spelling: the
    /// refusal quotes what was typed and lists the kinds there are, in the
    /// spelling the help lists them in.
    #[test]
    fn an_unknown_kind_lists_them_all() {
        let err = parse_prompt_arg("briefing").expect_err("unknown");
        assert!(err.starts_with("unknown prompt kind: briefing"), "{err}");
        for expected in [SYSTEM, "engineer-briefing", "reviewer-resume"] {
            assert!(err.contains(expected), "{err}");
        }
    }

    /// Every role runs on a system prompt, and every briefing belongs to the
    /// roles that own it — the check `parse_prompt_arg` cannot make. Where the
    /// kind exists but not for this profile, the message has to say which
    /// prompts this one has, since the CLI knows and the caller does not.
    #[test]
    fn every_role_owns_its_system_prompt_and_its_own_briefings() {
        for role in Role::ALL {
            assert_eq!(owns(role, PromptArg::System).expect(SYSTEM), PromptArg::System);
        }
        for kind in PromptKind::ALL {
            for role in kind.roles() {
                assert!(owns(*role, PromptArg::Briefing(kind)).is_ok(), "{kind:?}");
            }
        }

        let err = owns(
            Role::Reviewer,
            PromptArg::Briefing(PromptKind::EngineerBriefing),
        )
        .expect_err("wrong role")
        .to_string();
        assert!(err.contains("is a reviewer profile"), "{err}");
        assert!(err.contains("engineer-briefing"), "{err}");
        assert!(
            err.contains("its prompts are: system, reviewer-briefing, reviewer-resume"),
            "{err}"
        );
    }

    /// `--all` is the plural request, so the question counts the prompts —
    /// including the system prompt it takes with it.
    #[test]
    fn the_reset_question_names_what_it_is_about_to_replace() {
        let p = profile(Role::Engineer);
        let one = [PromptArg::Briefing(PromptKind::ChangesRequested)];
        assert_eq!(
            reset_question(&p, &one, false),
            "Reset the changes-requested prompt of Engineer (01Engineer) to the engineer default?"
        );
        assert_eq!(
            reset_question(&p, &owned(Some(p.role)), true),
            "Reset all 6 prompts of Engineer (01Engineer) to the engineer defaults?"
        );
    }

    /// `prompt get > file` then `prompt set --file file` has to round-trip, so
    /// nothing is trimmed, wrapped or added on the way in — and emptying a
    /// prompt is the caller's call to make, not the CLI's to refuse.
    #[test]
    fn a_file_is_read_byte_for_byte() {
        let dir = tempfile::tempdir().expect("tempdir");
        let write = |name: &str, raw: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, raw).expect("write");
            read_content(Some(path)).expect("content")
        };
        assert_eq!(write("prompt.md", "  Brief.\n\nEnd.\n"), "  Brief.\n\nEnd.\n");
        assert_eq!(write("empty.md", ""), "");

        let err = read_content(Some("/no/such/prompt.md".into())).expect_err("missing");
        assert!(err.to_string().contains("/no/such/prompt.md"), "{err}");
    }

    /// Whether a prompt is the default is the daemon's word, not a comparison
    /// made here: the CLI prints the flag it was sent, for the system prompt as
    /// for a briefing — and a default was written by nobody, so it has no date.
    /// json carries all four, since that is what a script reads.
    #[test]
    fn a_prompt_carries_the_status_and_the_date_the_daemon_sent() {
        let mut p = profile(Role::Engineer);
        assert_eq!(Prompt::system(&p).row()[1], "customized");
        assert_eq!(Prompt::system(&p).row()[2], local_time(&p.updated_at));
        assert_eq!(
            Prompt::system(&p).json(),
            json!({
                "kind": "system",
                "content": "you are an engineer",
                "is_default": false,
                "updated_at": "2026-08-17T09:00:00Z",
            })
        );

        p.system_prompt_is_default = true;
        assert_eq!(Prompt::system(&p).row()[1], "default");
        assert_eq!(Prompt::system(&p).row()[2], "-");

        assert_eq!(briefing().row()[1], "default");
        assert_eq!(
            briefing().json(),
            json!({
                "kind": "engineer_briefing",
                "content": "brief {task_title}",
                "is_default": true,
                "updated_at": null,
            })
        );
    }
}
