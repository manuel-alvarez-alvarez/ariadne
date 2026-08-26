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

use axum::http::StatusCode;

use common::{Harness, harness, patch_json, post_json};

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
        .profile_on(
            "planner",
            Role::Planner,
            Some(AgentKind::ClaudeCode),
            Some("opus"),
        )
        .await;
    let engineer = h
        .profile_on(
            "engineer",
            Role::Engineer,
            Some(AgentKind::Codex),
            Some("gpt-5"),
        )
        .await;
    let strict = h
        .profile_on(
            "strict",
            Role::Reviewer,
            Some(AgentKind::ClaudeCode),
            Some("sonnet"),
        )
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

// -- the model the user chose -----------------------------------------------

/// The profiles a chosen model has to overrule: all three on claude_code, all
/// three on the same model, so nothing below can pass by reading a profile.
async fn on_claude(h: &Harness) -> (Profile, Profile, Profile, Profile) {
    let planner = h
        .profile_on(
            "planner",
            Role::Planner,
            Some(AgentKind::ClaudeCode),
            Some("claude-opus-5"),
        )
        .await;
    let engineer = h
        .profile_on(
            "engineer",
            Role::Engineer,
            Some(AgentKind::ClaudeCode),
            Some("claude-opus-5"),
        )
        .await;
    let chosen = h
        .profile_on(
            "chosen",
            Role::Reviewer,
            Some(AgentKind::ClaudeCode),
            Some("claude-opus-5"),
        )
        .await;
    let untouched = h
        .profile_on(
            "untouched",
            Role::Reviewer,
            Some(AgentKind::ClaudeCode),
            Some("claude-opus-5"),
        )
        .await;
    (planner, engineer, chosen, untouched)
}

/// A goal created with a model of its own plans on it — on the agent CLI that
/// model belongs to, which is not the one the planner profile is on.
#[tokio::test]
async fn a_goal_created_with_a_model_plans_on_it() {
    let h = harness().await;
    let (planner, ..) = on_claude(&h).await;
    let repo = h.repository(&h.dir.path().join("repo")).await;

    let goal: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "planner_profile": planner.name,
                    "model": "gpt-5.3-codex",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal.agent_kind, Some(AgentKind::Codex));
    assert_eq!(goal.model.as_deref(), Some("gpt-5.3-codex"));

    let session = h.launcher.spawn_planner(&goal.id).await.unwrap();
    assert_eq!(session.agent_kind(), AgentKind::Codex);
    assert_eq!(session.model.as_deref(), Some("gpt-5.3-codex"));
    let argv = h.spawn_argv(&session.id);
    assert!(argv.starts_with("codex "), "{argv}");
    assert!(argv.contains("gpt-5.3-codex"), "{argv}");
}

/// A task created with models runs each agent on its own: the engineer on the
/// one it was given, the reviewer that was given one on that, and the reviewer
/// that was given none on its profile's.
#[tokio::test]
async fn a_task_created_with_models_runs_each_agent_on_its_own() {
    let h = harness().await;
    let (planner, engineer, chosen, untouched) = on_claude(&h).await;
    let repo_path = h.git_repo("repo");
    let repo = h.repository(&repo_path).await;
    let goal = h.goal_on(&planner, &repo, 1).await;

    let task: TaskDto = h
        .json(
            post_json(
                &format!("/v1/goals/{}/tasks", goal.id),
                serde_json::json!({
                    "title": "Do the thing",
                    "engineer_profile": engineer.name,
                    "model": "gpt-5.6-sol",
                    "reviewers": [
                        {"profile": chosen.name, "model": "ollama/llama3:8b"},
                        {"profile": untouched.name},
                    ],
                }),
            ),
            StatusCode::CREATED,
        )
        .await;

    assert_eq!(task.agent_kind, Some(AgentKind::Codex));
    assert_eq!(task.model.as_deref(), Some("gpt-5.6-sol"));
    let pins: Vec<_> = task
        .reviewers
        .iter()
        .map(|r| (r.profile_id.as_str(), r.agent_kind, r.model.as_deref()))
        .collect();
    assert_eq!(
        pins,
        vec![
            (
                chosen.id.as_str(),
                Some(AgentKind::Opencode),
                Some("ollama/llama3:8b")
            ),
            (
                untouched.id.as_str(),
                Some(AgentKind::ClaudeCode),
                Some("claude-opus-5")
            ),
        ]
    );

    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    assert_eq!(session.agent_kind(), AgentKind::Codex);
    assert_eq!(session.model.as_deref(), Some("gpt-5.6-sol"));
    let session = h
        .launcher
        .spawn_reviewer(&task.id, &chosen.id)
        .await
        .unwrap();
    assert_eq!(session.agent_kind(), AgentKind::Opencode);
    assert_eq!(session.model.as_deref(), Some("ollama/llama3:8b"));
}

