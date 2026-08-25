//! Integration tests for the agent-configuration endpoints.
//!
//! The contract is that every agent kind is configured out of the box, that
//! its defaults stay readable beside the flags in force (so a client resets by
//! sending them back), and that an edit reaches the next launch — spawn and
//! resume alike — rather than only the sessions started afterwards.

mod common;

use axum::http::StatusCode;

use ariadne_api::agents::AgentConfigDto;
use ariadne_core::AgentKind;

use common::{get, harness, put_json};

#[tokio::test]
async fn every_agent_kind_is_listed_with_its_flags_and_its_defaults() {
    let h = harness().await;
    let configs: Vec<AgentConfigDto> = h.json(get("/v1/agents"), StatusCode::OK).await;
    assert_eq!(
        configs.iter().map(|c| c.agent_kind).collect::<Vec<_>>(),
        AgentKind::ALL.to_vec()
    );
    for config in &configs {
        assert_eq!(
            config.default_flags,
            config.agent_kind.default_flags(),
            "{:?}",
            config.agent_kind
        );
        // Nothing has been edited yet, so the two halves agree.
        assert_eq!(config.extra_flags, config.default_flags);
    }
}

/// The flags are replaced whole, an empty list included, and the defaults keep
/// being served beside them: that is what a "restore defaults" button sends.
#[tokio::test]
async fn flags_are_replaced_whole_and_the_defaults_stay_readable() {
    let h = harness().await;
    let updated: AgentConfigDto = h
        .json(
            put_json(
                "/v1/agents/claude_code",
                serde_json::json!({"extra_flags": ["--permission-mode=acceptEdits"]}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(updated.extra_flags, ["--permission-mode=acceptEdits"]);
    assert_eq!(updated.default_flags, ["--dangerously-skip-permissions"]);

    let emptied: AgentConfigDto = h
        .json(
            put_json("/v1/agents/codex", serde_json::json!({"extra_flags": []})),
            StatusCode::OK,
        )
        .await;
    assert!(emptied.extra_flags.is_empty());

    // Restoring is the same call with the defaults the GET handed out.
    let restored: AgentConfigDto = h
        .json(
            put_json(
                "/v1/agents/codex",
                serde_json::json!({"extra_flags": emptied.default_flags}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        restored.extra_flags,
        ["--dangerously-bypass-approvals-and-sandbox"]
    );

    let configs: Vec<AgentConfigDto> = h.json(get("/v1/agents"), StatusCode::OK).await;
    assert_eq!(
        configs[0].extra_flags,
        ["--permission-mode=acceptEdits"],
        "the edit survived the round trip"
    );
}

#[tokio::test]
async fn an_unknown_agent_kind_is_refused_by_name() {
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
        err.error.message.contains("claude_code"),
        "{}",
        err.error.message
    );
}

/// The point of the whole move: what the config says is what the agent is
/// launched with, on the spawn path and the resume path alike.
#[tokio::test]
async fn a_launch_takes_its_flags_from_the_agent_config() {
    let h = harness().await;
    let (cast, _) = h.resumable_engineer().await;
    let task = cast.task.id;

    let session = h
        .launcher
        .resume_engineer(&task, "Round 1: please fix things.")
        .await
        .unwrap();
    let argv = h.spawn_argv(&session.id);
    assert_eq!(
        argv.matches("--dangerously-skip-permissions").count(),
        1,
        "the seeded bypass, exactly once: {argv}"
    );

    // Edited over REST, the next launch of the same session picks it up.
    let _: AgentConfigDto = h
        .json(
            put_json(
                "/v1/agents/claude_code",
                serde_json::json!({"extra_flags": ["--permission-mode=acceptEdits"]}),
            ),
            StatusCode::OK,
        )
        .await;
    let session = h
        .launcher
        .resume_engineer(&task, "Round 2: please fix things.")
        .await
        .unwrap();
    let argv = h.spawn_argv(&session.id);
    assert!(
        argv.contains("--permission-mode=acceptEdits"),
        "the edited flags: {argv}"
    );
    assert!(
        !argv.contains("--dangerously-skip-permissions"),
        "the flag the user dropped is gone: {argv}"
    );
}
