//! Integration tests for the per-profile prompt endpoints.
//!
//! The contract is that every prompt a profile owns is listable — the ones it
//! was given and the defaults it was not, each saying which it is — editable
//! with any text whose `{placeholder}`s its kind can fill in, and resettable to
//! the default by dropping what was set; and that a kind belonging to another
//! role, or a placeholder nothing would substitute, is refused with a sentence,
//! not a 500.

mod common;

use axum::http::StatusCode;

use ariadne_api::profiles::{ProfileDto, ProfilePromptDto};
use ariadne_core::{PromptKind, Role};
use ariadne_store::defaults::{BUILTIN_PROFILES, default_prompt, default_system_prompt};

use common::{Harness, get, harness, post, post_json, put_json};

/// How many prompts the database is actually holding: what the endpoints
/// answer is the effective prompt, which says nothing about whether a row is
/// behind it.
async fn prompt_rows(h: &Harness) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM profile_prompts")
        .fetch_one(h.db())
        .await
        .unwrap()
}

/// A profile is created on the defaults of its role and stores none of them:
/// what it is briefed with is the code's until somebody sets a prompt. And it
/// is reachable by unique name, as everything under `/v1/profiles` is.
#[tokio::test]
async fn a_profile_lists_the_prompts_of_its_role_in_briefing_order() {
    let h = harness().await;
    let engineer = h.profile("eng", Role::Engineer).await;

    let prompts: Vec<ProfilePromptDto> =
        h.get(&format!("/v1/profiles/{}/prompts", engineer.id)).await;

    assert_eq!(
        prompts.iter().map(|p| p.kind).collect::<Vec<_>>(),
        PromptKind::for_role(Role::Engineer).to_vec()
    );
    // Nothing was set on it, so every prompt is the default of its kind and
    // there is no date on any of them.
    for prompt in &prompts {
        assert_eq!(
            prompt.content,
            default_prompt(Role::Engineer, prompt.kind).unwrap()
        );
        assert!(prompt.is_default, "{} was stored", prompt.kind.as_str());
        assert!(prompt.updated_at.is_none());
    }
    assert_eq!(prompt_rows(&h).await, 0);

    let by_name: Vec<ProfilePromptDto> = h.get("/v1/profiles/eng/prompts").await;
    assert_eq!(by_name.len(), prompts.len());
}

/// A planner lists the prompts a planner owns, and none of the engineer's.
#[tokio::test]
async fn a_planner_lists_only_its_own_prompts() {
    let h = harness().await;
    let planner = h.profile("plan", Role::Planner).await;

    let prompts: Vec<ProfilePromptDto> =
        h.get(&format!("/v1/profiles/{}/prompts", planner.id)).await;

    assert_eq!(
        prompts.iter().map(|p| p.kind).collect::<Vec<_>>(),
        PromptKind::for_role(Role::Planner).to_vec()
    );
    assert!(
        !prompts.iter().any(|p| p.kind == PromptKind::EngineerResume),
        "a planner was given the engineer's resume"
    );
}

