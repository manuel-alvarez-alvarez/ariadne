//! Integration tests for the model catalog endpoint.
//!
//! The contract is that `GET /v1/models?agent=<kind>` returns exactly the
//! curated catalog of that agent with its descriptions, and that the endpoint
//! is in the OpenAPI document. OpenCode discovery is not exercised: it
//! depends on an installed `opencode` binary.

mod common;

use axum::http::StatusCode;

use ariadne_api::models::ModelDto;
use ariadne_core::AgentKind;
use ariadne_core::models::curated_models;

use common::{harness, Harness, get};

async fn models(h: &Harness, path: &str) -> Vec<ModelDto> {
    h.get(path).await
}

/// `?agent=<kind>` returns exactly that agent's curated catalog, in order,
/// each entry carrying its description.
#[tokio::test]
async fn curated_agents_return_their_catalog() {
    let h = harness().await;
    for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
        let got = models(&h, &format!("/v1/models?agent={}", kind.as_str())).await;
        let want = curated_models(kind);
        assert_eq!(got.len(), want.len());
        for (got, want) in got.iter().zip(want) {
            assert_eq!(got.id, want.id);
            assert_eq!(got.agent_kind, kind);
            assert_eq!(got.description.as_deref(), Some(want.description));
        }
    }
}

/// Without `agent`, the union: every curated entry is present (plus whatever
/// opencode discovery yields on the machine, which is not asserted).
#[tokio::test]
async fn no_param_returns_the_union() {
    let h = harness().await;
    let got = models(&h, "/v1/models").await;
    for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
        for want in curated_models(kind) {
            assert!(
                got.iter().any(|m| m.id == want.id && m.agent_kind == kind),
                "missing {}/{}",
                kind.as_str(),
                want.id
            );
        }
    }
}

/// An unknown agent kind is a 400, not a 500 or a silent union.
#[tokio::test]
async fn unknown_agent_is_rejected() {
    let h = harness().await;
    let (status, _) = h.send(get("/v1/models?agent=nonsense")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The endpoint is part of the OpenAPI document.
#[tokio::test]
async fn endpoint_is_in_the_openapi_document() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/models"]["get"].is_object());
    assert!(doc["components"]["schemas"]["ModelDto"].is_object());
}
