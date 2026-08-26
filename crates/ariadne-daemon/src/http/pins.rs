//! The model a request chose, as the pin the store writes.
//!
//! A goal, a task and every reviewer slot carry the agent CLI and the model
//! their agent runs on, pinned when they are created off the profile behind
//! them. Choosing a model overrides that pair — and only that pair: the agent
//! CLI follows the model, because a model belongs to one CLI and nothing else
//! decides which ([`agent_kind_of`]). A model no rule places is refused here,
//! where the answer is about the request, rather than stored as a model whose
//! agent nobody could name at spawn time.

use ariadne_core::models::agent_kind_of;
use ariadne_store::AgentPin;

use super::error::{ApiError, ApiResult};

/// What an update writes to put the pins back on the profile's, spelled the
/// way `PUT /v1/profiles/{id}` spells the same thing.
const CLEAR: [&str; 2] = ["", "default"];

/// The pin a creation asks for: None where it named no model and the profile's
/// own agent and model are what gets pinned.
pub fn chosen(model: Option<&str>) -> ApiResult<Option<AgentPin>> {
    match model {
        None => Ok(None),
        // Only an update has something to clear; on a creation an empty model
        // is a field somebody meant to fill in.
        Some("") => Err(ApiError::bad_request(
            "model is empty — leave it out to run on the profile's own model",
        )),
        Some(model) => Ok(Some(pin(model)?)),
    }
}

/// The same for an edit, which has one more thing to say: `Some(None)` is the
/// caller asking for the profile's pins back, and None is the caller saying
/// nothing about the model at all.
pub fn rechosen(model: Option<&str>) -> ApiResult<Option<Option<AgentPin>>> {
    match model {
        None => Ok(None),
        Some(model) if CLEAR.contains(&model) => Ok(Some(None)),
        Some(model) => Ok(Some(Some(pin(model)?))),
    }
}

/// A model as the pair that runs it, or the refusal that names it: the user
/// picks models, so an id nothing places is a question only the user can
/// answer.
fn pin(model: &str) -> ApiResult<AgentPin> {
    match agent_kind_of(model) {
        Some(agent_kind) => Ok(AgentPin {
            agent_kind,
            model: model.to_string(),
        }),
        None => Err(ApiError::bad_request(format!(
            "unknown model `{model}`: cannot tell which agent runs it"
        ))),
    }
}
