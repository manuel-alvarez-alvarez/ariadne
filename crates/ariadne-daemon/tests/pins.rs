//! What the API says a task's and a goal's agents run on.
//!
//! The pins live on `task_agents` and `goals`, and the launcher spawns from
//! them. There is nothing behind a pin to fall back to: what the orchestrator
//! sized an agent at, or what the user chose instead, is the whole of the
//! answer, and a model is required wherever an agent is pinned — no CLI
//! default stands in for one.
//!
//! One field carries the whole choice, `<agent_kind>:<model>`, on the way in
//! and on the way out. The effort rides in the field beside it, checked
//! against the model it is to run at before anything is written; `default`
//! stays legal for the effort alone.

mod common;

use ariadne_api::goals::GoalDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{AgentKind, Seat};

use axum::http::StatusCode;

use common::acp::{discovery_settled, registry_home, script, stub_acp_agent};
use common::{Harness, TIMEOUT, eventually, harness, patch_json, post_json, put_json};

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
    assert_eq!(goal.model, "codex:gpt-5.3-codex");

    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    assert_eq!(session.agent_kind(), AgentKind::Codex);
    assert_eq!(session.model, "gpt-5.3-codex");
    let argv = h.spawn_argv(&session.id);
    assert!(argv.starts_with("codex "), "{argv}");
    assert!(argv.contains("gpt-5.3-codex"), "{argv}");
}

/// A goal or a task with no model at all, an empty one, or the word `default`
/// is refused, and the refusal says a model is required.
#[tokio::test]
async fn a_request_with_no_model_is_refused_because_a_model_is_required() {
    let h = harness().await;
    let repo = h.repository(&h.dir.path().join("plain-repo")).await;

    // A goal with the field missing entirely never deserializes: the field is
    // required on the wire.
    let err = h
        .error(
            post_json(
                "/v1/goals",
                serde_json::json!({ "title": "Ship it", "repository_ids": [repo.id] }),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert!(
        err.error.message.contains("model"),
        "the refusal names the missing field: {}",
        err.error.message
    );

    // Empty, whitespace-only and `default` deserialize, and are refused by
    // the rule — a colon followed by whitespace alone is an empty model too.
    for model in ["", " ", "default", "codex: ", "codex:   "] {
        let err = h
            .error(
                post_json(
                    "/v1/goals",
                    serde_json::json!({
                        "title": "Ship it",
                        "repository_ids": [repo.id],
                        "model": model,
                    }),
                ),
                StatusCode::BAD_REQUEST,
            )
            .await;
        assert!(
            err.error.message.contains("a model is required"),
            "{model:?}: {}",
            err.error.message
        );
    }

    // An agent assignment is held to the same rule: the field is required on
    // the wire, and the words that used to clear it are refused by the rule.
    let goal = goal_on(
        &h,
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
    )
    .await;
    for (author, status) in [
        (
            serde_json::json!({ "seat": "author", "skills": ["coding"] }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            serde_json::json!({ "seat": "author", "skills": ["coding"], "model": "" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            serde_json::json!({ "seat": "author", "skills": ["coding"], "model": "default" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            serde_json::json!({ "seat": "author", "skills": ["coding"], "model": "codex: " }),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let err = h
            .error(
                post_json(
                    &format!("/v1/goals/{}/tasks", goal.id),
                    serde_json::json!({ "title": "A task", "agents": [author] }),
                ),
                status,
            )
            .await;
        assert!(err.error.message.contains("model"), "{}", err.error.message);
    }

    // And so is an edit: `default` used to clear the pin, and there is no
    // longer anything to clear it to.
    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "codex:gpt-5.3-codex" }),
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
    )
    .await;
    for model in ["", " ", "default", "codex: "] {
        let err = h
            .error(
                patch_json(
                    &format!("/v1/tasks/{}", task.id),
                    serde_json::json!({ "model": model }),
                ),
                StatusCode::BAD_REQUEST,
            )
            .await;
        assert!(
            err.error.message.contains("a model is required"),
            "{model:?}: {}",
            err.error.message
        );
    }
    let untouched: TaskDto = h.json(get_task(&task.id), StatusCode::OK).await;
    assert_eq!(
        agent(&untouched, Seat::Author).model,
        "codex:gpt-5.3-codex",
        "a refused edit moved nothing"
    );
}

fn get_task(id: &str) -> axum::http::Request<axum::body::Body> {
    common::get(&format!("/v1/tasks/{id}"))
}

/// A bare agent CLI parses nowhere: it names no model, and a model is
/// required.
#[tokio::test]
async fn a_bare_agent_cli_is_refused_wherever_a_model_is_written() {
    let h = harness().await;
    let repo = h.repository(&h.dir.path().join("repo")).await;

    let err = h
        .error(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": "codex",
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("`codex` names no model")
            && err.error.message.contains("a model is required"),
        "{}",
        err.error.message
    );
}

/// Every agent of a task answers for itself: the author's pin and the
/// reviewer's are each read off their own row.
#[tokio::test]
async fn a_task_staffs_each_agent_on_its_own_pin() {
    let h = harness().await;
    let goal = goal_on(
        &h,
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
    )
    .await;
    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "codex:gpt-5.3-codex", "effort": "high" }),
        serde_json::json!({ "model": "claude_code:claude-opus-5" }),
    )
    .await;

    let author = agent(&task, Seat::Author);
    assert_eq!(author.model, "codex:gpt-5.3-codex");
    assert_eq!(author.effort.as_deref(), Some("high"));

    let reviewer = agent(&task, Seat::Reviewer);
    assert_eq!(reviewer.model, "claude_code:claude-opus-5");
    assert_eq!(reviewer.effort, None, "no effort chosen is the CLI's own");
}

/// An edit moves the author's pin whole: the new model, and no effort left
/// behind from the model that was.
#[tokio::test]
async fn an_edit_moves_the_pin_whole() {
    let h = harness().await;
    let goal = goal_on(
        &h,
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
    )
    .await;
    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "codex:gpt-5.3-codex", "effort": "high" }),
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
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
    assert_eq!(author.model, "claude_code:claude-opus-5");
    assert_eq!(
        author.effort, None,
        "the effort belonged to the model that was left behind"
    );
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
    assert_eq!(goal.model, "opencode:ollama/llama3:8b");

    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    assert_eq!(session.model, "ollama/llama3:8b");
}

