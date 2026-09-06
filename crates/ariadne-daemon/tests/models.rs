//! Integration tests for the model catalog endpoint.
//!
//! The contract is that `GET /v1/models` returns everything an agent can be
//! pinned to, each entry's id spelled the way a request writes it: every agent
//! CLI on its own — that CLI on its own default model — and then the curated
//! models of it, `<agent_kind>:<model>`. Nothing scopes the catalog any more,
//! so there is one answer and it is the union. Each entry says what the model
//! is for and carries the efforts it can be run at. OpenCode discovery is not
//! exercised here: it depends on an installed `opencode` binary, and its
//! parser is unit-tested in the daemon.
//!
//! Every entry also says whether an agent can be staffed on it. The catalog
//! is code and discovery, so what the database holds is the user's
//! subtraction from it: a model turned off stays listed, off, and is refused
//! as a pin.

mod common;

use ariadne_api::models::ModelDto;
use ariadne_core::models::curated_models;
use ariadne_core::{AgentKind, ModelTier};

use axum::http::StatusCode;

use ariadne_api::error::ErrorBody;

use common::{Harness, harness, put_json};

async fn models(h: &Harness) -> Vec<ModelDto> {
    h.get("/v1/models").await
}

/// Every curated model is listed under its agent CLI, with everything a
/// orchestrator sizes a task from and an id that carries the CLI it runs on.
#[tokio::test]
async fn every_curated_model_is_listed_as_its_agent_runs_it() {
    let h = harness().await;
    let got = models(&h).await;
    for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
        for want in curated_models(kind) {
            let id = format!("{}:{}", kind.as_str(), want.id);
            let found = got
                .iter()
                .find(|m| m.id == id)
                .unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(found.agent_kind, kind, "{id}");
            assert_eq!(found.description.as_deref(), Some(want.description), "{id}");
            assert_ne!(found.tier, ModelTier::Unknown, "{id}");
            for band in [found.cost, found.speed] {
                let band = band.unwrap_or_else(|| panic!("{id} is unranked"));
                assert!((1..=5).contains(&band), "{id}: {band}");
            }
            assert!(!found.best_for.is_empty(), "{id}");
            assert!(!found.avoid_for.is_empty(), "{id}");
            assert!(
                found.efforts.iter().all(|e| e.description.is_some()),
                "{id}: every effort says what it buys"
            );
        }
    }
}

/// A curated model carries the efforts it can be run at, cheapest first, and
/// flags the one its CLI runs it at when none is passed — including the models
/// that take no effort at all, which say so with an empty list.
#[tokio::test]
async fn a_curated_model_carries_its_efforts_and_its_default() {
    let h = harness().await;
    let got = models(&h).await;
    let found = |id: &str| {
        got.iter()
            .find(|m| m.id == id)
            .unwrap_or_else(|| panic!("missing {id}"))
            .clone()
    };
    let ids = |m: &ModelDto| -> Vec<String> { m.efforts.iter().map(|e| e.id.clone()).collect() };
    let defaults = |m: &ModelDto| -> Vec<String> {
        m.efforts
            .iter()
            .filter(|e| e.default)
            .map(|e| e.id.clone())
            .collect()
    };

    let luna = found("codex:gpt-5.6-luna");
    assert_eq!(ids(&luna), ["low", "medium", "high", "xhigh", "max"]);
    assert_eq!(defaults(&luna), ["medium"], "exactly one, and it is medium");
    assert_eq!(
        luna.efforts[0].description.as_deref(),
        Some("Fast responses with lighter reasoning: small, well-specified changes")
    );

    let opus = found("claude_code:claude-opus-4-7");
    assert_eq!(defaults(&opus), ["xhigh"], "the one model that runs deep");

    for id in [
        "claude_code:claude-haiku-4-5",
        "claude_code:claude-sonnet-4-5",
    ] {
        let model = found(id);
        assert!(model.efforts.is_empty(), "{id} takes no effort at all");
    }
}

/// Each agent CLI is offered on its own as well, which is that CLI on whatever
/// model it defaults to — the pin a picker offers where no model is chosen.
#[tokio::test]
async fn each_agent_is_offered_on_its_own_default_model() {
    let h = harness().await;
    let got = models(&h).await;
    for kind in AgentKind::ALL {
        let found = got
            .iter()
            .find(|m| m.id == kind.as_str())
            .unwrap_or_else(|| panic!("missing {}", kind.as_str()));
        assert_eq!(found.agent_kind, kind);
        assert!(
            found
                .description
                .as_deref()
                .is_some_and(|d| d.contains("its own default model")),
            "{:?}",
            found.description
        );
        // Which model it is, is the CLI's own business, so nothing here
        // claims to know what it is like or what it is run at.
        assert_eq!(found.tier, ModelTier::Unknown);
        assert_eq!((found.cost, found.speed), (None, None));
        assert!(found.best_for.is_empty() && found.avoid_for.is_empty());
        assert!(found.efforts.is_empty());
    }
}

