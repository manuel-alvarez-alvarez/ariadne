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

mod common;

use ariadne_api::models::ModelDto;
use ariadne_core::models::curated_models;
use ariadne_core::{AgentKind, ModelTier};

use common::{Harness, harness};

async fn models(h: &Harness) -> Vec<ModelDto> {
    h.get("/v1/models").await
}

/// Every curated model is listed under its agent CLI, with everything a
/// planner sizes a task from and an id that carries the CLI it runs on.
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
