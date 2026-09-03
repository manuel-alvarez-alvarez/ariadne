//! Integration tests for the one prompt a profile owns.
//!
//! The contract is that a profile carries a system prompt — the default of
//! its role until somebody writes one, resettable to that default by dropping
//! what was written — and carries nothing of the lifecycle around it: the
//! briefings that start, resume and nudge a session are Ariadne's own, and no
//! route reads or writes them.

mod common;

use axum::http::StatusCode;

use ariadne_api::profiles::ProfileDto;
use ariadne_core::Role;
use ariadne_store::defaults::{BUILTIN_PROFILES, default_system_prompt};

use common::{get, harness, post, post_json, put_json};

/// A profile is created on the system prompt of its role and stores none of
/// it; a text given at creation is the profile's own until the reset takes it
/// back off.
#[tokio::test]
async fn a_created_profile_starts_on_the_default_of_its_role() {
    let h = harness().await;

    let plain: ProfileDto = h
        .json(
            post_json(
                "/v1/profiles",
                serde_json::json!({ "name": "rev", "role": "reviewer" }),
            ),
            StatusCode::CREATED,
        )
        .await;
    // No system prompt was given, so the role's own is what it runs on.
    assert_eq!(plain.system_prompt, default_system_prompt(Role::Reviewer));
    assert!(plain.system_prompt_is_default);

    let own: ProfileDto = h
        .json(
            post_json(
                "/v1/profiles",
                serde_json::json!({
                    "name": "eng",
                    "role": "engineer",
                    "system_prompt": "You are eng.",
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(own.system_prompt, "You are eng.");
    assert!(!own.system_prompt_is_default);

    let reset: ProfileDto = h
        .json(
            post(&format!("/v1/profiles/{}/system-prompt/reset", own.id)),
            StatusCode::OK,
        )
        .await;
    assert_eq!(reset.id, own.id);
    assert_eq!(reset.system_prompt, default_system_prompt(Role::Engineer));
    assert!(reset.system_prompt_is_default);
}

/// The system prompt is rewritten by the update route, and reachable by
/// unique name, as everything under `/v1/profiles` is.
#[tokio::test]
async fn a_system_prompt_is_rewritten_and_reset_by_name() {
    let h = harness().await;
    h.profile("eng", Role::Engineer).await;

    let updated: ProfileDto = h
        .json(
            put_json(
                "/v1/profiles/eng",
                serde_json::json!({ "system_prompt": "You are eng." }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(updated.system_prompt, "You are eng.");
    assert!(!updated.system_prompt_is_default);

    let read: ProfileDto = h.get("/v1/profiles/eng").await;
    assert_eq!(read.system_prompt, "You are eng.");

    let reset: ProfileDto = h
        .json(
            post("/v1/profiles/eng/system-prompt/reset"),
            StatusCode::OK,
        )
        .await;
    assert_eq!(reset.system_prompt, default_system_prompt(Role::Engineer));
    assert!(reset.system_prompt_is_default);
}

/// The lifecycle briefings are Ariadne's own: no route reads one and no route
/// writes one, whichever kind is asked for.
#[tokio::test]
async fn no_route_reads_or_writes_a_lifecycle_prompt() {
    let h = harness().await;
    let engineer = h.profile("eng", Role::Engineer).await;
    let id = &engineer.id;

    for request in [
        get(&format!("/v1/profiles/{id}/prompts")),
        put_json(
            &format!("/v1/profiles/{id}/prompts/engineer_briefing"),
            serde_json::json!({ "content": "Do {task_title}." }),
        ),
        post(&format!(
            "/v1/profiles/{id}/prompts/engineer_briefing/reset"
        )),
    ] {
        let (status, _) = h.send(request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn an_unknown_profile_is_a_404_on_the_system_prompt_reset() {
    let h = harness().await;

    let err = h
        .error(
            post("/v1/profiles/nosuchprofile/system-prompt/reset"),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert_eq!(err.error.code, "not_found");
}

/// The planner writes a spec the user approves and sizes each slot it
/// assigns, and both rules reach a planner nobody has edited: the text is a
/// constant, never a row, so the seeded Planner and every profile still on the
/// default answer with the text the code ships today rather than the one that
/// was current when they were created.
#[tokio::test]
async fn a_planner_on_the_default_prompt_is_briefed_to_write_a_spec_and_size_its_slots() {
    let h = harness().await;
    let seeded = BUILTIN_PROFILES
        .iter()
        .find(|b| b.role == Role::Planner)
        .expect("a Planner is seeded");

    let fresh: ProfileDto = h
        .json(
            post_json(
                "/v1/profiles",
                serde_json::json!({ "name": "plan", "role": "planner" }),
            ),
            StatusCode::CREATED,
        )
        .await;

    for id in [seeded.id, &fresh.id] {
        let profile: ProfileDto = h.get(&format!("/v1/profiles/{id}")).await;
        assert!(profile.system_prompt_is_default, "{id} was edited");
        assert_eq!(profile.system_prompt, default_system_prompt(Role::Planner));
        for guidance in [
            "Draft a spec: scope, behavior, acceptance criteria.",
            "Ask the user about each unclear point.",
            "Ask again until the user writes an explicit yes.",
            "Call `create_task` for task 1",
            "`list_models`",
            "Size each slot",
            "a top effort only where the task earns it",
            "Keep a reviewer under its engineer",
            "`best_for`",
            "`cost`",
            "tier: unknown",
        ] {
            assert!(
                profile.system_prompt.contains(guidance),
                "the {id} prompt does not say {guidance}: {}",
                profile.system_prompt
            );
        }
    }

    // And none of it was copied anywhere: the text a profile answers with is
    // the code's until somebody sets one of its own.
    let stored: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM profiles WHERE system_prompt IS NOT NULL")
            .fetch_one(h.db())
            .await
            .unwrap();
    assert_eq!(stored, 0);
}