/// Editing a pending task moves its models, and "default" hands them back to
/// the profiles — as those profiles stand at that moment, which is what
/// reassigning a reviewer has always done.
#[tokio::test]
async fn an_edit_moves_the_models_and_default_hands_them_back() {
    let h = harness().await;
    let (planner, engineer, chosen, _) = on_claude(&h).await;
    let repo = h.repository(&h.dir.path().join("repo")).await;
    let goal = h.goal_on(&planner, &repo, 1).await;
    let task = h
        .task_on(&goal, &repo, "Do it", &engineer, &[&chosen])
        .await;

    let moved: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({
                    "model": "gpt-5.3-codex",
                    "reviewers": [{"profile": chosen.name, "model": "o3"}],
                }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(moved.agent_kind, Some(AgentKind::Codex));
    assert_eq!(moved.model.as_deref(), Some("gpt-5.3-codex"));
    assert_eq!(moved.reviewers[0].agent_kind, Some(AgentKind::Codex));
    assert_eq!(moved.reviewers[0].model.as_deref(), Some("o3"));

    // An edit about something else leaves the choice standing.
    let renamed: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({"title": "Do it well"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(renamed.model.as_deref(), Some("gpt-5.3-codex"));

    // The profiles have moved since the task was cut, and handing the models
    // back hands back what they are on now.
    h.move_profile(
        &engineer.id,
        Some(AgentKind::Opencode),
        Some("ollama/llama3:8b"),
    )
    .await;
    h.move_profile(
        &chosen.id,
        Some(AgentKind::ClaudeCode),
        Some("claude-haiku-4-5"),
    )
    .await;
    let back: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({
                    "model": "default",
                    "reviewers": [{"profile": chosen.name}],
                }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(back.agent_kind, Some(AgentKind::Opencode));
    assert_eq!(back.model.as_deref(), Some("ollama/llama3:8b"));
    assert_eq!(back.reviewers[0].agent_kind, Some(AgentKind::ClaudeCode));
    assert_eq!(back.reviewers[0].model.as_deref(), Some("claude-haiku-4-5"));
}

/// The user picks models and nothing else, so an id nothing can place is
/// refused by name rather than stored for a spawn to trip over. A model the
/// catalog has never heard of but whose vendor spells it plainly is placed all
/// the same.
#[tokio::test]
async fn a_model_nothing_can_place_is_refused_by_name() {
    let h = harness().await;
    let (planner, engineer, chosen, _) = on_claude(&h).await;
    let repo = h.repository(&h.dir.path().join("repo")).await;

    let goal_with = |model: serde_json::Value| {
        post_json(
            "/v1/goals",
            serde_json::json!({
                "title": "Ship it",
                "repository_ids": [repo.id],
                "planner_profile": planner.name,
                "model": model,
            }),
        )
    };

    let err = h
        .error(goal_with("llama3".into()), StatusCode::BAD_REQUEST)
        .await;
    assert!(
        err.error.message.contains("unknown model `llama3`"),
        "{}",
        err.error.message
    );

    // Empty is a field somebody meant to fill in, not a way to say "the
    // profile's" — that is what leaving it out says.
    let err = h.error(goal_with("".into()), StatusCode::BAD_REQUEST).await;
    assert!(err.error.message.contains("empty"), "{}", err.error.message);

    // Released after this build, and still placed by how Anthropic spells it.
    let goal: GoalDto = h
        .json(
            goal_with("claude-opus-9-20991231".into()),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal.agent_kind, Some(AgentKind::ClaudeCode));
    assert_eq!(goal.model.as_deref(), Some("claude-opus-9-20991231"));

    // The same refusal on the way in for a task, engineer and reviewer alike.
    let task_with =
        |body: serde_json::Value| post_json(&format!("/v1/goals/{}/tasks", goal.id), body);
    let err = h
        .error(
            task_with(serde_json::json!({
                "title": "Do the thing",
                "engineer_profile": engineer.name,
                "model": "llama3",
                "reviewers": [{"profile": chosen.name}],
            })),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("unknown model `llama3`"),
        "{}",
        err.error.message
    );
    let err = h
        .error(
            task_with(serde_json::json!({
                "title": "Do the thing",
                "engineer_profile": engineer.name,
                "reviewers": [{"profile": chosen.name, "model": "llama3"}],
            })),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("unknown model `llama3`"),
        "{}",
        err.error.message
    );
}
