//! What the API says a task's and a goal's agents run on.
//!
//! The pins live on `task_agents` and `goals`, and the launcher spawns from
//! them. There is nothing behind a pin to fall back to: what the orchestrator
//! sized an agent at, or what the user chose instead, is the whole of the
//! answer, and `default` puts it back on auto rather than on somebody else's
//! choice.
//!
//! One field carries the whole choice, `<agent_kind>[:<model>]`, on the way in
//! and on the way out: an agent CLI on its own is that CLI on its own default
//! model, and null is auto. The effort rides in the field beside it, checked
//! against the model it is to run at before anything is written.

mod common;

use ariadne_api::goals::GoalDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{AgentKind, Seat};

use axum::http::StatusCode;

use common::{Harness, harness, patch_json, post_json, put_json};

/// A goal on `pin`, in a repository of its own.
async fn goal_on(h: &Harness, pin: serde_json::Value) -> GoalDto {
    let repo = h.repository(&h.git_repo("repo")).await;
    let mut body = serde_json::json!({ "title": "Ship it", "repository_ids": [repo.id] });
    merge(&mut body, pin);
    h.json(post_json("/v1/goals", body), StatusCode::CREATED)
        .await
}

/// A task on `goal`, its author and its one reviewer staffed with the pins
/// given: two agents that answer for themselves, so no assertion below can
/// pass by reading the other one's.
async fn task_on(
    h: &Harness,
    goal: &GoalDto,
    author: serde_json::Value,
    reviewer: serde_json::Value,
) -> TaskDto {
    let mut a = serde_json::json!({ "seat": "author", "skills": ["coding"] });
    merge(&mut a, author);
    let mut r = serde_json::json!({ "seat": "reviewer", "skills": ["code-review"] });
    merge(&mut r, reviewer);
    h.json(
        post_json(
            &format!("/v1/goals/{}/tasks", goal.id),
            serde_json::json!({ "title": "Do the thing", "agents": [a, r] }),
        ),
        StatusCode::CREATED,
    )
    .await
}

/// The fields of `extra` written over `body`.
fn merge(body: &mut serde_json::Value, extra: serde_json::Value) {
    let object = body.as_object_mut().expect("an object");
    for (key, value) in extra.as_object().expect("an object") {
        object.insert(key.clone(), value.clone());
    }
}

/// The agent in `seat`, which is what a pin is read off.
fn agent(task: &TaskDto, seat: Seat) -> &ariadne_api::tasks::TaskAgentDto {
    task.agents
        .iter()
        .find(|a| a.seat == seat)
        .unwrap_or_else(|| panic!("the task staffs no {}", seat.as_str()))
}

/// A goal created on an agent CLI and a model plans on both, and the session
/// its orchestrator is spawned into is launched with them.
#[tokio::test]
async fn a_goal_created_with_an_agent_and_a_model_plans_on_them() {
    let h = harness().await;
    let goal = goal_on(&h, serde_json::json!({ "model": "codex:gpt-5.3-codex" })).await;
    assert_eq!(goal.model.as_deref(), Some("codex:gpt-5.3-codex"));

    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    assert_eq!(session.agent_kind(), AgentKind::Codex);
    assert_eq!(session.model.as_deref(), Some("gpt-5.3-codex"));
    let argv = h.spawn_argv(&session.id);
    assert!(argv.starts_with("codex "), "{argv}");
    assert!(argv.contains("gpt-5.3-codex"), "{argv}");
}

/// The agent is the choice and the model only narrows it: an agent named on
/// its own pins that CLI with no model, which is what runs it on its own
/// default — on a goal and on either agent of a task alike.
#[tokio::test]
async fn an_agent_alone_pins_it_with_no_model_of_its_own() {
    let h = harness().await;
    let goal = goal_on(&h, serde_json::json!({ "model": "claude_code" })).await;
    assert_eq!(goal.model.as_deref(), Some("claude_code"));

    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "codex" }),
        serde_json::json!({ "model": "opencode" }),
    )
    .await;
    assert_eq!(agent(&task, Seat::Author).model.as_deref(), Some("codex"));
    assert_eq!(
        agent(&task, Seat::Reviewer).model.as_deref(),
        Some("opencode")
    );

    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    assert_eq!(session.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(
        session.model, None,
        "an agent with no model runs on that CLI's own default"
    );
}

