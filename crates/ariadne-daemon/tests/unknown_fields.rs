//! Integration tests for the refusal of unknown request fields.
//!
//! Every request DTO is `#[serde(deny_unknown_fields)]`, so a body carrying a
//! field the daemon does not declare is refused instead of being dropped in
//! silence — the silence that let a caller go on sending a field that had been
//! removed and read the `200` as agreement.

mod common;

use axum::http::StatusCode;

use ariadne_api::skills::SkillDto;

use common::{harness, put_json};

/// A skill takes one field, its document. A client that writes anything else
/// is told which field it is, and the skill is left as it stands; the same
/// body without it is stored.
#[tokio::test]
async fn an_unknown_field_is_refused_and_named() {
    let h = harness().await;
    let uri = "/v1/skills/coding";

    let refused = h
        .error(
            put_json(
                uri,
                serde_json::json!({ "document": "---\nname: coding\n---\n", "seat": "author" }),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert_eq!(refused.error.code, "invalid_request");
    assert!(
        refused.error.message.contains("unknown field `seat`"),
        "the refusal has to name the field: {}",
        refused.error.message
    );
    let unchanged: SkillDto = h.get(uri).await;
    assert!(
        unchanged.document_is_default,
        "the refused write left the skill on the document Ariadne ships"
    );

    let written: SkillDto = h
        .json(
            put_json(
                uri,
                serde_json::json!({ "document": "---\nname: coding\ndescription: ours\n---\n" }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(written.summary, "ours");
}
