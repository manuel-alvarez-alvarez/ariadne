//! Integration tests for the model catalog endpoint.
//!
//! The contract is that `GET /v1/models` returns everything an agent can be
//! pinned to, each entry's id spelled the way a request writes it: the models
//! discovery found each registry agent offering, `<agent>:<model>`, and no
//! bare-agent entry — a model is required wherever an agent is pinned.
//! Nothing scopes the catalog, so there is one answer and it is the union.
//! Each entry carries the efforts it can be run at, as the agent offered
//! them. What discovery reads off an agent is tested in `acp_discovery.rs`.
//!
//! Every entry also says whether an agent can be staffed on it. The catalog
//! is discovery, so what the database holds is the user's subtraction from
//! it: a model turned off stays listed, off, and is refused as a pin.

mod common;

use ariadne_api::models::ModelDto;

use axum::http::StatusCode;

use ariadne_api::error::ErrorBody;

use common::acp::{discovery_settled, option, registry_home, script, stub_acp_agent};
use common::{Harness, harness, put_json};

async fn models(h: &Harness) -> Vec<ModelDto> {
    h.get("/v1/models").await
}

/// A harness whose registry agent `stub` offers two models, and three
/// efforts with `medium` the one it runs at, discovered.
async fn two_model_harness(dir: &std::path::Path) -> Harness {
    let mut offer = script();
    offer["config_options"] = serde_json::json!([
        {
            "id": "model-id", "name": "Model", "category": "model", "type": "select",
            "currentValue": "old-model",
            "options": [
                {"value": "old-model", "name": "The old one"},
                {"value": "new-model", "name": "The new one"},
            ],
        },
        {
            "id": "effort-id", "name": "Effort", "category": "thought_level", "type": "select",
            "currentValue": "medium",
            "options": [{"value": "low"}, {"value": "medium"}, {"value": "high"}],
        },
    ]);
    let stub = stub_acp_agent(dir, offer);
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    discovery_settled(&h, &stub).await;
    h
}

/// Every model an agent offered is listed under that agent's registry id,
/// with the efforts it offered, cheapest first as offered, and the one it
/// runs at by default flagged. Nothing is written about a discovered model
/// beyond what the agent said: its description and its efforts.
#[tokio::test]
async fn every_discovered_model_is_listed_as_its_agent_runs_it() {
    let dir = tempfile::tempdir().unwrap();
    let h = two_model_harness(dir.path()).await;
    let got = models(&h).await;
    assert_eq!(
        got.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["stub:old-model", "stub:new-model"]
    );
    for model in &got {
        assert_eq!(model.agent_id, "stub");
        assert_eq!(
            model
                .efforts
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["low", "medium", "high"]
        );
        assert_eq!(
            model
                .efforts
                .iter()
                .filter(|e| e.default)
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["medium"]
        );
        assert!(model.enabled);
    }
    assert_eq!(got[1].description.as_deref(), Some("The new one"));

    let raw: Vec<serde_json::Value> = h.get("/v1/models").await;
    let mut fields: Vec<&str> = raw[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        ["agent_id", "description", "efforts", "enabled", "id"]
    );
}

/// An agent discovery has not accepted offers nothing: the catalog is what
/// discovery found, and a daemon that has not run it has found nothing.
#[tokio::test]
async fn an_agent_discovery_has_not_accepted_offers_no_model() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), script());
    let h = harness().home(registry_home(&stub)).await;
    assert!(models(&h).await.is_empty());
}

/// No agent is offered on its own: a model is required wherever an agent is
/// pinned, so a bare-agent entry would be an id no request may write. Every
/// entry names both halves, `<agent>:<model>`.
#[tokio::test]
async fn no_bare_agent_entry_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let h = two_model_harness(dir.path()).await;
    let got = models(&h).await;
    assert!(!got.is_empty());
    for entry in &got {
        assert!(
            entry.id.starts_with(&format!("{}:", entry.agent_id)),
            "`{}` and its agent_id disagree",
            entry.id
        );
        assert!(
            entry.id.len() > entry.agent_id.len() + 1,
            "`{}` names no model, and nothing may pin it",
            entry.id
        );
    }
}

/// The endpoint is part of the OpenAPI document, and nothing scopes it.
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
    let dir = tempfile::tempdir().unwrap();
    let h = two_model_harness(dir.path()).await;
    let id = "stub:old-model";
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
    let dir = tempfile::tempdir().unwrap();
    let h = two_model_harness(dir.path()).await;
    let envelope: ErrorBody = h
        .error(
            put_json(
                "/v1/models/enabled",
                serde_json::json!({"id": "stub:no-such-model", "enabled": false}),
            ),
            StatusCode::NOT_FOUND,
        )
        .await;
    assert!(
        envelope.error.message.contains("stub:no-such-model"),
        "{}",
        envelope.error.message
    );
}

/// The last model left on cannot be turned off. A daemon that can staff
/// nothing is not a state to leave a user in, and it is the one state this
/// endpoint could put them in.
#[tokio::test]
async fn the_last_model_left_on_cannot_be_turned_off() {
    let dir = tempfile::tempdir().unwrap();
    let h = two_model_harness(dir.path()).await;
    let _: ModelDto = h
        .json(
            put_json(
                "/v1/models/enabled",
                serde_json::json!({"id": "stub:new-model", "enabled": false}),
            ),
            StatusCode::OK,
        )
        .await;

    let envelope: ErrorBody = h
        .error(
            put_json(
                "/v1/models/enabled",
                serde_json::json!({"id": "stub:old-model", "enabled": false}),
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

/// A model option with no choices of its own offers the one it holds: an
/// agent that names only its current model is still an agent with a model.
#[tokio::test]
async fn a_model_option_with_no_choices_offers_its_current_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut offer = script();
    offer["config_options"] = serde_json::json!([option("model-id", "model", "only-model")]);
    let stub = stub_acp_agent(dir.path(), offer);
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    discovery_settled(&h, &stub).await;
    assert_eq!(
        models(&h)
            .await
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["stub:only-model"]
    );
}
