//! The prompts an agent is launched with are Ariadne's own templates.
//!
//! One thing has to hold: the built-in template of the kind is what the
//! session gets, with nothing of the database between the two — a profile
//! carries a system prompt and no lifecycle text at all.
//!
//! And what assembly comes to is pinned here too: the placeholders of a spawn,
//! a resume and a review round are filled in by hand and compared with what
//! the daemon produced, so a change to the assembler shows up as a diff rather
//! than as an agent quietly briefed with something else.
//!
//! The agent is the harness's stub, and the rendered briefing is read back
//! from the session's launch file — what the agent was told. `git` is real —
//! spawning an author creates its worktree.

mod common;

use ariadne_core::{Actor, PromptKind, Seat, TaskStatus};
use ariadne_daemon::agents::prompts;
use ariadne_store::defaults::default_prompt_text;

use common::{Cast, Harness, harness};

/// What the author requested review with, and what it wrote afterwards:
/// the briefing has to carry the first and never the second.
const SUMMARY: &str = "Rendered the board from the store, with a test per lane.";

/// A task ready for its author to be spawned, in a real repo.
async fn seeded(h: &Harness) -> Cast {
    h.git_repo("repo");
    h.cast().await
}

/// The placeholders filled in by hand, so that what an assertion compares
/// against is the assembled text and not the assembler's own answer to the
/// same question.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in values {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

fn default_for(kind: PromptKind) -> String {
    default_prompt_text(kind).to_string()
}

/// The built-in template is the briefing the agent is launched with,
/// placeholders and all, as its first prompt — behind the system layer and
/// the index of its skills.
#[tokio::test]
async fn a_spawned_author_is_briefed_from_the_builtin_template() {
    let h = harness().await;
    let cast = seeded(&h).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let briefing = prompts::author_briefing(
        prompts::template_for(PromptKind::AuthorBriefing),
        &task,
        &cast.goal,
        &cast.repo,
        &[],
    );
    let launch = h.launch_file(&session.id).expect("a launch file");
    assert_eq!(
        launch.initial_prompt.as_deref(),
        Some(briefing.as_str()),
        "the built-in briefing, rendered"
    );

    // The system layer is what the seat owes, and then the index of the skills
    // this agent loads: one line each, with the document left on disk for the
    // agent to read when it needs it.
    let run_dir = h.launcher.cfg.run_dir.join(&session.id);
    let system = launch.system_prompt;
    let (owed, index) = system
        .split_once(
            "\n\nYour skills. Read the document of a skill before you do the work it covers:",
        )
        .expect("a skill index");
    assert_eq!(
        owed,
        ariadne_store::defaults::default_system_prompt(Seat::Author).trim(),
        "the seat's own text, word for word out of the code"
    );
    let document = run_dir.join("skills").join("coding").join("SKILL.md");
    assert_eq!(
        index.trim(),
        format!(
            "- coding: Implement a task from its specification, with the tests that \
             prove it. Use when a task asks for new code, a feature, or a fix. ({})",
            document.display()
        ),
        "one line per skill: its summary, and where its document is"
    );
    // And the path the line names is a document, not a promise.
    let written = std::fs::read_to_string(&document).expect("the skill document on disk");
    assert_eq!(
        written,
        ariadne_store::defaults::default_skill_document("coding").unwrap(),
        "the shipped document, written whole for the agent to open"
    );
}

