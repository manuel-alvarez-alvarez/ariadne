//! What the API says a task's and a goal's agents run on.
//!
//! The pins live on `tasks`, `task_reviewers` and `goals`, and the launcher
//! spawns from them — but a surface that keeps reading the profile still shows
//! the wrong answer the moment that profile is edited, which is exactly when
//! the question gets asked. So these check the DTOs the CLI and the web read:
//! the engineer's pin, one pin per reviewer slot and the planner's, all of them
//! surviving a profile moved to another agent and another model.
//!
//! No tmux, no git, no agent CLI: nothing here launches anything.

mod common;

use ariadne_api::goals::GoalDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{AgentKind, Role};
use ariadne_store::{Goal, Profile, ProfileUpdate, Task};

use common::{Harness, harness};

/// A seeded goal and task, with the reviewer profiles the slots were cut from.
struct Seeded {
    goal: Goal,
    task: Task,
    strict: Profile,
    auto: Profile,
}

/// A goal and a task, each agent on a different agent CLI and model so no
/// assertion can pass by reading somebody else's pin. The reviewers are two:
/// one pinned to a model, one left on the agent's default and on auto. Every
/// profile then moves, after the goal and the task were created from them.
async fn seeded(h: &Harness) -> Seeded {
    let planner = h
        .profile_on("planner", Role::Planner, Some(AgentKind::ClaudeCode), Some("opus"))
        .await;
    let engineer = h
        .profile_on("engineer", Role::Engineer, Some(AgentKind::Codex), Some("gpt-5"))
        .await;
    let strict = h
        .profile_on("strict", Role::Reviewer, Some(AgentKind::ClaudeCode), Some("sonnet"))
        .await;
    let auto = h.profile_on("auto", Role::Reviewer, None, None).await;

    let (goal, repo) = h.goal(&planner).await;
    let task = h
        .task_on(&goal, &repo, "Surfaces", &engineer, &[&strict, &auto])
        .await;

    for (profile, kind, model) in [
        (&planner, AgentKind::Opencode, "grok"),
        (&engineer, AgentKind::ClaudeCode, "haiku"),
        (&strict, AgentKind::Codex, "gpt-5-mini"),
        (&auto, AgentKind::Codex, "gpt-5-mini"),
    ] {
        h.store
            .update_profile(
                &profile.id,
                ProfileUpdate {
                    agent_kind: Some(Some(kind)),
                    model: Some(Some(model.into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    Seeded {
        goal,
        task,
        strict,
        auto,
    }
}

/// Each slot answers for itself: the engineer's pin, and two reviewers moved
/// onto the same agent and model still reading back as what each was assigned
/// with, in review order.
#[tokio::test]
async fn a_task_carries_the_pins_its_profiles_no_longer_have() {
    let h = harness().await;
    let Seeded {
        task, strict, auto, ..
    } = seeded(&h).await;

    let dto: TaskDto = h.get(&format!("/v1/tasks/{}", task.id)).await;
    assert_eq!(dto.agent_kind, Some(AgentKind::Codex));
    assert_eq!(dto.model.as_deref(), Some("gpt-5"));
    let pins: Vec<_> = dto
        .reviewers
        .iter()
        .map(|r| (r.profile_id.as_str(), r.agent_kind, r.model.as_deref()))
        .collect();
    assert_eq!(
        pins,
        // The second was created on auto with no model, and auto is a pin
        // like any other: it stays auto, not the agent the profile moved to.
        vec![
            (
                strict.id.as_str(),
                Some(AgentKind::ClaudeCode),
                Some("sonnet")
            ),
            (auto.id.as_str(), None, None),
        ]
    );
}

#[tokio::test]
async fn a_goal_carries_the_planner_pin_its_profile_no_longer_has() {
    let h = harness().await;
    let Seeded { goal, .. } = seeded(&h).await;

    let dto: GoalDto = h.get(&format!("/v1/goals/{}", goal.id)).await;
    assert_eq!(dto.agent_kind, Some(AgentKind::ClaudeCode));
    assert_eq!(dto.model.as_deref(), Some("opus"));
}

/// The list is what the board reads, and it goes through the same conversion:
/// a pin that only the single-task route carried would be a hole in the UI.
#[tokio::test]
async fn the_task_list_carries_the_pins_too() {
    let h = harness().await;
    let Seeded { task, .. } = seeded(&h).await;

    let listed: Vec<TaskDto> = h.get("/v1/tasks").await;
    let found = listed.iter().find(|t| t.id == task.id).expect("the task");
    assert_eq!(found.agent_kind, Some(AgentKind::Codex));
    assert_eq!(found.model.as_deref(), Some("gpt-5"));
    assert_eq!(found.reviewers.len(), 2);
}
