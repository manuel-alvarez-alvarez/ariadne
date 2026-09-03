//! The one prompt a profile owns, and what a `profile prompt` line does with
//! it.
//!
//! A profile says what its agent is briefed as — its system prompt — and
//! nothing of the lifecycle around it: the briefings that start, resume and
//! nudge a session are Ariadne's own templates, the same for every profile.
//! So `profile prompt get|set|reset` is about the system prompt, and the
//! `system` it is spelled with is the only kind a line may name — a briefing
//! named there is refused with the reason ([`parse_prompt_arg`]).

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use ariadne_api::profiles::{ProfileDto, UpdateProfileRequest};
use ariadne_client::Client;

use super::PromptCommand;
use super::{get_profile, profile_path};
use crate::commands::{Subject, confirm};
use crate::output::{Format, print, style, view};

/// How the system prompt is spelled on a command line.
pub(super) const SYSTEM: &str = "system";

/// The `[KIND]` of a `profile prompt` line, which names the one prompt a
/// profile owns.
///
/// The word is optional — a profile has one prompt, so a line that leaves it
/// out means that one — and it is kept because it is what a briefing named by
/// mistake is answered on: `engineer-briefing` is Ariadne's own text now, and
/// the message is where somebody reads that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemPromptArg;

/// A `<kind>` argument: `system`, and nothing else.
pub(super) fn parse_prompt_arg(s: &str) -> Result<SystemPromptArg, String> {
    match s == SYSTEM {
        true => Ok(SystemPromptArg),
        false => Err(format!(
            "{s} is no prompt of a profile: a profile owns its {SYSTEM} prompt \
             alone, and every briefing a session is started, resumed or nudged \
             with is Ariadne's own"
        )),
    }
}

pub async fn run(client: &Client, cmd: PromptCommand, format: Format) -> Result<()> {
    match cmd {
        PromptCommand::Get { id, .. } => {
            let profile = get_profile(client, &id).await?;
            // Raw and unadorned, trailing newline included or not exactly as
            // it is stored: `prompt get > file` then `prompt set --file` has
            // to round-trip.
            print(format, &profile, || print!("{}", profile.system_prompt))?;
        }
        PromptCommand::Set { id, file, .. } => {
            let profile = get_profile(client, &id).await?;
            let content = read_content(file)?;
            let p: ProfileDto = client
                .put_json(
                    &profile_path(&profile.id),
                    &UpdateProfileRequest {
                        system_prompt: Some(content),
                        ..Default::default()
                    },
                )
                .await?;
            print(format, &p, || {
                println!(
                    "{} system prompt of {} ({})",
                    style::paint(view().color, style::OK, "updated"),
                    p.name,
                    style::paint(view().color, style::ID, &p.id)
                )
            })?;
        }
        PromptCommand::Reset { id, yes, .. } => {
            let profile = get_profile(client, &id).await?;
            let subject = Subject::new("profile", &profile.name, &profile.id);
            confirm(
                "reset the system prompt of",
                &subject,
                &reset_question(&profile, &subject),
                yes,
            )?;
            let p = client.reset_system_prompt(&profile.id).await?;
            print(format, &p, || {
                println!(
                    "reset system prompt of {} ({}) to the {} default",
                    p.name,
                    p.id,
                    p.role.as_str()
                )
            })?;
        }
    }
    Ok(())
}

/// What `profile prompt reset` asks before it overwrites: whatever the system
/// prompt says now is gone, so the question names the role default it is
/// about to be replaced by.
fn reset_question(profile: &ProfileDto, subject: &Subject) -> String {
    format!(
        "Reset the system prompt of {} to the {} default?",
        subject.named(),
        profile.role.as_str()
    )
}

/// The `system_prompt` field of a create or an update: `--system-prompt` text
/// as it was typed, `--system-prompt-file`'s contents, or nothing when
/// neither flag was given — which is what leaves the profile on its role's
/// default. Clap keeps the two flags mutually exclusive, so at most one of
/// them ever carries a value.
pub fn read_system_prompt(text: Option<String>, file: Option<PathBuf>) -> Result<Option<String>> {
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

/// The new text of `profile prompt set`: the file that was named, else stdin.
/// Whatever it holds is what gets sent — byte for byte, an empty file
/// included: the CLI is a pipe here and not a censor, and what goes in is what
/// `prompt get` prints back. The one refusal is a terminal on stdin, where
/// nobody piped anything in and reading it would hang on a person.
fn read_content(file: Option<PathBuf>) -> Result<String> {
    let Some(file) = file else {
        if std::io::stdin().is_terminal() {
            bail!("no new text: pass --file <path>, or pipe the prompt in on stdin");
        }
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading the new system prompt from stdin")?;
        return Ok(buf);
    };
    std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_core::Role;

    fn profile(role: Role) -> ProfileDto {
        crate::commands::fixtures::profile("Engineer", role)
    }

    /// The question is the last thing between the caller and a replaced
    /// prompt, so it names the profile and the role default it goes back to.
    #[test]
    fn the_reset_question_names_what_it_is_about_to_replace() {
        let p = ProfileDto {
            id: "01m0prof0000000000000abcde".into(),
            ..profile(Role::Engineer)
        };
        let subject = Subject::new("profile", &p.name, &p.id);
        assert_eq!(
            reset_question(&p, &subject),
            "Reset the system prompt of \"Engineer\" (…000abcde) to the engineer default?"
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
        assert_eq!(
            write("prompt.md", "  Brief.\n\nEnd.\n"),
            "  Brief.\n\nEnd.\n"
        );
        assert_eq!(write("empty.md", ""), "");

        let err = read_content(Some("/no/such/prompt.md".into())).expect_err("missing");
        assert!(err.to_string().contains("/no/such/prompt.md"), "{err}");
    }

    /// The word a line may name is `system`, in either spelling; a briefing
    /// named there is refused with what owns it, since that is where somebody
    /// reads that a profile no longer holds one.
    #[test]
    fn the_only_kind_a_line_names_is_the_system_prompt() {
        assert_eq!(parse_prompt_arg(SYSTEM), Ok(SystemPromptArg));
        for spelling in ["engineer-briefing", "engineer_briefing", "System"] {
            let err = parse_prompt_arg(spelling).expect_err("a briefing");
            assert!(
                err.starts_with(&format!("{spelling} is no prompt")),
                "{err}"
            );
            assert!(err.contains("Ariadne's own"), "{err}");
        }
    }

    /// A create or an update sends the text it was given, the file's contents
    /// where a file was named, and nothing at all where neither flag was —
    /// which is what leaves the profile on the default of its role.
    #[test]
    fn a_created_prompt_is_the_text_the_file_or_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("system.md");
        std::fs::write(&path, "You are eng.\n").expect("write");

        assert_eq!(
            read_system_prompt(Some("You are eng.".into()), None).expect("text"),
            Some("You are eng.".to_string())
        );
        assert_eq!(
            read_system_prompt(None, Some(path)).expect("file"),
            Some("You are eng.\n".to_string())
        );
        assert_eq!(read_system_prompt(None, None).expect("neither"), None);

        let err = read_system_prompt(None, Some("/no/such/system.md".into())).expect_err("missing");
        assert!(err.to_string().contains("/no/such/system.md"), "{err}");
    }
}