/// The code's text is what reaches the agent, without anything having been
/// copied into the database first — and what reaches it is the built-in
/// template with every placeholder filled in, exactly.
#[tokio::test]
async fn a_spawn_assembles_the_default_briefing_word_for_word() {
    let h = harness().await;
    let cast = seeded(&h).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let expected = fill(
        &default_for(PromptKind::AuthorBriefing),
        &[
            ("task_title", &task.title),
            ("task_description", &task.description),
            ("goal_title", &cast.goal.title),
            ("worktree_path", task.worktree_path.as_deref().unwrap()),
            ("branch", &task.branch),
            ("base_branch", &cast.repo.base_branch),
            ("repo_path", &cast.repo.path),
            ("landing", cast.task.landing().as_str()),
            ("dependencies", "none"),
        ],
    );

    let launch = h.launch_file(&session.id).expect("a launch file");
    assert_eq!(
        launch.initial_prompt.as_deref(),
        Some(expected.as_str()),
        "the default briefing, assembled"
    );
    // The same text is what the assembler answers on its own, so nothing
    // between the two decorates it.
    assert_eq!(
        prompts::author_briefing(
            &default_for(PromptKind::AuthorBriefing),
            &task,
            &h.store.get_goal(&task.goal_id).await.unwrap(),
            &cast.repo,
            &[],
        ),
        expected
    );
}

/// The other two assemblies an agent meets, pinned the same way: what an
/// author holding unfinished work is picked up with, and what a reviewer
/// owing a verdict is.
#[tokio::test]
async fn a_resume_and_a_review_assemble_word_for_word() {
    let h = harness().await;
    let cast = seeded(&h).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();

    assert_eq!(
        prompts::author_resume_briefing(&default_for(PromptKind::AuthorResume), &task,),
        fill(
            &default_for(PromptKind::AuthorResume),
            &[("task_title", &task.title), ("branch", &task.branch)],
        )
    );

    assert_eq!(
        prompts::reviewer_resume_briefing(
            &default_for(PromptKind::ReviewerResume),
            &task,
            Some(SUMMARY),
        ),
        fill(
            &default_for(PromptKind::ReviewerResume),
            &[
                ("task_title", &task.title),
                ("branch", &task.branch),
                ("summary", SUMMARY),
            ],
        )
    );
    // A review nobody wrote a summary for still says so in words.
    assert!(
        prompts::reviewer_resume_briefing(&default_for(PromptKind::ReviewerResume), &task, None,)
            .contains("(none provided)")
    );
}

/// The `{summary}` a reviewer is briefed with is the one the author
/// requested review with — the round's own record of it — and not whatever
/// the author happened to say last.
///
/// The two are only the same until the author writes anything else: a
/// "thanks, will do" posted after the request would otherwise be what the
/// reviewers, and the people reading a published request, are handed as the
/// summary of the change.
#[tokio::test]
async fn a_reviewer_is_briefed_with_the_summary_review_was_requested_with() {
    let h = harness().await;
    let cast = seeded(&h).await;
    let task = &cast.task;
    // The author's worktree is what creates the branch the reviewer's is
    // cut from.
    h.launcher.spawn_author(&task.id).await.unwrap();

    for status in [TaskStatus::Ready, TaskStatus::InProgress] {
        h.store
            .transition_task(&task.id, status, Actor::Daemon, None, None)
            .await
            .unwrap();
    }
    h.store
        .transition_task(
            &task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            Some(SUMMARY),
            None,
        )
        .await
        .unwrap();
    let session = h
        .launcher
        .spawn_reviewer(&task.id, &cast.reviewer.id)
        .await
        .unwrap();
    let reviewed = h.store.get_task(&task.id).await.unwrap();
    let expected = fill(
        &default_for(PromptKind::ReviewerBriefing),
        &[
            ("task_title", &reviewed.title),
            ("task_description", &reviewed.description),
            ("goal_title", &cast.goal.title),
            ("branch", &reviewed.branch),
            ("base_branch", &cast.repo.base_branch),
            ("repo_path", &cast.repo.path),
            ("summary", SUMMARY),
        ],
    );
    let launch = h.launch_file(&session.id).expect("a launch file");
    assert_eq!(
        launch.initial_prompt.as_deref(),
        Some(expected.as_str()),
        "the review-round briefing, assembled"
    );
    // The summary is what the author requested review with, undecorated:
    // it is the whole of what the reviewer is told.
    assert!(!expected.contains("Review requested:"));
}
