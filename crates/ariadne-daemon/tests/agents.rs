//! Integration tests for the agent-configuration endpoints.
//!
//! The contract is that every registry agent is listed, with no flags until
//! somebody sets some and its defaults readable beside the flags in force (so
//! a client resets by sending them back), and that an edit reaches the next
//! launch — spawn and resume alike — rather than only the sessions started
//! afterwards.

mod common;

use axum::http::StatusCode;

use ariadne_api::agents::AgentConfigDto;

use common::{STUB, TIMEOUT, eventually, get, harness, put_json};

/// Every agent the registry holds is listed, in registry order, and one
/// nobody configured is launched with nothing of ours.
#[tokio::test]
async fn every_registry_agent_is_listed_with_its_flags_and_its_defaults() {
    let h = harness().await;
    let configs: Vec<AgentConfigDto> = h.json(get("/v1/agents"), StatusCode::OK).await;
    assert_eq!(
        configs
            .iter()
            .map(|c| c.agent_id.as_str())
            .collect::<Vec<_>>(),
        ["claude-code-acp", "codex-acp", "opencode-acp", STUB]
    );
    for config in &configs {
        assert!(config.extra_flags.is_empty(), "{}", config.agent_id);
        assert!(config.default_flags.is_empty(), "{}", config.agent_id);
    }
}

/// The flags are replaced whole, an empty list included, and the defaults
/// keep being served beside them: that is what a "restore defaults" button
/// sends.
#[tokio::test]
async fn flags_are_replaced_whole_and_the_defaults_stay_readable() {
    let h = harness().await;
    let updated: AgentConfigDto = h
        .json(
            put_json(
                "/v1/agents/claude-code-acp",
                serde_json::json!({"extra_flags": ["--verbose"]}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(updated.agent_id, "claude-code-acp");
    assert_eq!(updated.extra_flags, ["--verbose"]);
    assert!(updated.default_flags.is_empty());

    let set: AgentConfigDto = h
        .json(
            put_json(
                "/v1/agents/codex-acp",
                serde_json::json!({"extra_flags": ["--quiet"]}),
            ),
            StatusCode::OK,
        )
        .await;
    // Restoring is the same call with the defaults the GET handed out.
    let restored: AgentConfigDto = h
        .json(
            put_json(
                "/v1/agents/codex-acp",
                serde_json::json!({"extra_flags": set.default_flags}),
            ),
            StatusCode::OK,
        )
        .await;
    assert!(restored.extra_flags.is_empty());

    let configs: Vec<AgentConfigDto> = h.json(get("/v1/agents"), StatusCode::OK).await;
    let claude = configs
        .iter()
        .find(|config| config.agent_id == "claude-code-acp")
        .unwrap();
    assert_eq!(
        claude.extra_flags,
        ["--verbose"],
        "the edit survived the round trip"
    );
}

/// An agent the registry does not hold has nothing to configure: the
/// refusal names it and says where the agents are.
#[tokio::test]
async fn an_unknown_agent_is_refused_by_name() {
    let h = harness().await;
    let err = h
        .error(
            put_json("/v1/agents/emacs", serde_json::json!({"extra_flags": []})),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(err.error.code, "invalid_request");
    assert!(err.error.message.contains("emacs"), "{}", err.error.message);
    assert!(
        err.error.message.contains("ACP registry"),
        "{}",
        err.error.message
    );
}

/// The point of the whole setting: what the config says is what the agent is
/// launched with, behind its registry command, on the spawn path and the
/// resume path alike.
#[tokio::test]
async fn a_launch_takes_its_flags_from_the_agent_config() {
    let h = harness().discover_agents().await;
    let _: AgentConfigDto = h
        .json(
            put_json(
                &format!("/v1/agents/{STUB}"),
                serde_json::json!({"extra_flags": ["--first"]}),
            ),
            StatusCode::OK,
        )
        .await;
    let (cast, _) = h.resumable_author().await;
    let task = cast.task.id;

    let session = h
        .launcher
        .resume_author(&task, "Round 1: please fix things.")
        .await
        .unwrap();
    eventually(TIMEOUT, "the first launch", || async {
        h.agent.launches_for(&session.id).len() == 1
    })
    .await;
    assert_eq!(h.agent.launches_for(&session.id)[0], ["--first"]);

    // Edited over REST, the next launch of the same session picks it up.
    let _: AgentConfigDto = h
        .json(
            put_json(
                &format!("/v1/agents/{STUB}"),
                serde_json::json!({"extra_flags": ["--second", "--third"]}),
            ),
            StatusCode::OK,
        )
        .await;
    let session = h
        .launcher
        .resume_author(&task, "Round 2: please fix things.")
        .await
        .unwrap();
    eventually(TIMEOUT, "the second launch", || async {
        h.agent.launches_for(&session.id).len() == 2
    })
    .await;
    assert_eq!(
        h.agent.launches_for(&session.id)[1],
        ["--second", "--third"],
        "the edited flags, and not the dropped one"
    );
}
