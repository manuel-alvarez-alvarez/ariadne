//! What a request chose to run on, as the pin the store writes.
//!
//! A goal, a task and every reviewer slot carry the agent CLI and, where one
//! was named, the model their agent runs on, pinned when they are created off
//! the profile behind them. Overriding that is one string,
//! `<agent_kind>[:<model>]` ([`ModelRef`]): the agent CLI on its own runs it on
//! its own default model, an agent with a model after the `:` pins both, and a
//! string naming no agent CLI is refused here — nothing derives one from the
//! other, and the model half is free text the CLI is handed as typed (opencode
//! discovers its own models at runtime, so no catalog could vouch for it
//! anyway).
//!
//! The store keeps the two halves in two columns, so this module is also where
//! they are put back together for a response: [`spelled`].

use ariadne_core::AgentKind;
use ariadne_core::models::ModelRef;
use ariadne_store::AgentPin;

use super::error::{ApiError, ApiResult};

/// What a request writes to mean "not my choice": on a creation the profile's
/// own model, on an update the profile's own model again, and on a profile
/// itself auto — the first installed CLI on its own default model. One word
/// wherever a model is written.
const CLEAR: [&str; 2] = ["", "default"];

/// The pin a creation asks for: None where it chose nothing — which is the
/// profile's own agent and model for a goal, a task or a slot, and auto for a
/// profile itself.
pub fn chosen(model: Option<&str>) -> ApiResult<Option<AgentPin>> {
    match model {
        None => Ok(None),
        // Empty is a field somebody meant to fill in, not a way to say
        // "nothing chosen" — leaving it out is what says that.
        Some("") => Err(ApiError::bad_request(
            "model is empty — write the agent CLI it runs on, `<agent_kind>[:<model>]`, \
             or leave the field out to choose nothing at all",
        )),
        Some(model) if CLEAR.contains(&model) => Ok(None),
        Some(model) => Ok(Some(pin(model)?)),
    }
}

/// The same for an edit, which has one more thing to say: `Some(None)` is the
/// caller asking for the profile's pins back, and None is the caller saying
/// nothing about them at all.
pub fn rechosen(model: Option<&str>) -> ApiResult<Option<Option<AgentPin>>> {
    match model {
        None => Ok(None),
        Some(model) if CLEAR.contains(&model) => Ok(Some(None)),
        Some(model) => Ok(Some(Some(pin(model)?))),
    }
}

/// One `<agent_kind>[:<model>]` as the pin it spells, or the refusal naming
/// what was typed and the form that would have worked.
fn pin(model: &str) -> ApiResult<AgentPin> {
    let chosen: ModelRef = model.parse().map_err(ApiError::bad_request)?;
    Ok(AgentPin {
        agent_kind: chosen.agent_kind,
        model: chosen.model,
    })
}

/// The two columns a row keeps its pin in, as the one string a response
/// carries: None where the row is on auto, which has no agent CLI to name and
/// so no spelling at all.
pub fn spelled(agent_kind: Option<AgentKind>, model: Option<&str>) -> Option<String> {
    Some(
        ModelRef {
            agent_kind: agent_kind?,
            model: model.map(str::to_string),
        }
        .to_string(),
    )
}
