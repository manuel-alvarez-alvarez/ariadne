//! Integration tests for the documents a skill owns.
//!
//! The contract is that a skill carries one text — the one Ariadne ships until
//! somebody writes over it, resettable to that shipped text by dropping what
//! was written — and that a shipped skill is reset rather than deleted while
//! one of the user's own is deleted rather than reset. What an agent is
//! briefed with around a skill is Ariadne's own, and no route reads or writes
//! it.

use crate::common;

use axum::http::StatusCode;

use ariadne_api::skills::{SkillDto, SkillSeat};
use ariadne_core::{Seat, TaskStatus};
use ariadne_store::defaults::{
    BUILTIN_SKILLS, ORCHESTRATION_SKILL, PR_BABYSIT_SKILL, default_skill_document,
    default_system_prompt, skill_text,
};
use ariadne_store::{AgentSession, Task};

use common::{Harness, delete, get, harness, post, post_json, put_json};

/// A shipped skill runs on the text Ariadne ships and stores none of it; a
/// text written over it is the skill's own until the reset takes it back off.
#[tokio::test]
async fn a_shipped_skill_runs_on_the_document_ariadne_ships() {
    let h = harness().await;

    let shipped: SkillDto = h.json(get("/v1/skills/coding"), StatusCode::OK).await;
    assert_eq!(shipped.document, default_skill_document("coding").unwrap());
    assert!(shipped.document_is_default);
    assert!(shipped.builtin);

    let edited: SkillDto = h
        .json(
            put_json(
                "/v1/skills/coding",
                serde_json::json!({ "document": "---\nname: coding\ndescription: ours\n---\n" }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.summary, "ours");
    assert!(!edited.document_is_default);

    let reset: SkillDto = h
        .json(post("/v1/skills/coding/document/reset"), StatusCode::OK)
        .await;
    assert_eq!(reset.document, default_skill_document("coding").unwrap());
    assert!(
        reset.document_is_default,
        "the reset dropped the text rather than copying the default in"
    );
}

/// Every skill Ariadne ships is seeded into a fresh database, and every one of
/// them says in one line what it is for: the index an agent reads is that line
/// and nothing else, so a skill without one is a skill nobody can choose.
#[tokio::test]
async fn every_shipped_skill_is_seeded_and_describes_itself() {
    let h = harness().await;
    let skills: Vec<SkillDto> = h.json(get("/v1/skills"), StatusCode::OK).await;

    assert_eq!(skills.len(), BUILTIN_SKILLS.len());
    for shipped in &BUILTIN_SKILLS {
        let found = skills
            .iter()
            .find(|s| s.name == shipped.name)
            .unwrap_or_else(|| panic!("{} was not seeded", shipped.name));
        assert!(found.builtin);
        assert!(
            !found.summary.is_empty(),
            "{} describes itself nowhere",
            found.name
        );
    }
}

/// The API carries the seat a skill serves, so clients can keep the
/// orchestrator's playbook visible without offering it for task staffing.
#[tokio::test]
async fn a_skill_dto_names_the_seat_it_serves() {
    let h = harness().await;

    let orchestration: SkillDto = h
        .json(get("/v1/skills/orchestration"), StatusCode::OK)
        .await;
    let coding: SkillDto = h.json(get("/v1/skills/coding"), StatusCode::OK).await;

    assert_eq!(orchestration.seat, SkillSeat::Orchestrator);
    assert_eq!(coding.seat, SkillSeat::Task);
}

/// A skill of the user's own carries its own text, because nothing Ariadne
/// ships answers to its name — so there is nothing to reset it to, and the
/// refusal says as much rather than emptying it.
#[tokio::test]
async fn a_skill_of_your_own_is_deleted_rather_than_reset() {
    let h = harness().await;

    let mine: SkillDto = h
        .json(
            post_json(
                "/v1/skills",
                serde_json::json!({
                    "name": "api-design",
                    "document": "---\nname: api-design\ndescription: mine\n---\n",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert!(!mine.builtin);
    assert_eq!(mine.summary, "mine");

    let (status, _) = h.send(post("/v1/skills/api-design/document/reset")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = h.send(delete("/v1/skills/api-design")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = h.send(get("/v1/skills/api-design")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// And the other way round: a shipped skill is reset rather than deleted, so
/// the catalog cannot be emptied by mistake.
#[tokio::test]
async fn a_shipped_skill_cannot_be_deleted() {
    let h = harness().await;
    let (status, _) = h.send(delete("/v1/skills/coding")).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// The orchestrator's playbook is the `orchestration` skill, and it reaches
/// an orchestrator session the way a task agent's skills reach it: the system
/// prompt is the seat text and then an index naming the skill, and the path
/// the index names holds the document in the run directory.
#[tokio::test]
async fn an_orchestrator_session_indexes_the_orchestration_skill() {
    let h = harness().await;
    let goal = h.planning_goal().await;
    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();

    let run_dir = h.launcher.cfg.run_dir.join(&session.id);
    let system = h
        .launch_file(&session.id)
        .expect("a launch file")
        .system_prompt;
    let (owed, index) = system
        .split_once(
            "\n\nYour skills. Read the document of a skill before you do the work it covers:",
        )
        .expect("a skill index");
    assert_eq!(
        owed,
        default_system_prompt(Seat::Orchestrator).trim(),
        "the seat's own text, word for word out of the code"
    );
    let document = run_dir
        .join("skills")
        .join(ORCHESTRATION_SKILL)
        .join("SKILL.md");
    assert!(
        index.contains(&format!("- {ORCHESTRATION_SKILL}: "))
            && index.contains(&document.display().to_string()),
        "one line naming the skill and where its document is: {index}"
    );
    // And the path the line names is the document, not a promise.
    assert_eq!(
        std::fs::read_to_string(&document).expect("the document on disk"),
        skill_text(default_skill_document(ORCHESTRATION_SKILL).unwrap()),
        "the shipped playbook, written whole for the agent to open"
    );
}

/// The playbook is editable like any shipped skill, and an edit reaches the
/// next launch rather than the ones already running: the document written for
/// a fresh orchestrator session is what the store holds at that moment.
#[tokio::test]
async fn an_edited_orchestration_skill_reaches_the_next_launch() {
    let h = harness().await;
    let goal = h.planning_goal().await;

    let first = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    h.launcher.kill_session(&first.id).await.unwrap();

    const EDITED: &str = "---\nname: orchestration\ndescription: our playbook\n---\nAsk twice.\n";
    h.store
        .set_skill_document(ORCHESTRATION_SKILL, EDITED)
        .await
        .unwrap();

    let second = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    let document = h
        .launcher
        .cfg
        .run_dir
        .join(&second.id)
        .join("skills")
        .join(ORCHESTRATION_SKILL)
        .join("SKILL.md");
    assert_eq!(
        std::fs::read_to_string(&document).unwrap(),
        EDITED,
        "the next launch reads the edited document"
    );
}

/// The session of the agent of column `step`, once the scheduler has started
/// it: the goal is active and the task in progress, and the daemon moves it
/// forward to that column.
async fn column_session(h: &Harness, task: &Task, step: &str) -> AgentSession {
    h.advance_to(task, step).await;
    h.notify(&task.id);
    h.step_session(task, step).await
}

/// Where a session's launch wrote the document of `skill`.
fn skill_file(h: &Harness, session: &AgentSession, skill: &str) -> std::path::PathBuf {
    h.launcher
        .cfg
        .run_dir
        .join(&session.id)
        .join("skills")
        .join(skill)
        .join("SKILL.md")
}

/// A column's agent loads the skills it is staffed on, and no other: the
/// `pr` column of `develop-review-pr` stages `pr-babysit`, so its agent is
/// indexed on that document and nothing else, over the seat text every
/// column's agent owes. The `develop` column's agent is indexed on `coding`
/// and knows nothing of the request.
#[tokio::test]
async fn a_columns_agent_loads_its_staffed_skills_and_the_pr_column_loads_pr_babysit() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.active_cast_running(Some("develop-review-pr")).await;
    let pr = column_session(&h, &cast.task, "pr").await;

    let launch = h.launch_file(&pr.id).expect("a launch file");
    assert!(
        launch
            .system_prompt
            .starts_with(default_system_prompt(Seat::Agent).trim()),
        "the seat's own text first: {}",
        launch.system_prompt
    );
    assert!(
        launch
            .system_prompt
            .contains(&format!("- {PR_BABYSIT_SKILL}: ")),
        "{}",
        launch.system_prompt
    );
    assert!(
        !launch.system_prompt.contains("- coding: "),
        "the request column does not write code: {}",
        launch.system_prompt
    );
    let document = skill_file(&h, &pr, PR_BABYSIT_SKILL);
    assert_eq!(
        std::fs::read_to_string(&document).expect("the document on disk"),
        skill_text(default_skill_document(PR_BABYSIT_SKILL).unwrap())
    );
    assert!(!skill_file(&h, &pr, "coding").exists());
}

/// `pull-request` left the catalog: opening a request is the daemon's own
/// tool now, behind `open_pull_request`, so no agent reads a skill for it —
/// not even the agent of a `pr` column.
///
/// An old staffing on it is the kept-row case any dropped skill leaves
/// ([`crate::common::Harness::stage_dropped_skill`]): the row stays because
/// the agent still names it, and it reads as empty since nothing ships
/// under the name any more, so the copy written to the agent's run
/// directory is blank.
#[tokio::test]
async fn an_old_staffing_on_pull_request_reads_as_empty() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.active_cast().await;
    h.stage_dropped_skill(&cast.develop().id, "pull-request", 2)
        .await;
    h.advance(&cast.task, TaskStatus::InProgress).await;
    h.notify(&cast.task.id);
    let develop = h.step_session(&cast.task, "develop").await;

    assert_eq!(
        std::fs::read_to_string(skill_file(&h, &develop, "pull-request")).unwrap(),
        "",
        "an old staffing on pull-request does not read the removed skill"
    );
    assert!(
        !std::fs::read_to_string(skill_file(&h, &develop, "coding"))
            .unwrap()
            .is_empty(),
        "the column's own skill is read whole beside it"
    );
}

/// The system prompt of a seat is Ariadne's own and says nothing about the
/// work: what an agent can do arrives as an index of its skills, one line
/// each, with the document left on disk for the agent to read when it needs
/// it.
#[test]
fn a_seat_prompt_carries_only_what_the_seat_owes() {
    for seat in Seat::ALL {
        let prompt = default_system_prompt(seat);
        for shipped in &BUILTIN_SKILLS {
            assert!(
                !prompt.contains(shipped.document),
                "the {} prompt carries the {} document",
                seat.as_str(),
                shipped.name
            );
        }
    }
}
