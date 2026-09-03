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
//! No tmux and no agent CLI: `tmux` is a stub that records the commands the
//! launcher issues, and the rendered briefing is read back from the session's
//! spawn plan — tmux is handed `ariadne _spawn <plan>` and nothing of the
//! briefing itself. `git` is real — spawning an engineer creates its worktree.

mod common;

use ariadne_core::{Actor, PromptKind, TaskStatus};
use ariadne_daemon::agents::prompts;
use ariadne_store::defaults::default_prompt_text;

use common::{Cast, Harness, harness};

/// What the engineer requested review with, and what it wrote afterwards:
/// the briefing has to carry the first and never the second.
const SUMMARY: &str = "Rendered the board from the store, with a test per lane.";

/// A task ready for its engineer to be spawned, in a real repo.
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
/// placeholders and all — and nowhere else: a briefing in the tmux command
/// line is what the plan file exists to prevent, whatever its size.
#[tokio::test]
async fn a_spawned_engineer_is_briefed_from_the_builtin_template() {
    let h = harness().await;
    let cast = seeded(&h).await;

    let session = h.launcher.spawn_engineer(&cast.task.id).await.unwrap();
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let briefing = prompts::engineer_briefing(
        prompts::template_for(PromptKind::EngineerBriefing),
        &task,
        &cast.goal,
        &cast.repo,
        &[],
    );
    let plan = h.spawn_plan(&session.id).expect("a spawn plan");
    assert!(
        plan.argv.iter().any(|arg| arg == &briefing),
        "the built-in briefing, rendered: {:?}",
        plan.argv
    );
    let log = h.tmux_calls().join("\n");
    assert!(
        !log.contains(&briefing),
        "the briefing reached the tmux command line: {log}"
    );

    // The system layer is the profile's prompt as it stands — no playbook
    // appended to it by the daemon any more.
    let system = std::fs::read_to_string(
        h.launcher
            .cfg
            .run_dir
            .join(&session.id)
            .join("system-prompt.md"),
    )
    .unwrap();
    assert_eq!(system, "You are engineer.");
}

/// The code's text is what reaches the agent, without anything having been
/// copied into the database first — and what reaches it is the built-in
/// template with every placeholder filled in, exactly.
#[tokio::test]
async fn a_spawn_assembles_the_default_briefing_word_for_word() {
    let h = harness().await;
    let cast = seeded(&h).await;

    let session = h.launcher.spawn_engineer(&cast.task.id).await.unwrap();
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let expected = fill(
        &default_for(PromptKind::EngineerBriefing),
        &[
            ("task_title", &task.title),
            ("task_description", &task.description),
            ("goal_title", &cast.goal.title),
            ("worktree_path", task.worktree_path.as_deref().unwrap()),
            ("branch", &task.branch),
            ("base_branch", &cast.repo.base_branch),
            ("repo_path", &cast.repo.path),
            ("merge_strategy", cast.repo.merge_strategy().as_str()),
            ("dependencies", "none"),
        ],
    );

    let plan = h.spawn_plan(&session.id).expect("a spawn plan");
    assert!(
        plan.argv.iter().any(|arg| arg == &expected),
        "the default briefing, assembled: {:?}",
        plan.argv
    );
    // The same text is what the assembler answers on its own, so nothing
    // between the two decorates it.
    assert_eq!(
        prompts::engineer_briefing(
            &default_for(PromptKind::EngineerBriefing),
            &task,
            &h.store.get_goal(&task.goal_id).await.unwrap(),
            &cast.repo,
            &[],
        ),
        expected
    );
}

/// The other two assemblies an agent meets, pinned the same way: what an
/// engineer holding unfinished work is picked up with, and what a reviewer
/// owing a verdict is.
#[tokio::test]
async fn a_resume_and_a_review_round_assemble_word_for_word() {
    let h = harness().await;
    let cast = seeded(&h).await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();

    assert_eq!(
        prompts::engineer_resume_briefing(
            &default_for(PromptKind::EngineerResume),
            &task,
        ),
        fill(
            &default_for(PromptKind::EngineerResume),
            &[("task_title", &task.title), ("branch", &task.branch)],
        )
    );

    let round = task.review_round.to_string();
    assert_eq!(
        prompts::reviewer_resume_briefing(
            &default_for(PromptKind::ReviewerResume),
            &task,
            Some(SUMMARY),
        ),
        fill(
            &default_for(PromptKind::ReviewerResume),
            &[
                ("review_round", &round),
                ("task_title", &task.title),
                ("branch", &task.branch),
                ("summary", SUMMARY),
            ],
        )
    );
    // A round nobody wrote a summary for still says so in words.
    assert!(
        prompts::reviewer_resume_briefing(
            &default_for(PromptKind::ReviewerResume),
            &task,
            None,
        )
        .contains("(none provided)")
    );
}

/// The `{summary}` a reviewer is briefed with is the one the engineer
/// requested review with — the round's own record of it — and not whatever
/// the engineer happened to say last.
///
/// The two are only the same until the engineer writes anything else: a
/// "thanks, will do" posted after the request would otherwise be what the
/// reviewers, and the people reading a published request, are handed as the
/// summary of the change.
#[tokio::test]
async fn a_reviewer_is_briefed_with_the_summary_review_was_requested_with() {
    let h = harness().await;
    let cast = seeded(&h).await;
    let task = &cast.task;
    // The engineer's worktree is what creates the branch the reviewer's is
    // cut from.
    h.launcher.spawn_engineer(&task.id).await.unwrap();

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
            Actor::Engineer,
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
            ("review_round", &reviewed.review_round.to_string()),
            ("task_description", &reviewed.description),
            ("goal_title", &cast.goal.title),
            ("branch", &reviewed.branch),
            ("base_branch", &cast.repo.base_branch),
            ("repo_path", &cast.repo.path),
            ("summary", SUMMARY),
        ],
    );
    let plan = h.spawn_plan(&session.id).expect("a spawn plan");
    assert!(
        plan.argv.iter().any(|arg| arg == &expected),
        "the review-round briefing, assembled: {:?}",
        plan.argv
    );
    // The summary is what the engineer requested review with, undecorated:
    // it is the whole of what the reviewer is told.
    assert!(!expected.contains("Review requested:"));
}
