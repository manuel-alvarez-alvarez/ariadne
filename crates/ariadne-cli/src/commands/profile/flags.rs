//! The `--prompt` and `--prompt-file` flags of `profile create` and
//! `profile update`.
//!
//! They are one list: each kind may be given once, by either flag, and both
//! are read and checked before a single request goes out — a line that repeats
//! a kind, names a file that will not read, or names a prompt this role does
//! not own must not leave half a profile behind it.

use anyhow::{Context, Result, bail};

use ariadne_api::profiles::ProfileDto;
use ariadne_client::Client;
use ariadne_core::PromptKind;

use super::prompts::{Owner, PromptArg, SYSTEM, owned, parse_prompt_arg, spelled};

/// One `<kind>=<value>` flag: which prompt it is for, and the text it carries
/// — or, from `--prompt-file`, the path that text is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptAssignment {
    pub(crate) kind: PromptArg,
    pub(crate) value: String,
}

/// `--prompt <kind>=<text>`.
pub fn parse_prompt_text(s: &str) -> Result<PromptAssignment, String> {
    parse_assignment(s, "<text>")
}

/// `--prompt-file <kind>=<path>`. The file is read later, by [`read_prompts`]:
/// a path is a path whether or not it exists yet.
pub fn parse_prompt_file(s: &str) -> Result<PromptAssignment, String> {
    parse_assignment(s, "<path>")
}

/// Split at the first `=`, with the kind checked against every kind there is.
/// Whether the profile's own role owns it is [`Owner::owns`]'s call — that one
/// needs a role, and clap has none.
fn parse_assignment(s: &str, value: &str) -> Result<PromptAssignment, String> {
    let (kind, text) = s.split_once('=').ok_or_else(|| {
        format!(
            "missing <kind>=: write {SYSTEM}={value} to set the system prompt — the kinds are: {}",
            spelled(&owned(None))
        )
    })?;
    Ok(PromptAssignment {
        kind: parse_prompt_arg(kind)?,
        value: text.to_string(),
    })
}

/// Both flags of one command line, merged and read.
///
/// Nothing in this needs to know the role, so `create` and `update` both run
/// it before they ask the daemon anything. What is left for [`owned_prompts`]
/// is the one check that does need a role.
pub fn read_prompts(
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
    let order = owned(Some(owner.role()));
    out.sort_by_key(|(kind, _)| order.iter().position(|k| k == kind).unwrap_or(usize::MAX));
    Ok(out)
}

/// A collected list split the way the API takes it: the system prompt, which
/// travels with the profile itself, apart from the briefings, which do not.
pub(crate) fn split_system(
    given: Vec<(PromptArg, String)>,
) -> (Option<String>, Vec<(PromptKind, String)>) {
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

/// The briefings of a create or update, one PUT each, after the profile itself
/// has been written. Answers with everything that was written, the system
/// prompt included when the profile body carried one.
///
/// A write that fails stops the rest: the profile is already part-way changed,
/// so the error says which prompt failed and which ones stand.
pub(crate) async fn write_briefings(
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

    use ariadne_core::Role;

    /// A flag as clap would have parsed it.
    fn assignment(kind: &str, value: &str) -> PromptAssignment {
        PromptAssignment {
            kind: parse_prompt_arg(kind).expect("a kind"),
            value: value.to_string(),
        }
    }

    /// The two flags are one list: text and files together, each kind once, in
    /// the order the profile owns them rather than the order they were typed.
    /// The system prompt is then split off, since it travels with the profile
    /// row and the briefings do not.
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
                    assignment(SYSTEM, "you are"),
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

        let (system, briefings) = split_system(collected);
        assert_eq!(system.as_deref(), Some("you are"));
        assert_eq!(
            briefings,
            [
                (PromptKind::EngineerBriefing, "brief".to_string()),
                (PromptKind::ChangesRequested, "fix it\n".to_string()),
            ]
        );

        // A line with no prompt flags sets no prompts at all.
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

    /// Two values for one prompt is nobody's intention, whichever flags spell
    /// it — and a file that is not there is named before anything is sent.
    #[test]
    fn a_line_that_cannot_be_right_is_refused_before_any_request() {
        let twice = |texts, files| read_prompts(texts, files).expect_err("bad").to_string();
        for err in [
            twice(
                vec![assignment(SYSTEM, "a"), assignment(SYSTEM, "b")],
                vec![],
            ),
            twice(
                vec![assignment(SYSTEM, "a")],
                vec![assignment(SYSTEM, "/tmp/x.md")],
            ),
        ] {
            assert!(err.starts_with("system is set twice"), "{err}");
            assert!(err.contains("--prompt-file"), "{err}");
        }

        let err = twice(
            vec![],
            vec![assignment("engineer_briefing", "/no/such/brief.md")],
        );
        assert!(err.contains("/no/such/brief.md"), "{err}");
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

    /// The same refusal on a `profile create` line, which has a role and no
    /// profile to name: it says whose the kind is and which ones were meant.
    #[test]
    fn a_kind_of_another_role_stops_a_create_line() {
        let err = owned_prompts(
            Owner::Role(Role::Engineer),
            read_prompts(vec![assignment("planner_briefing", "plan")], vec![]).expect("read"),
        )
        .expect_err("wrong role")
        .to_string();
        assert!(
            err.starts_with("engineer profiles have no planner-briefing prompt"),
            "{err}"
        );
        assert!(err.contains("(planner owns it)"), "{err}");
        assert!(
            err.contains(
                "their prompts are: system, engineer-briefing, engineer-resume, changes-requested"
            ),
            "{err}"
        );
    }
}
