//! What a request chose to run on, as the pin the store writes.
//!
//! A goal, a task and every reviewer slot carry the model, and where one was
//! named the effort their agent runs at, pinned when they are created. The
//! model is one string, `<agent>:<model>` ([`ModelRef`]): the id of an agent
//! in the ACP registry, and a model of it. Both halves are required — a model
//! is required wherever an agent is pinned, and no agent default stands in
//! for one — and a string whose first segment names no registry agent is
//! refused here. The model half is free text the agent is handed as typed:
//! a discovered catalog lists what an agent offers, but an agent may take a
//! model its catalog does not list.
//!
//! The effort is the field beside it, and it belongs to a model: it is
//! checked against the model that will be *effective* — the one the request
//! names, or else the one the row already runs on ([`Standing`]) — against
//! that model's own efforts where the catalog (`GET /v1/models`) lists them,
//! and against anything that is not blank where nothing does. So an effort
//! written on its own moves the effort and leaves the model where it is, and
//! a model written without one runs at that agent's own default: the effort
//! belonged to the model that was left behind. `default` stays legal for the
//! effort alone — it clears the effort back to the agent's own.
//!
//! The store keeps the pin whole, so the string a request wrote is the string
//! every response carries back, and the launcher splits it again at the
//! launch (021).

use ariadne_core::models::{ModelRef, effort_error};
use ariadne_store::{AgentPin, Store};

use crate::acp_discovery::AgentRegistry;

use super::catalog::models::efforts_of;
use super::error::{ApiError, ApiResult};

/// What an effort field writes to mean "the agent's own": the same word an
/// update writes to clear it. Efforts only — a model has no default to clear
/// to, so `default` written as a model is refused.
const CLEAR_EFFORT: [&str; 2] = ["", "default"];

/// What a row runs on: the pin it already carries, which is what an effort
/// named with no model beside it is checked against and pinned to — so that
/// moving an effort alone leaves the model where it was.
#[derive(Debug, Clone, Copy)]
pub struct Standing<'a> {
    pub model: &'a str,
}

/// What an edit says about the pin a row is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repin {
    /// The request said nothing about it: the row keeps the pin it has.
    Untouched,
    /// Onto this agent, model and effort.
    To(AgentPin),
    /// The model stays where it is and only the effort moves: `Some` to that
    /// effort, `None` back to the agent's own.
    Effort(Option<String>),
}

/// The pin a creation asks for. A model is required: a request that names
/// none, or writes the empty string or `default`, is refused by the rule.
pub async fn chosen(
    store: &Store,
    registry: &AgentRegistry,
    model: Option<&str>,
    effort: Option<&str>,
) -> ApiResult<AgentPin> {
    pin(store, registry, required(model)?, named(effort)).await
}

/// The same for an edit, which has one more thing to say: an effort can move
/// on its own, checked against the model the row already runs.
pub async fn rechosen(
    store: &Store,
    registry: &AgentRegistry,
    model: Option<&str>,
    effort: Option<&str>,
    standing: Standing<'_>,
) -> ApiResult<Repin> {
    match model {
        Some(model) => Ok(Repin::To(
            pin(store, registry, required(Some(model))?, named(effort)).await?,
        )),
        None => match effort {
            None => Ok(Repin::Untouched),
            Some(effort) if CLEAR_EFFORT.contains(&effort) => Ok(Repin::Effort(None)),
            Some(effort) => {
                checked(registry, standing, effort).await?;
                Ok(Repin::Effort(Some(effort.to_string())))
            }
        },
    }
}

/// A model refused for how it is written, before anything it names is looked
/// up in the store: that a model is missing, or that a string names no
/// registry agent, is a fact about the request, not about anything beside
/// it. The whole check is [`chosen`], which also asks the store what the
/// user has turned off.
pub fn readable(model: Option<&str>, registry: &AgentRegistry) -> ApiResult<()> {
    parsed(required(model)?, registry).map(|_| ())
}

/// The model a request must carry: not absent, not empty — whitespace alone
/// included — and not the word `default`. A model is required, and there is
/// no default to fall back to.
fn required(model: Option<&str>) -> ApiResult<&str> {
    match model {
        Some(model) if !model.trim().is_empty() && model != "default" => Ok(model),
        _ => Err(ApiError::bad_request(
            "a model is required — every agent names its registry agent and its \
             model, `<agent>:<model>`, and no default stands in for one",
        )),
    }
}

/// The effort a field actually names: None where it is absent or where it
/// carries the word that clears it, which says "the agent's own" in as many
/// words as leaving it out does.
fn named(effort: Option<&str>) -> Option<&str> {
    effort.filter(|effort| !CLEAR_EFFORT.contains(effort))
}

/// One `<agent>:<model>` as the [`ModelRef`] it spells, holding its first
/// segment to an agent the registry holds: a pin naming no agent could never
/// be launched, and an agent that exists only as a typo is refused here
/// rather than at the launch.
fn parsed(model: &str, registry: &AgentRegistry) -> ApiResult<ModelRef> {
    let chosen: ModelRef = model.parse().map_err(ApiError::bad_request)?;
    if registry.command_of(&chosen.agent).is_none() {
        return Err(ApiError::bad_request(format!(
            "unknown agent `{}` in `{model}` — what stands before the `:` is the \
             id of an agent in the ACP registry (`GET /v1/acp-agents`)",
            chosen.agent
        )));
    }
    Ok(chosen)
}

/// One `<agent>:<model>` and the effort beside it as the pin they
/// spell, or the refusal naming what was typed and the form that would have
/// worked.
async fn pin(
    store: &Store,
    registry: &AgentRegistry,
    model: &str,
    effort: Option<&str>,
) -> ApiResult<AgentPin> {
    let chosen = parsed(model, registry)?;
    available(store, &chosen).await?;
    if let Some(effort) = effort {
        checked(registry, Standing { model }, effort).await?;
    }
    Ok(AgentPin {
        model: chosen.to_string(),
        effort: effort.map(str::to_string),
    })
}

/// A model the user has turned off is not one an agent can be staffed on
/// (`PUT /v1/models/enabled`).
///
/// Asked only of a model the request *names*. An effort moved on its own is
/// checked against the model the row already runs, and refusing that would
/// trap a row on a model that was turned off under it: a pin is a snapshot,
/// and work already staffed keeps running on what it was staffed with.
///
/// The id the switch stores is the catalog's, so what is asked is the same
/// spelling the request wrote.
async fn available(store: &Store, chosen: &ModelRef) -> ApiResult<()> {
    let off = store.disabled_models().await?;
    match off.contains(&chosen.to_string()) {
        true => Err(ApiError::bad_request(format!(
            "`{chosen}` is turned off — pin a model that is on, or turn this one \
             back on first"
        ))),
        false => Ok(()),
    }
}

/// One effort against the model it is to run at: the model's own efforts where
/// the catalog lists them, and anything that is not blank where nothing does
/// — a hand-typed model id the catalog has never heard of.
async fn checked(registry: &AgentRegistry, standing: Standing<'_>, effort: &str) -> ApiResult<()> {
    let efforts = efforts_of(registry, standing.model).await;
    match effort_error(efforts.as_deref(), effort) {
        Some(why) => Err(ApiError::bad_request(why)),
        None => Ok(()),
    }
}
