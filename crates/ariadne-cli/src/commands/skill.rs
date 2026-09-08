//! `ariadne skill ...`
//!
//! A skill is a document, so these are the things one does to a document:
//! list them, read one, write one, replace the text, put a shipped one back,
//! and delete one of your own. There is no prompt subcommand beside it — a
//! skill *is* the text, and the briefings a session is started, resumed and
//! nudged with are Ariadne's own.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use serde_json::json;

use ariadne_api::skills::{CreateSkillRequest, SkillDto, SkillSeat, UpdateSkillRequest};
use ariadne_client::Client;

use super::{Subject, confirm, path_segment};
use crate::output::{
    Column, Format, Kv, UNCAPPED, age, col, empty_state, moment, ok_id_line, print, print_kv,
    print_list, view,
};

/// Columns of `skill ls`. A skill is its name and the line it says about
/// itself; whether it is one Ariadne ships matters next, because that is what
/// says whether it can be reset or deleted.
const LS: &[Column] = &[
    col("title", 24).title(),
    col("summary", 64).rank(3),
    col("scope", UNCAPPED).rank(2),
    col("source", UNCAPPED).rank(2),
    col("age", UNCAPPED).rank(1),
];

#[derive(Subcommand)]
pub enum SkillCommand {
    /// List every skill, shipped and written
    Ls,
    /// Show a skill and its document
    Inspect {
        /// Skill name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::skill_names))]
        name: String,
    },
    /// Print a skill's document raw, ready to be piped to a file
    Get {
        /// Skill name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::skill_names))]
        name: String,
    },
    /// Write a skill of your own
    ///
    /// The document is the whole `SKILL.md`: YAML frontmatter naming the skill
    /// and describing it in one line, then the body. It is read from a file,
    /// or from stdin where no file is named.
    Create {
        /// Skill name, kebab-case: how an agent loads it and how a task names
        /// it
        name: String,
        /// Read the document from this file (default: stdin)
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Replace a skill's document
    Set {
        /// Skill name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::skill_names))]
        name: String,
        /// Read the new document from this file (default: stdin)
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Put a shipped skill back on the document Ariadne ships
    Reset {
        /// Skill name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::skill_names))]
        name: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Delete a skill of your own
    Rm {
        /// Skill name
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::skill_names))]
        name: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

pub async fn run(client: &Client, cmd: SkillCommand, format: Format) -> Result<()> {
    match cmd {
        SkillCommand::Ls => {
            let skills: Vec<SkillDto> = client.get_json("/v1/skills").await?;
            let now = chrono::Utc::now();
            print_list(
                format,
                &skills,
                LS,
                |s| {
                    vec![
                        s.name.clone(),
                        s.summary.clone(),
                        seat_label(s),
                        source_label(s),
                        age(&s.created_at, now),
                    ]
                },
                empty_state("No skills are available.", Some("ariadne doctor")),
            )?;
        }
        SkillCommand::Inspect { name } => {
            let s = get_skill(client, &name).await?;
            print(format, &s, || {
                print_kv(&[
                    ("name", Kv::title(s.name.clone())),
                    ("summary", s.summary.clone().into()),
                    ("scope", seat_label(&s).into()),
                    ("source", source_label(&s).into()),
                    ("created", Kv::meta(moment(&s.created_at))),
                    ("document", format!("\n---\n{}", s.document).into()),
                ])
            })?;
        }
        SkillCommand::Get { name } => {
            let skill = get_skill(client, &name).await?;
            // Raw and unadorned, trailing newline included or not exactly as
            // it is stored: `skill get > file` then `skill set --file` has to
            // round-trip.
            print(format, &skill, || print!("{}", skill.document))?;
        }
        SkillCommand::Create { name, file } => {
            // Read before anything is sent: a line naming an unreadable file
            // asks the daemon for nothing at all.
            let document = read_document(file)?;
            let skill: SkillDto = client
                .post_json(
                    "/v1/skills",
                    &CreateSkillRequest {
                        name: name.clone(),
                        document,
                    },
                )
                .await?;
            print(format, &skill, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "created", &skill.name)
                )
            })?;
        }
        SkillCommand::Set { name, file } => {
            let document = read_document(file)?;
            let skill: SkillDto = client
                .put_json(
                    &skill_path(&name),
                    &UpdateSkillRequest {
                        document: Some(document),
                    },
                )
                .await?;
            print(format, &skill, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "updated", &skill.name)
                )
            })?;
        }
        SkillCommand::Reset { name, yes } => {
            let skill = get_skill(client, &name).await?;
            let subject = Subject::new("skill", &skill.name, &skill.name);
            confirm("reset", &subject, &reset_question(&skill), yes)?;
            let skill = client.reset_skill(&path_segment(&name)).await?;
            print(format, &skill, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "reset", &skill.name)
                )
            })?;
        }
        SkillCommand::Rm { name, yes } => {
            let skill = get_skill(client, &name).await?;
            let subject = Subject::new("skill", &skill.name, &skill.name);
            confirm("delete", &subject, &rm_question(&skill), yes)?;
            client
                .send_no_content::<()>(http::Method::DELETE, &skill_path(&name), None)
                .await?;
            // The skill is gone, so there is no DTO left to print: what the
            // caller asked about, and that it happened.
            print(format, &json!({ "skill": name, "deleted": true }), || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "deleted", &name)
                )
            })?;
        }
    }
    Ok(())
}