/// An opencode model is `provider/model` — that is the spelling opencode
/// itself takes back — so one with no provider prefix is refused when it is
/// pinned, wherever that is.
#[tokio::test]
async fn an_opencode_model_with_no_provider_prefix_is_refused() {
    let h = harness().await;
    let repo = h.repository(&h.dir.path().join("plain-repo")).await;

    let err = h
        .error(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": "opencode:llama3",
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("`llama3` names no provider")
            && err.error.message.contains("provider/model"),
        "{}",
        err.error.message
    );

    // The same rule on a staffed agent and on an edit.
    let goal = goal_on(
        &h,
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
    )
    .await;
    let err = h
        .error(
            post_json(
                &format!("/v1/goals/{}/tasks", goal.id),
                serde_json::json!({
                    "title": "A task",
                    "agents": [{ "seat": "author", "skills": ["coding"],
                                 "model": "opencode:llama3" }],
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("provider/model"),
        "{}",
        err.error.message
    );

    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
    )
    .await;
    let err = h
        .error(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "model": "opencode:llama3" }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("provider/model"),
        "{}",
        err.error.message
    );

    // With the prefix it is stored as typed.
    let pinned: GoalDto = h
        .json(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": "opencode:ollama/llama3",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(pinned.model, "opencode:ollama/llama3");
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
}

/// The effort rides beside the model and moves with it: an edit that names an
/// effort alone leaves the model where it is, and `default` — still legal for
/// the effort — clears it back to the CLI's own.
#[tokio::test]
async fn an_effort_of_its_own_is_run_at_the_model_already_pinned() {
    let h = harness().await;
    let goal = goal_on(
        &h,
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
    )
    .await;
    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "claude_code:claude-opus-5", "effort": "high" }),
        serde_json::json!({ "model": "claude_code:claude-sonnet-5" }),
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
        author.model, "claude_code:claude-opus-5",
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
    assert_eq!(author.model, "claude_code:claude-opus-5");
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
    assert_eq!(author.model, off);
    assert_eq!(author.effort.as_deref(), Some("high"));
}