/// Every agent of a task answers for itself, and an agent left out of the
/// request is on auto: null, which the launcher resolves at spawn time.
#[tokio::test]
async fn a_task_staffs_each_agent_on_its_own_pin() {
    let h = harness().await;
    let goal = goal_on(&h, serde_json::json!({ "model": "claude_code" })).await;
    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "codex:gpt-5.3-codex", "effort": "high" }),
        serde_json::json!({}),
    )
    .await;

    let author = agent(&task, Seat::Author);
    assert_eq!(author.model.as_deref(), Some("codex:gpt-5.3-codex"));
    assert_eq!(author.effort.as_deref(), Some("high"));

    let reviewer = agent(&task, Seat::Reviewer);
    assert_eq!(reviewer.model, None, "nothing chosen is auto");
    assert_eq!(reviewer.effort, None);
}

/// An edit moves the author's pin, and `default` hands it back to auto —
/// there is nothing else for it to go back to.
#[tokio::test]
async fn an_edit_moves_the_pin_and_default_hands_it_back_to_auto() {
    let h = harness().await;
    let goal = goal_on(&h, serde_json::json!({ "model": "claude_code" })).await;
    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "codex:gpt-5.3-codex", "effort": "high" }),
        serde_json::json!({}),
    )
    .await;

    let moved: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "model": "claude_code:claude-opus-5" }),
            ),
            StatusCode::OK,
        )
        .await;
    let author = agent(&moved, Seat::Author);
    assert_eq!(author.model.as_deref(), Some("claude_code:claude-opus-5"));
    assert_eq!(
        author.effort, None,
        "the effort belonged to the model that was left behind"
    );

    let cleared: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "model": "default" }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(agent(&cleared, Seat::Author).model, None);
}

/// A model is one field, and it names the agent CLI that runs it: a string
/// that names none is refused, and the refusal writes the form it wanted.
#[tokio::test]
async fn a_model_naming_no_agent_is_refused_by_name() {
    let h = harness().await;
    let repo = h.repository(&h.dir.path().join("repo")).await;
    let goal_with = |pin: serde_json::Value| {
        let mut body = serde_json::json!({ "title": "Ship it", "repository_ids": [repo.id] });
        merge(&mut body, pin);
        post_json("/v1/goals", body)
    };

    let err = h
        .error(
            goal_with(serde_json::json!({ "model": "claude-opus-5" })),
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
            goal_with(serde_json::json!({ "model": "llama:x" })),
            StatusCode::BAD_REQUEST,
        )
        .await;
    for kind in AgentKind::ALL {
        assert!(
            err.error.message.contains(kind.as_str()),
            "the refusal lists {}: {}",
            kind.as_str(),
            err.error.message
        );
    }
}

/// The model half is free text handed to the agent CLI as typed: an id with
/// colons and slashes of its own reaches the row whole, catalog or no catalog.
#[tokio::test]
async fn a_model_is_stored_as_typed_whatever_the_catalogs_list() {
    let h = harness().await;
    let goal = goal_on(
        &h,
        serde_json::json!({ "model": "opencode:ollama/llama3:8b" }),
    )
    .await;
    assert_eq!(goal.model.as_deref(), Some("opencode:ollama/llama3:8b"));

    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    assert_eq!(session.model.as_deref(), Some("ollama/llama3:8b"));
}

