//! What a request chose to run on, as the pin the store writes.
//!
//! A goal, a task and every reviewer slot carry the agent CLI and, where one
//! was named, the model their agent runs on, pinned when they are created off
//! the profile behind them. Overriding that is a choice of agent first: an
//! agent alone runs that CLI on its own default model, an agent with a model
//! pins both, and a model on its own is refused here — nothing derives one
//! from the other, and the model is free text the CLI is handed as typed
//! (opencode discovers its own models at runtime, so no catalog could vouch
//! for it anyway).

use ariadne_core::AgentKind;
use ariadne_store::AgentPin;

use super::error::{ApiError, ApiResult};

/// What a request writes to mean "no model of my own, the agent CLI's
/// default", and — on an update, in `agent_kind` — to put the pins back on the
/// profile's. Both spelled the way `PUT /v1/profiles/{id}` spells the same
/// thing.
const CLEAR: [&str; 2] = ["", "default"];

/// The pin a creation asks for: None where it named no agent and the profile's
/// own agent and model are what gets pinned.
pub fn chosen(agent_kind: Option<AgentKind>, model: Option<&str>) -> ApiResult<Option<AgentPin>> {
    match agent_kind {
        Some(agent_kind) => Ok(Some(AgentPin {
            agent_kind,
            model: named(model),
        })),
        None => match model {
            None => Ok(None),
            Some(model) => Err(orphan(model)),
        },
    }
}

/// The same for an edit, which has one more thing to say: `Some(None)` is the
/// caller asking for the profile's pins back, and None is the caller saying
/// nothing about them at all.
pub fn rechosen(
    agent_kind: Option<&str>,
    model: Option<&str>,
) -> ApiResult<Option<Option<AgentPin>>> {
    match agent_kind {
        None => match model {
            None => Ok(None),
            Some(model) => Err(orphan(model)),
        },
        Some(kind) if CLEAR.contains(&kind) => match named(model) {
            // Handing the pins back to the profile and naming a model are two
            // different requests, and one of them would have to be dropped.
            Some(model) => Err(ApiError::bad_request(format!(
                "model `{model}` came with agent_kind `{kind}`, which puts the task \
                 back on its profile's agent and model — name the agent CLI instead"
            ))),
            None => Ok(Some(None)),
        },
        Some(kind) => Ok(Some(Some(AgentPin {
            agent_kind: kind.parse().map_err(ApiError::bad_request)?,
            model: named(model),
        }))),
    }
}

/// The model of a pin: what was typed, or None where nothing was — which is
/// the agent CLI's own default, and is also what its two spellings mean.
fn named(model: Option<&str>) -> Option<String> {
    model.filter(|m| !CLEAR.contains(m)).map(str::to_string)
}

/// A model nobody said what to run: the agent is the choice, so the answer is
/// a refusal naming the model rather than a guess at the CLI behind it.
fn orphan(model: &str) -> ApiError {
    if model.is_empty() {
        return ApiError::bad_request(
            "model is empty — leave it out to run on the profile's own agent and model",
        );
    }
    ApiError::bad_request(format!(
        "model `{model}` names no agent — choose the agent CLI it runs on"
    ))
}