/// The path of one skill, with the name escaped as a path segment.
fn skill_path(name: &str) -> String {
    format!("/v1/skills/{}", path_segment(name))
}

async fn get_skill(client: &Client, name: &str) -> Result<SkillDto> {
    Ok(client.get_json(&skill_path(name)).await?)
}

/// Where a skill's document comes from, which is what says whether it can be
/// reset and whether it can be deleted.
fn source_label(s: &SkillDto) -> String {
    match (s.builtin, s.document_is_default) {
        (true, true) => "shipped".into(),
        (true, false) => "shipped (edited)".into(),
        (false, _) => "yours".into(),
    }
}

/// Who can load a skill. The orchestrator's playbook stays listed so it can
/// be inspected, edited and reset, but its mark says it is not staffable.
fn seat_label(s: &SkillDto) -> String {
    match s.seat {
        SkillSeat::Orchestrator => "orchestrator only".into(),
        SkillSeat::Task => "task agents".into(),
    }
}

/// The document a `create` or a `set` writes: a file where one was named, and
/// stdin otherwise — but never a terminal nobody piped anything into, which
/// would hang with no sign of why.
fn read_document(file: Option<PathBuf>) -> Result<String> {
    match file {
        Some(path) => {
            std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
        }
        None => {
            if std::io::stdin().is_terminal() {
                bail!("nothing to read: pass --file <path>, or pipe the document in");
            }
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading the document from stdin")?;
            Ok(buf)
        }
    }
}

/// What `skill reset` asks: the name alone does not say what is about to be
/// thrown away, so the question says it is the edited text.
fn reset_question(s: &SkillDto) -> String {
    format!(
        "Throw away the text written over the {} skill and go back to the one Ariadne ships?",
        s.name
    )
}

/// What `skill rm` asks before it deletes.
fn rm_question(s: &SkillDto) -> String {
    format!("Delete the {} skill?", s.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, builtin: bool, default: bool) -> SkillDto {
        SkillDto {
            document_is_default: default,
            builtin,
            ..crate::commands::fixtures::skill(name, "one line")
        }
    }

    #[test]
    fn the_skill_subject_column_is_title() {
        let table = crate::output::render_table(
            LS,
            &[vec![String::new(); LS.len()]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.contains("TITLE"), "{table}");
        assert!(!header.contains("NAME"), "{table}");
    }

    /// The three things a reader needs off a listing: whether Ariadne ships
    /// the skill, whether somebody has written over it, and whether it is
    /// theirs to delete.
    #[test]
    fn a_listing_says_where_each_document_came_from() {
        assert_eq!(source_label(&skill("coding", true, true)), "shipped");
        assert_eq!(
            source_label(&skill("coding", true, false)),
            "shipped (edited)"
        );
        assert_eq!(source_label(&skill("ours", false, false)), "yours");
    }

    /// The orchestrator's playbook is visible to the person who can edit it,
    /// but its listing mark says that task staffing cannot load it.
    #[test]
    fn a_listing_marks_an_orchestrator_only_skill() {
        let mut orchestration = skill("orchestration", true, true);
        orchestration.seat = SkillSeat::Orchestrator;
        assert_eq!(seat_label(&orchestration), "orchestrator only");
        assert_eq!(seat_label(&skill("coding", true, true)), "task agents");
    }

    /// Both questions name the skill: the last thing between the caller and a
    /// document that is about to go says which one it is.
    #[test]
    fn the_questions_name_the_skill() {
        let s = skill("code-review", true, false);
        assert_eq!(
            reset_question(&s),
            "Throw away the text written over the code-review skill and go back to the one \
             Ariadne ships?"
        );
        assert_eq!(rm_question(&s), "Delete the code-review skill?");
    }
}
