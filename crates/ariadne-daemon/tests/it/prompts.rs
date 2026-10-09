//! The prompts an agent is launched with are Ariadne's own templates.
//!
//! One thing has to hold: the built-in template of the kind is what the
//! session gets, with nothing of the database between the two — a profile
//! carries a system prompt and no lifecycle text at all.
//!
//! And what assembly comes to is pinned here too: the placeholders of the
//! orchestrator's briefing, a column's first briefing, a return to a column
//! and a nudge are filled in by hand and compared with what the daemon
//! produced, so a change to the assembler shows up as a diff rather than as
//! an agent quietly briefed with something else.
//!
//! The agent is the harness's stub. The orchestrator's briefing is read back
//! from its launch file — what the agent was told at its launch — and a
//! column's from the prompts the scheduler hands it, since a column's agent is
//! launched bare and briefed afterwards. `git` is real: starting a column's
//! agent creates the task's worktree.

use crate::common;

use crate::common::{post_json, sh};
use ariadne_core::{PromptKind, Seat};
use ariadne_daemon::scheduler::QUIET_NUDGE_SECS;
use ariadne_store::defaults::default_prompt_text;
use axum::http::StatusCode;
use serde_json::json;

use common::{Cast, Harness, TIMEOUT, eventually, harness};

/// A task ready for its first column's agent to be started, in a real repo,
/// on a goal under way.
async fn seeded(h: &Harness) -> Cast {
    h.git_repo("repo");
    h.active_cast().await
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

/// Every placeholder `kind` declares, as it would read in a template.
fn tokens_of(kind: PromptKind) -> Vec<String> {
    kind.placeholders()
        .iter()
        .map(|name| format!("{{{name}}}"))
        .collect()
}

/// The commit the `develop` column's gate asks for, made in the task's
/// worktree.
fn commit_in(worktree: &str) {
    sh(
        std::path::Path::new(worktree),
        "echo change > feature && git add feature && \
         git -c user.name=Test -c user.email=test@test commit -qm 'feat: add the feature'",
    );
}

#[tokio::test]
async fn an_issue_goal_keeps_its_url_and_briefs_the_orchestrator_to_close_it() {
    let h = harness().await;
    let (_, repository) = h.goal().await;
    let url = "https://github.com/acme/widgets/issues/12";
    let goal: ariadne_api::goals::GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                json!({
                    "title": "Fix issue", "description": "Issue body", "issue_url": url,
                    "repository_ids": [repository.id], "model": "stub:test-model"
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    let read = h.store.get_goal(&goal.id).await.unwrap();
    assert_eq!(read.issue_url.as_deref(), Some(url));
    let dto: ariadne_api::goals::GoalDto = h.get(&format!("/v1/goals/{}", goal.id)).await;
    assert_eq!(dto.issue_url.as_deref(), Some(url));
    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    let briefing = h.launch_file(&session.id).unwrap().initial_prompt.unwrap();
    assert!(
        briefing.contains(&format!("This goal comes from {url}")),
        "{briefing}"
    );
    assert!(briefing.contains(&format!("Closes {url}")), "{briefing}");
}

/// The orchestrator is briefed with the goal's workflow and every column of
/// it, one line each — what it staffs every task against — and with no
/// landing: a goal lands the way its last column says, not the way a line of
/// its briefing does. The whole text is the built-in template with every
/// placeholder filled in, exactly.
#[tokio::test]
async fn the_orchestrator_briefing_names_the_workflow_and_its_columns_and_no_landing() {
    let h = harness().await;
    let (goal, repo) = h.goal().await;
    let steps = h.store.goal_steps(&goal.id).await.unwrap();
    assert_eq!(steps.len(), 3, "the default workflow has three columns");

    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    let briefing = h.launch_file(&session.id).unwrap().initial_prompt.unwrap();

    let columns = steps
        .iter()
        .map(|s| {
            let skills: Vec<String> = serde_json::from_str(&s.skills).unwrap();
            let mut line = format!("- {} [{}]: {}", s.id, s.title, s.description);
            if !skills.is_empty() {
                line.push_str(&format!(" Skills: {}.", skills.join(", ")));
            }
            if let Some(rank) = &s.rank {
                line.push_str(&format!(" Rank: {rank}."));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = fill(
        &default_for(PromptKind::OrchestratorBriefing),
        &[
            ("goal_title", &goal.title),
            ("goal_description", &goal.description),
            ("workflow", &goal.workflow),
            ("columns", &columns),
            (
                "repositories",
                &format!("- {} (base branch: {})", repo.path, repo.base_branch),
            ),
        ],
    );
    assert_eq!(briefing, expected, "the built-in briefing, rendered");
    assert!(
        briefing.contains("Workflow: develop-review-merge"),
        "{briefing}"
    );
    for column in [
        "- develop [Develop]:",
        "- review [Review]:",
        "- merge [Merge]:",
    ] {
        assert!(
            briefing.lines().any(|line| line.starts_with(column)),
            "no line for {column}: {briefing}"
        );
    }
    assert!(!briefing.contains("Landing"), "{briefing}");
}

/// The first briefing of a column is the built-in template with every
/// placeholder filled in, exactly, handed to the agent as its first prompt
/// — behind a system layer that is the seat's own text and the index of the
/// agent's skills.
#[tokio::test]
async fn a_started_column_agent_is_briefed_from_the_builtin_template() {
    let h = harness().scheduler().await;
    let cast = seeded(&h).await;

    let session = h.step_session(&cast.task, "develop").await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let step = h
        .store
        .goal_steps(&cast.goal.id)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.id == "develop")
        .unwrap();
    let expected = fill(
        &default_for(PromptKind::StepBriefing),
        &[
            ("task_title", &task.title),
            ("task_description", &task.description),
            ("goal_title", &cast.goal.title),
            ("worktree_path", task.worktree_path.as_deref().unwrap()),
            ("branch", &task.branch),
            ("base_branch", &cast.repo.base_branch),
            ("repo_path", &cast.repo.path),
            ("step_id", &step.id),
            ("step_title", &step.title),
            ("step_description", &step.description),
            // A first column has nothing before it, and this task waits on
            // nothing.
            ("previous_summary", ""),
            ("dependencies", ""),
        ],
    );
    // The first prompt of a launch carries the seat text and the skill index
    // ahead of the briefing, since ACP has no system prompt of its own; the
    // briefing is what ends it.
    let prompts = h.prompts_to(&session);
    let first = prompts.first().map(String::as_str).unwrap_or_default();
    assert!(
        first.ends_with(&expected),
        "the default briefing, assembled, ends the first prompt: {first}"
    );

    // The launch itself carries no briefing: the column's prompt is the
    // scheduler's, so a relaunch is never briefed twice.
    let launch = h.launch_file(&session.id).expect("a launch file");
    assert!(
        launch
            .initial_prompt
            .as_deref()
            .unwrap_or_default()
            .is_empty(),
        "{:?}",
        launch.initial_prompt
    );

    // The system layer is what the seat owes, and then the index of the skills
    // this agent loads: one line each, with the document left on disk for the
    // agent to read when it needs it.
    let run_dir = h.launcher.cfg.run_dir.join(&session.id);
    let (owed, index) = launch
        .system_prompt
        .split_once(
            "\n\nYour skills. Read the document of a skill before you do the work it covers:",
        )
        .expect("a skill index");
    assert_eq!(
        owed,
        ariadne_store::defaults::default_system_prompt(Seat::Agent).trim(),
        "the seat's own text, word for word out of the code"
    );
    let shipped = ariadne_store::defaults::default_skill_document("coding").unwrap();
    let document = run_dir.join("skills").join("coding").join("SKILL.md");
    assert_eq!(
        index.trim(),
        format!(
            "- coding: {} ({})",
            ariadne_store::defaults::skill_summary(shipped).unwrap(),
            document.display()
        ),
        "one line per skill: its summary, and where its document is"
    );
    // And the path the line names is a document, not a promise.
    let written = std::fs::read_to_string(&document).expect("the skill document on disk");
    assert_eq!(
        written,
        ariadne_store::defaults::skill_text(shipped),
        "the shipped document, written whole for the agent to open"
    );
}

/// The other two texts a column's agent meets, pinned the same way: what it
/// is told when the task comes back to its column, and what it is nudged with
/// when it has gone quiet with the task still in front of it.
#[tokio::test]
async fn a_return_and_a_nudge_assemble_word_for_word() {
    let h = harness().scheduler().await;
    let cast = seeded(&h).await;
    let develop = h.step_session(&cast.task, "develop").await;
    commit_in(develop.worktree_path.as_deref().unwrap());
    h.complete_step(&cast.task, &develop, "The feature is committed.")
        .await;
    let review = h.step_session(&cast.task, "review").await;

    const REASON: &str = "Fix the edge case the test names.";
    h.fail_step(&cast.task, &review, REASON).await;
    let expected = fill(
        &default_for(PromptKind::StepReturn),
        &[
            ("task_title", &cast.task.title),
            ("step_title", "Develop"),
            ("direction", "back"),
            ("reason", REASON),
        ],
    );
    // The stub records the seat text ahead of every prompt it is sent, so
    // the briefing is what a prompt ends with.
    eventually(TIMEOUT, "the return briefing", || async {
        h.prompts_to(&develop)
            .iter()
            .any(|prompt| prompt.ends_with(&expected))
    })
    .await;

    // Quiet for longer than the nudge threshold, with the task still in its
    // column: the nudge is the resume text, rendered for this task.
    eventually(TIMEOUT, "the develop agent to finish its turn", || async {
        h.session_status(&develop).await == ariadne_core::SessionStatus::Idle
    })
    .await;
    let expected = fill(
        &default_for(PromptKind::AgentResume),
        &[("task_title", &cast.task.title), ("step_title", "Develop")],
    );
    h.backdate(
        &["last_activity_at", "launched_at"],
        &develop,
        QUIET_NUDGE_SECS + 5,
    )
    .await;
    h.notify(&cast.task.id);
    eventually(TIMEOUT, "the nudge", || async {
        h.prompts_to(&develop)
            .iter()
            .any(|prompt| prompt.ends_with(&expected))
    })
    .await;
}

/// Every placeholder a kind declares is one its assembler fills in: the
/// built-in template of every kind validates against its kind, and no
/// `{token}` of the kind is left in a briefing the daemon rendered.
#[tokio::test]
async fn every_allowed_placeholder_is_one_a_briefing_fills_in() {
    for kind in [
        PromptKind::OrchestratorBriefing,
        PromptKind::OrchestratorResume,
        PromptKind::GoalAttention,
        PromptKind::IncomingMessage,
        PromptKind::StepBriefing,
        PromptKind::StepReturn,
        PromptKind::AgentResume,
    ] {
        kind.validate_template(default_prompt_text(kind))
            .unwrap_or_else(|e| panic!("{}: {e}", kind.as_str()));
    }

    let h = harness().scheduler().await;
    let cast = seeded(&h).await;
    // The scheduler starts the goal's orchestrator on its own.
    let launched = async || {
        h.sessions_of_goal(&cast.goal.id)
            .await
            .into_iter()
            .find(|s| s.seat() == Some(Seat::Orchestrator) && h.launch_file(&s.id).is_some())
    };
    eventually(TIMEOUT, "the orchestrator to be launched", || async {
        launched().await.is_some()
    })
    .await;
    let briefing = h
        .launch_file(&launched().await.unwrap().id)
        .unwrap()
        .initial_prompt
        .unwrap();
    for token in tokens_of(PromptKind::OrchestratorBriefing) {
        assert!(!briefing.contains(&token), "{token} left in: {briefing}");
    }

    let develop = h.step_session(&cast.task, "develop").await;
    let briefing = h.prompted(&develop);
    for token in tokens_of(PromptKind::StepBriefing) {
        assert!(!briefing.contains(&token), "{token} left in: {briefing}");
    }
}