/// A discovered catalog id — `<agent-id>:<model>`, the id `GET /v1/models`
/// serves — pins agents through the public API: the goal and the task take
/// it, spell it back whole, and the launch it produces runs the registry
/// agent's command with the bare model half pinned. An id naming no
/// registry agent stays refused.
#[tokio::test]
async fn a_discovered_catalog_id_pins_agents_through_the_api() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), script());
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    discovery_settled(&h, &stub).await;

    // The goal takes the id and spells it back whole.
    let goal = goal_on(
        &h,
        serde_json::json!({ "model": "stub:old-model", "effort": "low" }),
    )
    .await;
    assert_eq!(goal.model, "stub:old-model");
    assert_eq!(goal.effort.as_deref(), Some("low"));

    // So does a task's agent, on creation and on an edit.
    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "stub:old-model" }),
        serde_json::json!({ "model": "stub:old-model", "effort": "low" }),
    )
    .await;
    assert_eq!(agent(&task, Seat::Author).model, "stub:old-model");
    assert_eq!(agent(&task, Seat::Reviewer).model, "stub:old-model");
    let moved: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "model": "stub:old-model", "effort": "low" }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(agent(&moved, Seat::Author).model, "stub:old-model");

    // The pin reaches the registry command: the orchestrator spawned off it
    // runs the stub, with the bare model and effort halves pinned.
    let session = h.launcher.spawn_orchestrator(&goal.id).await.unwrap();
    assert_eq!(session.agent_kind(), AgentKind::Acp);
    assert_eq!(session.model, "stub:old-model");
    eventually(TIMEOUT, "the stub to be launched and pinned", || async {
        stub.calls_of("session/set_config_option").len() >= 2
    })
    .await;
    let pins = stub.calls_of("session/set_config_option");
    assert_eq!(pins[0]["value"], "old-model");
    assert_eq!(pins[1]["value"], "low");

    // An id the registry does not carry is refused as before.
    let err = h
        .error(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "model": "nobody:some-model" }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("names no agent CLI")
            || err.error.message.contains("unknown agent"),
        "{}",
        err.error.message
    );
}

/// A discovered model's effort choices bound its pin, the way a curated
/// model's do: an effort discovery never listed for it is refused by name,
/// and a model the catalog does not list stays free text, held to nothing.
#[tokio::test]
async fn a_discovered_models_effort_choices_bound_its_pin() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), script());
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    discovery_settled(&h, &stub).await;

    let repo = h.repository(&h.dir.path().join("plain-repo")).await;
    let err = h
        .error(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": "stub:old-model",
                    "effort": "wild",
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error
            .message
            .contains("`wild` is no effort of that model"),
        "{}",
        err.error.message
    );

    // A model discovery never listed is free text, as an opencode model is:
    // the agent is real, and the model and effort halves are handed on as
    // typed.
    let goal = goal_on(
        &h,
        serde_json::json!({ "model": "stub:unlisted-model", "effort": "anything" }),
    )
    .await;
    assert_eq!(goal.model, "stub:unlisted-model");
    assert_eq!(goal.effort.as_deref(), Some("anything"));
}

/// A discovered model turned off is refused under the same id the switch
/// stores: the catalog id whole, not an `acp:`-prefixed spelling nothing
/// serves.
#[tokio::test]
async fn a_discovered_model_turned_off_cannot_be_staffed_on() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), script());
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    discovery_settled(&h, &stub).await;

    let _: serde_json::Value = h
        .json(
            put_json(
                "/v1/models/enabled",
                serde_json::json!({"id": "stub:old-model", "enabled": false}),
            ),
            StatusCode::OK,
        )
        .await;

    let repo = h.repository(&h.dir.path().join("plain-repo")).await;
    let err = h
        .error(
            post_json(
                "/v1/goals",
                serde_json::json!({
                    "title": "Ship it",
                    "repository_ids": [repo.id],
                    "model": "stub:old-model",
                }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        err.error.message.contains("`stub:old-model` is turned off"),
        "the refusal names the catalog id: {}",
        err.error.message
    );
}

/// A fallback `acp` model that carries a colon of its own keeps its `acp:`
/// prefix in every response: its first segment names no registry agent, so
/// that prefix is the only spelling a re-submit parses — and it does, back
/// to the same pin.
#[tokio::test]
async fn an_acp_fallback_model_with_a_colon_keeps_its_prefix() {
    let h = harness().await;
    let goal = goal_on(&h, serde_json::json!({ "model": "acp:vendor:model" })).await;
    assert_eq!(goal.model, "acp:vendor:model");

    let task = task_on(
        &h,
        &goal,
        serde_json::json!({ "model": "acp:vendor:model" }),
        // The reviewer is staffed with the goal response's own spelling,
        // which is the round trip: what a response says is re-submittable.
        serde_json::json!({ "model": goal.model }),
    )
    .await;
    assert_eq!(agent(&task, Seat::Author).model, "acp:vendor:model");
    assert_eq!(agent(&task, Seat::Reviewer).model, "acp:vendor:model");

    let respelled = agent(&task, Seat::Author).model.clone();
    let moved: TaskDto = h
        .json(
            patch_json(
                &format!("/v1/tasks/{}", task.id),
                serde_json::json!({ "model": respelled }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(agent(&moved, Seat::Author).model, "acp:vendor:model");
}