/// The endpoint is part of the OpenAPI document, and nothing scopes it: the
/// `agent` parameter went with the agent field it filtered.
#[tokio::test]
async fn endpoint_is_in_the_openapi_document_with_nothing_to_filter_by() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    let get = &doc["paths"]["/v1/models"]["get"];
    assert!(get.is_object());
    assert!(doc["components"]["schemas"]["ModelDto"].is_object());
    assert!(doc["components"]["schemas"]["EffortDto"].is_object());
    assert!(get["parameters"].is_null(), "{get}");
}

/// Turning a model off leaves it in the catalog and takes it out of use.
///
/// It stays listed because a catalog that hid it would leave nothing to turn
/// back on, and nothing to explain a refusal by. What changes is `enabled`,
/// which is what every surface reads: the desktop app greys the row, the CLI
/// prints `no`, and the orchestrator is never offered it at all.
#[tokio::test]
async fn a_model_turned_off_stays_in_the_catalog_and_out_of_use() {
    let h = harness().await;
    let id = "claude_code:claude-opus-5";
    assert!(
        models(&h).await.iter().all(|m| m.enabled),
        "a fresh daemon has nothing turned off"
    );

    let off: ModelDto = h
        .json(
            put_json(
                "/v1/models/enabled",
                serde_json::json!({"id": id, "enabled": false}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(off.id, id);
    assert!(!off.enabled);

    let listed = models(&h).await;
    let found = listed.iter().find(|m| m.id == id).expect("still listed");
    assert!(!found.enabled, "and listed as off");
    assert!(
        listed.iter().filter(|m| !m.enabled).count() == 1,
        "and nothing else moved with it"
    );

    // Turning it back on is the same call, and the catalog is whole again.
    let on: ModelDto = h
        .json(
            put_json(
                "/v1/models/enabled",
                serde_json::json!({"id": id, "enabled": true}),
            ),
            StatusCode::OK,
        )
        .await;
    assert!(on.enabled);
    assert!(models(&h).await.iter().all(|m| m.enabled));
}

/// An id the catalog does not carry is a 404 naming it: the catalog is what
/// there is, and a typo that wrote a row into the database would be a model
/// turned off that nothing could ever turn back on.
#[tokio::test]
async fn a_model_the_catalog_does_not_carry_cannot_be_turned_off() {
    let h = harness().await;
    let envelope: ErrorBody = h
        .error(
            put_json(
                "/v1/models/enabled",
                serde_json::json!({"id": "claude_code:no-such-model", "enabled": false}),
            ),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert!(
        envelope.error.message.contains("claude_code:no-such-model"),
        "{}",
        envelope.error.message
    );
}

/// The last model left on cannot be turned off. A daemon that can staff
/// nothing is not a state to leave a user in, and it is the one state this
/// endpoint could put them in.
///
/// The catalog is re-read between writes rather than listed once: opencode's
/// half of it is whatever discovery answers at that moment, so what "every
/// other entry" means is a question with a fresh answer each time.
#[tokio::test]
async fn the_last_model_left_on_cannot_be_turned_off() {
    let h = harness().await;
    let keep = "codex";
    // Everything but one, off — however many passes the catalog takes to
    // stop offering another.
    loop {
        let others: Vec<String> = models(&h)
            .await
            .into_iter()
            .filter(|m| m.enabled && m.id != keep)
            .map(|m| m.id)
            .collect();
        if others.is_empty() {
            break;
        }
        for id in others {
            let _: ModelDto = h
                .json(
                    put_json(
                        "/v1/models/enabled",
                        serde_json::json!({"id": id, "enabled": false}),
                    ),
                    StatusCode::OK,
                )
                .await;
        }
    }

    let envelope: ErrorBody = h
        .error(
            put_json(
                "/v1/models/enabled",
                serde_json::json!({"id": keep, "enabled": false}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        envelope.error.message.contains("last model"),
        "{}",
        envelope.error.message
    );
    assert!(
        models(&h).await.iter().any(|m| m.enabled),
        "and something is still there to staff an agent on"
    );
}