/// An effort belongs to a model, so it is checked against the one it will run
/// at, and one that model does not take is refused before anything is written.
#[tokio::test]
async fn an_effort_is_checked_against_the_model_it_runs_at() {
    let h = harness().await;
    let repo = h.repository(&h.dir.path().join("repo")).await;

    let err = h
        .error(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": "claude_code:claude-opus-5",
                    "effort": "nonsense",
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("nonsense"),
        "the refusal names the effort: {}",
        err.error.message
    );

    // An effort with no model beside it has nothing to be run at.
    let err = h
        .error(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "effort": "high",
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        !err.error.message.is_empty(),
        "an effort with no model is refused with a reason"
    );
}

/// The effort rides beside the model and moves with it: an edit that names an
/// effort alone leaves the model where it is.
#[tokio::test]
async fn an_effort_of_its_own_is_run_at_the_model_already_pinned() {
    let h = harness().await;
    let goal = goal_on(&h, serde_json::json!({ "model": "claude_code" })).await;
    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "claude_code:claude-opus-5", "effort": "high" }),
        serde_json::json!({}),
    )
    .await;

    let deeper: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "effort": "xhigh" }),
            ),
            StatusCode::OK,
        )
        .await;
    let author = agent(&deeper, Seat::Author);
    assert_eq!(
        author.model.as_deref(),
        Some("claude_code:claude-opus-5"),
        "the model stayed where it was"
    );
    assert_eq!(author.effort.as_deref(), Some("xhigh"));

    let plain: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "effort": "default" }),
            ),
            StatusCode::OK,
        )
        .await;
    let author = agent(&plain, Seat::Author);
    assert_eq!(author.model.as_deref(), Some("claude_code:claude-opus-5"));
    assert_eq!(author.effort, None);
}

/// A model the user turned off is refused wherever an agent is staffed on it,
/// and by name: the goal it would run in, the author of a task, a reviewer of
/// one, and an edit that moves an agent onto it.
///
/// Work already staffed on it is not disturbed — a pin is the snapshot a row
/// was created with — so the check is on the model a request *names*, and an
/// effort moved on its own goes through untouched.
#[tokio::test]
async fn a_model_that_is_turned_off_cannot_be_staffed_on() {
    let h = harness().await;
    let off = "claude_code:claude-opus-5";
    let on = "claude_code:claude-sonnet-5";

    // Staffed before it goes off, which is the row that has to keep working.
    let goal = goal_on(&h, serde_json::json!({ "model": on })).await;
    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": off }),
        serde_json::json!({ "model": on }),
    )
    .await;

    let _: serde_json::Value = h
        .json(
            put_json(
                "/v1/models/enabled",
                serde_json::json!({"id": off, "enabled": false}),
            ),
            StatusCode::OK,
        )
        .await;

    let refused = |message: String| {
        assert!(
            message.contains(off) && message.contains("turned off"),
            "the refusal names the model and why: {message}"
        );
    };

    // A goal.
    let repo = h.repository(&h.git_repo("second")).await;
    refused(
        h.error(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": off,
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await
        .error
        .message,
    );

    // An author, and a reviewer, on a task being created.
    for agents in [
        serde_json::json!([{ "seat": "author", "skills": ["coding"], "model": off }]),
        serde_json::json!([
            { "seat": "author", "skills": ["coding"], "model": on },
            { "seat": "reviewer", "skills": ["code-review"], "model": off },
        ]),
    ] {
        refused(
            h.error(
                post_json(
                    &format!("/v1/goals/{}/tasks", goal.id),
                    serde_json::json!({ "title": "A task", "agents": agents }),
                ),
                StatusCode::BAD_REQUEST,
            )
            .await
            .error
            .message,
        );
    }

    // And an edit that would move an agent onto it.
    refused(
        h.error(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "model": off }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await
        .error
        .message,
    );

    // The task staffed on it before it went off is untouched, and its effort
    // still moves: what a row runs on is what it was created with.
    let moved: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "effort": "high" }),
            ),
            StatusCode::OK,
        )
        .await;
    let author = moved
        .agents
        .iter()
        .find(|a| a.seat == Seat::Author)
        .expect("the task keeps its author");
    assert_eq!(author.model.as_deref(), Some(off));
    assert_eq!(author.effort.as_deref(), Some("high"));
}
