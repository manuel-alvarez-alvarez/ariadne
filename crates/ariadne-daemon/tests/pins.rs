//! What the API says a task's and a goal's agents run on.
//!
//! The pins live on `tasks`, `task_reviewers` and `goals`, and the launcher
//! spawns from them — but a surface that keeps reading the profile still shows
//! the wrong answer the moment that profile is edited, which is exactly when
//! the question gets asked. So these check the DTOs the CLI and the web read:
//! the engineer's pin, one pin per reviewer slot and the planner's, all of them
//! surviving a profile moved to another agent and another model.
//!
//! One field carries the whole choice, `<agent_kind>[:<model>]`, on the way in
//! and on the way out: an agent CLI on its own is that CLI on its own default
//! model, and null is auto. The effort rides in the field beside it, checked
//! against the model it is to run at before anything is written.
//!
//! No tmux, no git, no agent CLI: nothing here launches anything.

mod common;

use ariadne_api::goals::GoalDto;
use ariadne_api::profiles::ProfileDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{AgentKind, Role};
use ariadne_store::{Goal, Profile, ProfileUpdate, Task};

use axum::http::StatusCode;

use common::{Harness, harness, patch_json, post_json, put_json};

/// A seeded goal and task, with the reviewer profiles the slots were cut from.
struct Seeded {
    goal: Goal,
    task: Task,
    strict: Profile,
    auto: Profile,
}

/// A profile on an agent, a model and an effort of that model: the three
/// halves of a pin, so that nothing below can pass by carrying two of them.
async fn profile_at(
    h: &Harness,
    name: &str,
    role: Role,
    kind: AgentKind,
    model: &str,
    effort: &str,
) -> Profile {
    let profile = h.profile_on(name, role, Some(kind), Some(model)).await;
    h.store
        .update_profile(
            &profile.id,
            ProfileUpdate {
                effort: Some(Some(effort.into())),
                ..Default::default()
            },
        )
        .await
        .unwrap()
}

