//! Integration tests for the documents a skill owns.
//!
//! The contract is that a skill carries one text — the one Ariadne ships until
//! somebody writes over it, resettable to that shipped text by dropping what
//! was written — and that a shipped skill is reset rather than deleted while
//! one of the user's own is deleted rather than reset. What an agent is
//! briefed with around a skill is Ariadne's own, and no route reads or writes
//! it.

mod common;

use axum::http::StatusCode;

use ariadne_api::skills::SkillDto;
use ariadne_core::Seat;
use ariadne_store::defaults::{BUILTIN_SKILLS, default_skill_document, default_system_prompt};

use common::{delete, get, harness, post, post_json, put_json};

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