/// Dropping placeholders, keeping literal braces, saying almost nothing: what
/// a briefing says is the editor's business, not the API's.
///
/// And the rows follow the edit: setting one writes exactly one, resetting it
/// takes that one away again.
#[tokio::test]
async fn a_prompt_update_takes_any_content_and_a_reset_deletes_it() {
    let h = harness().await;
    let reviewer = h.profile("rev", Role::Reviewer).await;
    let uri = format!("/v1/profiles/{}/prompts/reviewer_briefing", reviewer.id);
    let content = "Review {task_title} and answer {\"verdict\": \"approve\"}.";

    let updated: ProfilePromptDto = h
        .json(
            put_json(&uri, serde_json::json!({ "content": content })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(updated.kind, PromptKind::ReviewerBriefing);
    assert_eq!(updated.content, content);
    assert!(!updated.is_default);
    assert!(updated.updated_at.is_some());
    assert_eq!(prompt_rows(&h).await, 1);

    // The edit is what a later read sees, and the only prompt of the profile
    // that is not its default.
    let prompts: Vec<ProfilePromptDto> =
        h.get(&format!("/v1/profiles/{}/prompts", reviewer.id)).await;
    assert_eq!(prompts[0].content, content);
    assert!(!prompts[0].is_default);
    assert!(prompts[1..].iter().all(|p| p.is_default));

    let reset: ProfilePromptDto = h.json(post(&format!("{uri}/reset")), StatusCode::OK).await;
    assert_eq!(
        reset.content,
        default_prompt(Role::Reviewer, PromptKind::ReviewerBriefing).unwrap()
    );
    assert!(reset.is_default);
    assert!(reset.updated_at.is_none());
    // The row is gone with it: a default is stored nowhere.
    assert_eq!(prompt_rows(&h).await, 0);
}

/// A `{token}` the kind has no value for is a 400 naming it and the ones it
/// could have used — the typo is caught here rather than shipped to an agent
/// as literal text.
#[tokio::test]
async fn a_placeholder_the_kind_cannot_fill_in_is_a_400_naming_it() {
    let h = harness().await;
    let engineer = h.profile("eng", Role::Engineer).await;
    let uri = format!("/v1/profiles/{}/prompts/engineer_briefing", engineer.id);

    let err = h
        .error(
            put_json(
                &uri,
                serde_json::json!({ "content": "# {task_titel}\n\n{task_description}" }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;

    assert_eq!(err.error.code, "invalid_request");
    assert!(
        err.error.message.contains("{task_titel}")
            && err.error.message.contains("{task_title}")
            && err.error.message.contains("{dependencies}"),
        "unhelpful message: {}",
        err.error.message
    );

    // The stored prompt is untouched.
    let prompts: Vec<ProfilePromptDto> =
        h.get(&format!("/v1/profiles/{}/prompts", engineer.id)).await;
    assert_eq!(
        prompts[0].content,
        default_prompt(Role::Engineer, PromptKind::EngineerBriefing).unwrap()
    );
}

/// Editing a prompt kind of another role is a 400 naming the roles involved,
/// on both the update and the reset route.
#[tokio::test]
async fn a_kind_of_another_role_is_refused_with_a_sentence() {
    let h = harness().await;
    let planner = h.profile("plan", Role::Planner).await;
    let uri = format!("/v1/profiles/{}/prompts/changes_requested", planner.id);

    for request in [
        put_json(&uri, serde_json::json!({ "content": "..." })),
        post(&format!("{uri}/reset")),
    ] {
        let err = h.error(request, StatusCode::BAD_REQUEST).await;
        assert_eq!(err.error.code, "invalid_request");
        assert!(
            err.error.message.contains("changes_requested")
                && err.error.message.contains("engineer"),
            "unhelpful message: {}",
            err.error.message
        );
    }
}

/// A kind nobody owns any more is one nobody can write: the landing briefing
/// is the repository's, and the prompt routes answer for it the way they
/// answer for a name that never existed.
#[tokio::test]
async fn a_landing_kind_is_no_longer_a_prompt_of_the_engineer() {
    let h = harness().await;
    let engineer = h.profile("eng", Role::Engineer).await;

    let prompts: Vec<ProfilePromptDto> =
        h.get(&format!("/v1/profiles/{}/prompts", engineer.id)).await;
    assert_eq!(
        prompts.iter().map(|p| p.kind).collect::<Vec<_>>(),
        vec![
            PromptKind::EngineerBriefing,
            PromptKind::EngineerResume,
            PromptKind::ChangesRequested,
        ]
    );

    for kind in ["landing_direct", "landing_pull_request"] {
        let err = h
            .error(
                put_json(
                    &format!("/v1/profiles/{}/prompts/{kind}", engineer.id),
                    serde_json::json!({ "content": "..." }),
                ),
                StatusCode::BAD_REQUEST,
            )
            .await;
        assert_eq!(err.error.code, "invalid_request");
        assert!(
            err.error.message.contains(kind) && err.error.message.contains("engineer_briefing"),
            "unhelpful message: {}",
            err.error.message
        );
    }
}

#[tokio::test]
async fn an_unknown_kind_is_a_400_listing_the_known_ones() {
    let h = harness().await;
    let engineer = h.profile("eng", Role::Engineer).await;

    let err = h
        .error(
            put_json(
                &format!("/v1/profiles/{}/prompts/nope", engineer.id),
                serde_json::json!({ "content": "..." }),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert_eq!(err.error.code, "invalid_request");
    assert!(
        err.error.message.contains("engineer_briefing"),
        "unhelpful message: {}",
        err.error.message
    );
}

#[tokio::test]
async fn an_unknown_profile_is_a_404_on_every_prompt_route() {
    let h = harness().await;

    for request in [
        get("/v1/profiles/nosuchprofile/prompts"),
        put_json(
            "/v1/profiles/nosuchprofile/prompts/engineer_briefing",
            serde_json::json!({ "content": "..." }),
        ),
        post("/v1/profiles/nosuchprofile/prompts/engineer_briefing/reset"),
        post("/v1/profiles/nosuchprofile/system-prompt/reset"),
    ] {
        let err = h.error(request, StatusCode::NOT_FOUND).await;
        assert_eq!(err.error.code, "not_found");
    }
}

/// A profile created over the API starts on every default of its role and
/// stores none of them; a system prompt given at creation is the profile's own
/// until the reset takes it back off — the same two states a briefing has.
#[tokio::test]
async fn a_created_profile_starts_on_the_defaults_its_system_prompt_aside() {
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

    let prompts: Vec<ProfilePromptDto> =
        h.get(&format!("/v1/profiles/{}/prompts", plain.id)).await;
    assert_eq!(
        prompts.iter().map(|p| p.kind).collect::<Vec<_>>(),
        PromptKind::for_role(Role::Reviewer).to_vec()
    );
    for prompt in &prompts {
        assert_eq!(
            prompt.content,
            default_prompt(Role::Reviewer, prompt.kind).unwrap()
        );
        assert!(prompt.is_default);
    }
    assert_eq!(prompt_rows(&h).await, 0);

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

/// The planner sizes each slot it assigns, and that rule reaches a planner
/// nobody has edited: it is a constant, never a row, so the seeded Planner and
/// every profile still on the default answer with the text the code ships
/// today rather than the one that was current when they were created.
#[tokio::test]
async fn a_planner_on_the_default_prompt_is_briefed_to_size_the_slots_it_assigns() {
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