/// A goal and a task, each agent on a different agent CLI, model and effort so
/// no assertion can pass by reading somebody else's pin. The reviewers are two:
/// one pinned to a model, one left on the agent's default and on auto. Every
/// profile then moves, after the goal and the task were created from them.
async fn seeded(h: &Harness) -> Seeded {
    let planner = profile_at(
        h,
        "planner",
        Role::Planner,
        AgentKind::ClaudeCode,
        "opus",
        "high",
    )
    .await;
    let engineer = profile_at(
        h,
        "engineer",
        Role::Engineer,
        AgentKind::Codex,
        "gpt-5",
        "medium",
    )
    .await;
    let strict = profile_at(
        h,
        "strict",
        Role::Reviewer,
        AgentKind::ClaudeCode,
        "sonnet",
        "low",
    )
    .await;
    let auto = h.profile_on("auto", Role::Reviewer, None, None).await;

    let (goal, repo) = h.goal(&planner).await;
    let task = h
        .task_on(&goal, &repo, "Surfaces", &engineer, &[&strict, &auto])
        .await;

    for (profile, kind, model, effort) in [
        (&planner, AgentKind::Opencode, "grok", "fast"),
        (&engineer, AgentKind::ClaudeCode, "haiku", "max"),
        (&strict, AgentKind::Codex, "gpt-5-mini", "minimal"),
        (&auto, AgentKind::Codex, "gpt-5-mini", "minimal"),
    ] {
        h.store
            .update_profile(
                &profile.id,
                ProfileUpdate {
                    agent_kind: Some(Some(kind)),
                    model: Some(Some(model.into())),
                    effort: Some(Some(effort.into())),
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
    assert_eq!(dto.model.as_deref(), Some("codex:gpt-5"));
    assert_eq!(dto.effort.as_deref(), Some("medium"));
    let pins: Vec<_> = dto
        .reviewers
        .iter()
        .map(|r| {
            (
                r.profile_id.as_str(),
                r.model.as_deref(),
                r.effort.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        pins,
        // The second was created on auto with no model, and auto is a pin
        // like any other: it stays auto, not the agent the profile moved to.
        vec![
            (strict.id.as_str(), Some("claude_code:sonnet"), Some("low")),
            (auto.id.as_str(), None, None),
        ]
    );
}

#[tokio::test]
async fn a_goal_carries_the_planner_pin_its_profile_no_longer_has() {
    let h = harness().await;
    let Seeded { goal, .. } = seeded(&h).await;

    let dto: GoalDto = h.get(&format!("/v1/goals/{}", goal.id)).await;
    assert_eq!(dto.model.as_deref(), Some("claude_code:opus"));
    assert_eq!(dto.effort.as_deref(), Some("high"));
}

/// The list is what the board reads, and it goes through the same conversion:
/// a pin that only the single-task route carried would be a hole in the UI.
#[tokio::test]
async fn the_task_list_carries_the_pins_too() {
    let h = harness().await;
    let Seeded { task, .. } = seeded(&h).await;

    let listed: Vec<TaskDto> = h.get("/v1/tasks").await;
    let found = listed.iter().find(|t| t.id == task.id).expect("the task");
    assert_eq!(found.model.as_deref(), Some("codex:gpt-5"));
    assert_eq!(found.effort.as_deref(), Some("medium"));
    assert_eq!(found.reviewers.len(), 2);
}

// -- the agent, and the model, the user chose -------------------------------

/// The profiles a chosen agent has to overrule: all three on claude_code, all
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

/// A goal created with an agent and a model of its own plans on them, whatever
/// the planner profile is on.
#[tokio::test]
async fn a_goal_created_with_an_agent_and_a_model_plans_on_them() {
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
                    "model": "codex:gpt-5.3-codex",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal.model.as_deref(), Some("codex:gpt-5.3-codex"));

    let session = h.launcher.spawn_planner(&goal.id).await.unwrap();
    assert_eq!(session.agent_kind(), AgentKind::Codex);
    assert_eq!(session.model.as_deref(), Some("gpt-5.3-codex"));
    let argv = h.spawn_argv(&session.id);
    assert!(argv.starts_with("codex "), "{argv}");
    assert!(argv.contains("gpt-5.3-codex"), "{argv}");
}

/// The agent is the choice and the model only narrows it: an agent named on
/// its own pins that CLI with no model, which is what runs it on its own
/// default — on a goal, on a task's engineer and on a reviewer slot alike.
#[tokio::test]
async fn an_agent_alone_pins_it_with_no_model_of_its_own() {
    let h = harness().await;
    let (planner, engineer, chosen, untouched) = on_claude(&h).await;
    let repo = h.repository(&h.git_repo("repo")).await;

    let goal: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "planner_profile": planner.name,
                    "model": "opencode",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal.model.as_deref(), Some("opencode"));

    let task: TaskDto = h
        .json(
            post_json(
                &format!("/v1/goals/{}/tasks", goal.id),
                serde_json::json!({
                    "title": "Do the thing",
                    "engineer_profile": engineer.name,
                    "model": "codex",
                    "reviewers": [
                        {"profile": chosen.name, "model": "opencode"},
                        {"profile": untouched.name},
                    ],
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(task.model.as_deref(), Some("codex"));
    assert_eq!(task.reviewers[0].model.as_deref(), Some("opencode"));
    // The slot that chose nothing is the one still on its profile's pair.
    assert_eq!(
        task.reviewers[1].model.as_deref(),
        Some("claude_code:claude-opus-5")
    );

    // And the CLI is spawned with no model of its own to run on.
    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    assert_eq!(session.agent_kind(), AgentKind::Codex);
    assert_eq!(session.model, None);
    let argv = h.spawn_argv(&session.id);
    assert!(argv.starts_with("codex "), "{argv}");
    assert!(!argv.contains("--model"), "{argv}");
}

/// A task created with agents runs each of them on its own: the engineer on
/// the pair it was given, the reviewer that was given one on that, and the
/// reviewer that was given none on its profile's.
#[tokio::test]
async fn a_task_created_with_agents_runs_each_one_on_its_own() {
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
                    "model": "codex:gpt-5.6-sol",
                    "reviewers": [
                        {"profile": chosen.name, "model": "opencode:ollama/llama3:8b"},
                        {"profile": untouched.name},
                    ],
                }),
            ),
            StatusCode::CREATED,
        )
        .await;

    assert_eq!(task.model.as_deref(), Some("codex:gpt-5.6-sol"));
    let pins: Vec<_> = task
        .reviewers
        .iter()
        .map(|r| (r.profile_id.as_str(), r.model.as_deref()))
        .collect();
    assert_eq!(
        pins,
        vec![
            (chosen.id.as_str(), Some("opencode:ollama/llama3:8b")),
            (untouched.id.as_str(), Some("claude_code:claude-opus-5")),
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

/// Editing a pending task moves its pins, and "default" hands them back to the
/// profiles — as those profiles stand at that moment, which is what
/// reassigning a reviewer has always done.
#[tokio::test]
async fn an_edit_moves_the_pins_and_default_hands_them_back() {
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
                    "model": "codex:gpt-5.3-codex",
                    "reviewers": [{"profile": chosen.name, "model": "codex:o3"}],
                }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(moved.model.as_deref(), Some("codex:gpt-5.3-codex"));
    assert_eq!(moved.reviewers[0].model.as_deref(), Some("codex:o3"));

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
    assert_eq!(renamed.model.as_deref(), Some("codex:gpt-5.3-codex"));

    // An agent named on its own drops the model the task had: that CLI on its
    // own default is a choice like any other.
    let dropped: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({"model": "opencode"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(dropped.model.as_deref(), Some("opencode"));

    // The profiles have moved since the task was cut, and handing the pins
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
    assert_eq!(back.model.as_deref(), Some("opencode:ollama/llama3:8b"));
    assert_eq!(
        back.reviewers[0].model.as_deref(),
        Some("claude_code:claude-haiku-4-5")
    );
}

/// A model is chosen by the CLI that runs it, so a string naming no agent CLI
/// is refused by name — with the form that would have worked — rather than
/// placed by guesswork, on a goal, on a task's engineer, on a reviewer slot
/// and on an edit alike.
#[tokio::test]
async fn a_model_naming_no_agent_is_refused_by_name() {
    let h = harness().await;
    let (planner, engineer, chosen, _) = on_claude(&h).await;
    let repo = h.repository(&h.dir.path().join("repo")).await;

    let goal_with = |pin: serde_json::Value| {
        let mut body = serde_json::json!({
            "title": "Ship it",
            "repository_ids": [repo.id],
            "planner_profile": planner.name,
        });
        let object = body.as_object_mut().expect("an object");
        for (key, value) in pin.as_object().expect("an object") {
            object.insert(key.clone(), value.clone());
        }
        post_json("/v1/goals", body)
    };

    let err = h
        .error(
            goal_with(serde_json::json!({"model": "claude-opus-5"})),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`claude-opus-5` names no agent CLI")
            && err.error.message.contains("`claude_code:claude-opus-5`"),
        "the refusal names the model and writes the form it wanted: {}",
        err.error.message
    );

    // An agent half that is no CLI is refused with the three that are.
    let err = h
        .error(
            goal_with(serde_json::json!({"model": "llama:x"})),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("unknown agent `llama`")
            && err.error.message.contains("claude_code, codex, opencode"),
        "{}",
        err.error.message
    );

    // Empty is a field somebody meant to fill in, not a way to say "the
    // profile's" — that is what leaving it out says.
    let err = h
        .error(
            goal_with(serde_json::json!({"model": ""})),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(err.error.message.contains("empty"), "{}", err.error.message);

    // And a colon with nothing after it is a model somebody meant to write.
    let err = h
        .error(
            goal_with(serde_json::json!({"model": "codex:"})),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("no model after the `:`"),
        "{}",
        err.error.message
    );

    let goal: GoalDto = h
        .json(
            goal_with(serde_json::json!({"model": "claude_code"})),
            StatusCode::CREATED,
        )
        .await;

    // The same refusal on the way in for a task, engineer and reviewer alike.
    let task_with =
        |body: serde_json::Value| post_json(&format!("/v1/goals/{}/tasks", goal.id), body);
    let err = h
        .error(
            task_with(serde_json::json!({
                "title": "Do the thing",
                "engineer_profile": engineer.name,
                "model": "gpt-5.3-codex",
                "reviewers": [{"profile": chosen.name}],
            })),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`gpt-5.3-codex` names no agent CLI"),
        "{}",
        err.error.message
    );
    let err = h
        .error(
            task_with(serde_json::json!({
                "title": "Do the thing",
                "engineer_profile": engineer.name,
                "reviewers": [{"profile": chosen.name, "model": "o3"}],
            })),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("`o3` names no agent CLI"),
        "{}",
        err.error.message
    );

    // And on an edit, where the field says one thing more: "default" hands
    // the pins back, and anything else has to name the CLI that runs it.
    let stored = h.store.get_goal(&goal.id).await.unwrap();
    let task = h
        .task_on(&stored, &repo, "Do it", &engineer, &[&chosen])
        .await;
    let err = h
        .error(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({"model": "gpt-5.3-codex"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`gpt-5.3-codex` names no agent CLI"),
        "{}",
        err.error.message
    );
}

/// Models are the agent CLI's business, not this daemon's: what a request
/// types is what gets pinned, whether or not any catalog here lists it —
/// opencode alone discovers its models at runtime, and none of them are known
/// until it is asked.
#[tokio::test]
async fn a_model_is_stored_as_typed_whatever_the_catalogs_list() {
    let h = harness().await;
    let (planner, ..) = on_claude(&h).await;
    let repo = h.repository(&h.dir.path().join("repo")).await;

    for model in [
        "opencode:ollama/llama3:8b",
        "claude_code:a-model-nobody-has-heard-of",
        "codex:gpt-5.3-codex",
    ] {
        let goal: GoalDto = h
            .json(
                post_json(
                    "/v1/goals",
                    serde_json::json!({
                        "title": "Ship it",
                        "repository_ids": [repo.id],
                        "planner_profile": planner.name,
                        "model": model,
                    }),
                ),
                StatusCode::CREATED,
            )
            .await;
        assert_eq!(goal.model.as_deref(), Some(model));
    }
}

/// A profile chooses what it runs on the way everything else does — one
/// string, the same refusals — and its two columns move together: clearing it
/// is what puts the profile back on auto, with no model left behind that no
/// CLI would run.
#[tokio::test]
async fn a_profile_is_pinned_and_cleared_by_the_one_field() {
    let h = harness().await;

    let created: ProfileDto = h
        .json(
            post_json(
                "/v1/profiles",
                serde_json::json!({
                    "name": "rust-engineer",
                    "role": "engineer",
                    "model": "codex:gpt-5.3-codex",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created.model.as_deref(), Some("codex:gpt-5.3-codex"));

    let err = h
        .error(
            post_json(
                "/v1/profiles",
                serde_json::json!({
                    "name": "no-cli",
                    "role": "engineer",
                    "model": "claude-opus-5",
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("`claude_code:claude-opus-5`"),
        "{}",
        err.error.message
    );

    let path = format!("/v1/profiles/{}", created.id);
    let moved: ProfileDto = h
        .json(
            put_json(&path, serde_json::json!({"model": "opencode"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        moved.model.as_deref(),
        Some("opencode"),
        "opencode on its own default model"
    );

    // Cleared, and both columns with it: auto has no model of its own.
    let cleared: ProfileDto = h
        .json(
            put_json(&path, serde_json::json!({"model": "default"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(cleared.model, None);
    let stored = h.store.get_profile(&created.id).await.unwrap();
    assert_eq!(stored.agent_kind(), None);
    assert_eq!(stored.model, None);

    // And an update that says nothing about it leaves it alone.
    let renamed: ProfileDto = h
        .json(
            put_json(&path, serde_json::json!({"name": "renamed"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(renamed.name, "renamed");
    assert_eq!(renamed.model, None);
}

// -- the effort it runs at --------------------------------------------------

/// An effort belongs to the model it runs at, so that is what it is checked
/// against: the model's own efforts where the catalog lists them, and
/// everything the agent CLI accepts where nothing does.
#[tokio::test]
async fn an_effort_is_checked_against_the_model_it_runs_at() {
    let h = harness().await;
    let profile_with = |name: &str, pin: serde_json::Value| {
        let mut body = serde_json::json!({"name": name, "role": "engineer"});
        let object = body.as_object_mut().expect("an object");
        for (key, value) in pin.as_object().expect("an object") {
            object.insert(key.clone(), value.clone());
        }
        post_json("/v1/profiles", body)
    };

    // The fast Codex model takes five efforts and `ultra` is not one of them.
    let err = h
        .error(
            profile_with(
                "too-deep",
                serde_json::json!({"model": "codex:gpt-5.6-luna", "effort": "ultra"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`ultra` is no effort of that model")
            && err.error.message.contains("low, medium, high, xhigh, max"),
        "the refusal names the effort and lists the ones there are: {}",
        err.error.message
    );

    let created: ProfileDto = h
        .json(
            profile_with(
                "deep-enough",
                serde_json::json!({"model": "codex:gpt-5.6-luna", "effort": "max"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(created.model.as_deref(), Some("codex:gpt-5.6-luna"));
    assert_eq!(created.effort.as_deref(), Some("max"));

    // Cleared, and the model it was run at left where it is.
    let path = format!("/v1/profiles/{}", created.id);
    let cleared: ProfileDto = h
        .json(
            put_json(
                &path,
                serde_json::json!({"model": "codex:gpt-5.6-luna", "effort": "default"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(cleared.model.as_deref(), Some("codex:gpt-5.6-luna"));
    assert_eq!(cleared.effort, None);
    let stored = h.store.get_profile(&created.id).await.unwrap();
    assert_eq!(stored.effort, None);

    // A model no catalog lists is held to everything its CLI accepts, which
    // is the most that can be said about an id somebody typed by hand.
    let new_model: ProfileDto = h
        .json(
            profile_with(
                "unlisted",
                serde_json::json!({"model": "claude_code:some-new-model", "effort": "xhigh"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(new_model.effort.as_deref(), Some("xhigh"));
    let err = h
        .error(
            profile_with(
                "unlisted-too-deep",
                serde_json::json!({"model": "claude_code:some-new-model", "effort": "ultra"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`ultra` is no effort of claude_code"),
        "{}",
        err.error.message
    );

    // And so is an agent CLI on its own default model: which model that is is
    // the CLI's business, so the effort is held to the CLI's own list.
    let cli_default: ProfileDto = h
        .json(
            profile_with(
                "cli-default",
                serde_json::json!({"model": "codex", "effort": "minimal"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(cli_default.model.as_deref(), Some("codex"));
    assert_eq!(cli_default.effort.as_deref(), Some("minimal"));
    let err = h
        .error(
            profile_with(
                "cli-default-nonsense",
                serde_json::json!({"model": "codex", "effort": "gigantic"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`gigantic` is no effort of codex"),
        "{}",
        err.error.message
    );

    // An effort with no model beside it has nothing to be run at, and saying
    // so beats guessing which model it was meant for.
    let err = h
        .error(
            profile_with("no-model", serde_json::json!({"effort": "high"})),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`high` is an effort with no model to run at"),
        "{}",
        err.error.message
    );
}

/// A task, its reviewer slots and its goal each carry an effort of their own,
/// checked the same way and pinned beside the model — and a model moved
/// without one runs at the CLI's own default, since the effort it was on need
/// not exist on the model it was moved to.
#[tokio::test]
async fn a_task_pins_the_effort_beside_the_model_and_moves_with_it() {
    let h = harness().await;
    let (planner, engineer, chosen, untouched) = on_claude(&h).await;
    let repo = h.repository(&h.git_repo("repo")).await;

    let goal: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "planner_profile": planner.name,
                    "model": "claude_code:claude-opus-5",
                    "effort": "max",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal.model.as_deref(), Some("claude_code:claude-opus-5"));
    assert_eq!(goal.effort.as_deref(), Some("max"));

    let tasks = format!("/v1/goals/{}/tasks", goal.id);
    let task: TaskDto = h
        .json(
            post_json(
                &tasks,
                serde_json::json!({
                    "title": "Do the thing",
                    "engineer_profile": engineer.name,
                    "model": "claude_code:claude-opus-5",
                    "effort": "xhigh",
                    "reviewers": [
                        {"profile": chosen.name, "model": "codex:gpt-5.6-luna", "effort": "max"},
                        {"profile": untouched.name},
                    ],
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(task.model.as_deref(), Some("claude_code:claude-opus-5"));
    assert_eq!(task.effort.as_deref(), Some("xhigh"));
    assert_eq!(task.reviewers[0].effort.as_deref(), Some("max"));
    assert_eq!(
        task.reviewers[1].effort, None,
        "the slot that chose nothing took its profile's, which is on none"
    );

    // A slot's effort is refused the way the engineer's is.
    let err = h
        .error(
            post_json(
                &tasks,
                serde_json::json!({
                    "title": "Do it deeper",
                    "engineer_profile": engineer.name,
                    "reviewers": [
                        {"profile": chosen.name, "model": "codex:gpt-5.6-luna", "effort": "ultra"},
                    ],
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`ultra` is no effort of that model"),
        "{}",
        err.error.message
    );

    // The session is opened on what the task was pinned to, effort and all.
    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    assert_eq!(session.model.as_deref(), Some("claude-opus-5"));
    assert_eq!(session.effort.as_deref(), Some("xhigh"));

    // An edit that names a model and no effort runs it at the CLI's own
    // default: the effort belonged to the model that was left behind.
    let moved: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({"model": "codex:gpt-5.6-luna"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(moved.model.as_deref(), Some("codex:gpt-5.6-luna"));
    assert_eq!(moved.effort, None);

    // And an edit naming both moves both.
    let deeper: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({"model": "codex:gpt-5.6-luna", "effort": "max"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(deeper.effort.as_deref(), Some("max"));

    // An effort of its own moves alone: what it is run at is the model the
    // task is already pinned to, which stays exactly where it is.
    let lowered: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({"effort": "low"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(lowered.model.as_deref(), Some("codex:gpt-5.6-luna"));
    assert_eq!(lowered.effort.as_deref(), Some("low"));

    // Checked against that same model, and cleared on its own.
    let err = h
        .error(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({"effort": "ultra"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`ultra` is no effort of that model"),
        "{}",
        err.error.message
    );
    let cleared: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({"effort": "default"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(cleared.model.as_deref(), Some("codex:gpt-5.6-luna"));
    assert_eq!(cleared.effort, None);
}

/// An effort written with no model beside it is about the effort alone: it is
/// checked against the model the row would run on anyway — the profile's on a
/// creation, its own on an edit — pinned to that model, and cleared without
/// disturbing it.
#[tokio::test]
async fn an_effort_of_its_own_is_run_at_the_model_already_pinned() {
    let h = harness().await;

    let created: ProfileDto = h
        .json(
            post_json(
                "/v1/profiles",
                serde_json::json!({
                    "name": "rust-engineer",
                    "role": "engineer",
                    "model": "codex:gpt-5.6-luna",
                    "effort": "max",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    let path = format!("/v1/profiles/{}", created.id);

    // The model it is run at stays where it is.
    let lowered: ProfileDto = h
        .json(
            put_json(&path, serde_json::json!({"effort": "low"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(lowered.model.as_deref(), Some("codex:gpt-5.6-luna"));
    assert_eq!(lowered.effort.as_deref(), Some("low"));

    // And it is that model the effort is checked against.
    let err = h
        .error(
            put_json(&path, serde_json::json!({"effort": "ultra"})),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`ultra` is no effort of that model")
            && err.error.message.contains("low, medium, high, xhigh, max"),
        "{}",
        err.error.message
    );

    // Cleared on its own, in either spelling, with the model left alone.
    for word in ["default", ""] {
        h.json::<ProfileDto>(
            put_json(&path, serde_json::json!({"effort": "high"})),
            StatusCode::OK,
        )
        .await;
        let cleared: ProfileDto = h
            .json(
                put_json(&path, serde_json::json!({"effort": word})),
                StatusCode::OK,
            )
            .await;
        assert_eq!(cleared.effort, None, "cleared by `{word}`");
        assert_eq!(cleared.model.as_deref(), Some("codex:gpt-5.6-luna"));
        let stored = h.store.get_profile(&created.id).await.unwrap();
        assert_eq!(stored.effort, None);
        assert_eq!(stored.model.as_deref(), Some("gpt-5.6-luna"));
    }

    // An edit about something else still leaves the effort alone.
    h.json::<ProfileDto>(
        put_json(&path, serde_json::json!({"effort": "high"})),
        StatusCode::OK,
    )
    .await;
    let renamed: ProfileDto = h
        .json(
            put_json(&path, serde_json::json!({"name": "renamed"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(renamed.name, "renamed");
    assert_eq!(renamed.effort.as_deref(), Some("high"));

    // Handing the model back to the profile takes the effort with it, so an
    // effort named beside that has no model here to be checked against.
    let err = h
        .error(
            put_json(
                &path,
                serde_json::json!({"model": "default", "effort": "high"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`high` is an effort beside a model handed back to its profile"),
        "{}",
        err.error.message
    );

    // A profile on auto is pinned to no agent CLI, so there is nothing an
    // effort of its own could be run at.
    let auto: ProfileDto = h
        .json(
            post_json(
                "/v1/profiles",
                serde_json::json!({"name": "auto", "role": "reviewer"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let err = h
        .error(
            put_json(
                &format!("/v1/profiles/{}", auto.id),
                serde_json::json!({"effort": "high"}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`high` is an effort with no model to run at"),
        "{}",
        err.error.message
    );
}

/// A goal, a task and a reviewer slot created with an effort and no model run
/// the profile's own model at that effort — the model is what the profile is
/// on, and only the effort was chosen.
#[tokio::test]
async fn an_effort_chosen_with_no_model_runs_the_profiles_own_at_it() {
    let h = harness().await;
    let (planner, engineer, chosen, untouched) = on_claude(&h).await;
    let repo = h.repository(&h.dir.path().join("repo")).await;

    let goal: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "planner_profile": planner.name,
                    "effort": "max",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal.model.as_deref(), Some("claude_code:claude-opus-5"));
    assert_eq!(goal.effort.as_deref(), Some("max"));

    let tasks = format!("/v1/goals/{}/tasks", goal.id);
    let task: TaskDto = h
        .json(
            post_json(
                &tasks,
                serde_json::json!({
                    "title": "Do the thing",
                    "engineer_profile": engineer.name,
                    "effort": "xhigh",
                    "reviewers": [
                        {"profile": chosen.name, "effort": "low"},
                        {"profile": untouched.name},
                    ],
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(task.model.as_deref(), Some("claude_code:claude-opus-5"));
    assert_eq!(task.effort.as_deref(), Some("xhigh"));
    assert_eq!(
        task.reviewers[0].model.as_deref(),
        Some("claude_code:claude-opus-5")
    );
    assert_eq!(task.reviewers[0].effort.as_deref(), Some("low"));
    assert_eq!(
        task.reviewers[1].effort, None,
        "the slot that chose nothing is on the profile's own, which has none"
    );

    // Checked against that profile's model, not against the CLI at large.
    let err = h
        .error(
            post_json(
                &tasks,
                serde_json::json!({
                    "title": "Deeper than it goes",
                    "engineer_profile": engineer.name,
                    "effort": "ultra",
                    "reviewers": [{"profile": chosen.name}],
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`ultra` is no effort of that model"),
        "{}",
        err.error.message
    );

    // And a profile on auto has no model to run one at.
    let auto = h
        .profile_on("auto-engineer", Role::Engineer, None, None)
        .await;
    let err = h
        .error(
            post_json(
                &tasks,
                serde_json::json!({
                    "title": "Nothing to run it at",
                    "engineer_profile": auto.name,
                    "effort": "high",
                    "reviewers": [{"profile": chosen.name}],
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`high` is an effort with no model to run at"),
        "{}",
        err.error.message
    );
}
