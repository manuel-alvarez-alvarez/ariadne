//! Integration tests for the refusal of unknown request fields.
//!
//! Every request DTO is `#[serde(deny_unknown_fields)]`, so a body carrying a
//! field the daemon does not declare is refused instead of being dropped in
//! silence — the silence that let a caller go on sending the removed
//! `agent_kind` of a profile and read the `200` as agreement.

mod common;

use axum::http::StatusCode;

use ariadne_api::profiles::ProfileDto;
use ariadne_core::Role;

use common::{harness, put_json};

/// `agent_kind` left the profile body when the flags moved to the agent. A
/// client that still writes it is told which field it is, and the profile is
/// left as it stands; the same body without it is stored.
#[tokio::test]
async fn an_unknown_field_is_refused_and_named() {
    let h = harness().await;
    let profile = h.profile("eng", Role::Engineer).await;
    let uri = format!("/v1/profiles/{}", profile.id);

    let refused = h
        .error(
            put_json(
                &uri,
                serde_json::json!({ "name": "engineer", "agent_kind": "claude_code" }),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert_eq!(refused.error.code, "invalid_request");
    assert!(
        refused.error.message.contains("unknown field `agent_kind`"),
        "the refusal has to name the field: {}",
        refused.error.message
    );
    let unchanged: ProfileDto = h.get(&uri).await;
    assert_eq!(unchanged.name, "eng");

    let renamed: ProfileDto = h
        .json(
            put_json(&uri, serde_json::json!({ "name": "engineer" })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(renamed.name, "engineer");
}
