//! What a request chose to run on, as the pin the store writes.
//!
//! A goal, a task and every reviewer slot carry the agent CLI, the model, and
//! where one was named the effort their agent runs at, pinned when they are
//! created. The first two are one string, `<agent_kind>:<model>`
//! ([`ModelRef`]): both halves are required — a model is required wherever an
//! agent is pinned, and no CLI default stands in for one — and a string
//! naming no agent CLI is refused here. The model half is free text the CLI
//! is handed as typed (opencode discovers its own models at runtime, so no
//! catalog could vouch for it anyway), except that an opencode model must
//! carry its `provider/` prefix, which is the spelling opencode itself takes
//! back.
//!
//! The effort is the field beside it, and it belongs to a model: it is checked
//! against the model that will be *effective* — the one the request names, or
//! else the one the row already runs on ([`Standing`]) — against that model's
//! own efforts where the catalog (`GET /v1/models`) lists them, and against
//! everything its CLI accepts where nothing does. So an effort written on its
//! own moves the effort and leaves the model where it is, and a model written
//! without one runs at that CLI's own default: the effort belonged to the
//! model that was left behind. `default` stays legal for the effort alone —
//! it clears the effort back to the CLI's own.
//!
//! The store keeps the halves in their own columns, so this module is also
//! where the two that make a model are put back together for a response:
//! [`spelled`].

use ariadne_core::AgentKind;
use ariadne_core::models::{ModelRef, effort_error};
use ariadne_store::{AgentPin, Store};

use super::catalog::models::efforts_of;
use super::error::{ApiError, ApiResult};

/// What an effort field writes to mean "the CLI's own": the same word an
/// update writes to clear it. Efforts only — a model has no default to clear
/// to, so `default` written as a model is refused.
const CLEAR_EFFORT: [&str; 2] = ["", "default"];

/// What a row runs on: the pin it already carries, which is what an effort
/// named with no model beside it is checked against and pinned to — so that
/// moving an effort alone leaves the model where it was.
#[derive(Debug, Clone, Copy)]
pub struct Standing<'a> {
    pub agent_kind: AgentKind,
    pub model: &'a str,
}

/// What an edit says about the pin a row is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repin {
    /// The request said nothing about it: the row keeps the pin it has.
    Untouched,
    /// Onto this agent, model and effort.
    To(AgentPin),
    /// The agent and model stay where they are and only the effort moves:
    /// `Some` to that effort, `None` back to the CLI's own.
    Effort(Option<String>),
}

/// The pin a creation asks for. A model is required: a request that names
/// none, or writes the empty string or `default`, is refused by the rule.
pub async fn chosen(
    store: &Store,
    model: Option<&str>,
    effort: Option<&str>,
) -> ApiResult<AgentPin> {
    pin(store, required(model)?, named(effort)).await
}

/// The same for an edit, which has one more thing to say: an effort can move
/// on its own, checked against the model the row already runs.
pub async fn rechosen(
    store: &Store,
    model: Option<&str>,
    effort: Option<&str>,
    standing: Standing<'_>,
) -> ApiResult<Repin> {
    match model {
        Some(model) => Ok(Repin::To(
            pin(store, required(Some(model))?, named(effort)).await?,
        )),
        None => match effort {
            None => Ok(Repin::Untouched),
            Some(effort) if CLEAR_EFFORT.contains(&effort) => Ok(Repin::Effort(None)),
            Some(effort) => {
                checked(standing, effort).await?;
                Ok(Repin::Effort(Some(effort.to_string())))
            }
        },
    }
}

/// A model refused for how it is written, before anything it names is looked
/// up: that a model is missing, or that a string names no agent CLI, is a
/// fact about the request, not about anything beside it. The whole check is
/// [`chosen`], which also asks the store what the user has turned off.
pub fn readable(model: Option<&str>) -> ApiResult<()> {
    parsed(required(model)?).map(|_| ())
}

/// The model a request must carry: not absent, not empty — whitespace alone
/// included — and not the word `default`. A model is required, and there is
/// no default to fall back to.
fn required(model: Option<&str>) -> ApiResult<&str> {
    match model {
        Some(model) if !model.trim().is_empty() && model != "default" => Ok(model),
        _ => Err(ApiError::bad_request(
            "a model is required — every agent names its CLI and its model, \
             `<agent_kind>:<model>`, and no default stands in for one",
        )),
    }
}

/// The effort a field actually names: None where it is absent or where it
/// carries the word that clears it, which says "the CLI's own" in as many
/// words as leaving it out does.
fn named(effort: Option<&str>) -> Option<&str> {
    effort.filter(|effort| !CLEAR_EFFORT.contains(effort))
}

/// One `<agent_kind>:<model>` as the [`ModelRef`] it spells, holding an
/// opencode model to the `provider/model` spelling opencode itself takes
/// back: one without the prefix would be dropped by opencode's own config,
/// and a model is never dropped silently.
fn parsed(model: &str) -> ApiResult<ModelRef> {
    let chosen: ModelRef = model.parse().map_err(ApiError::bad_request)?;
    if chosen.agent_kind == AgentKind::Opencode && !chosen.model.contains('/') {
        return Err(ApiError::bad_request(format!(
            "`{}` names no provider — an opencode model is written \
             `provider/model`, as in `opencode:anthropic/{}`",
            chosen.model, chosen.model
        )));
    }
    Ok(chosen)
}

/// One `<agent_kind>:<model>` and the effort beside it as the pin they
/// spell, or the refusal naming what was typed and the form that would have
/// worked.
async fn pin(store: &Store, model: &str, effort: Option<&str>) -> ApiResult<AgentPin> {
    let chosen = parsed(model)?;
    available(store, &chosen).await?;
    if let Some(effort) = effort {
        checked(
            Standing {
                agent_kind: chosen.agent_kind,
                model: &chosen.model,
            },
            effort,
        )
        .await?;
    }
    Ok(AgentPin {
        agent_kind: chosen.agent_kind,
        model: chosen.model,
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
async fn available(store: &Store, chosen: &ModelRef) -> ApiResult<()> {
    match store.disabled_models().await?.contains(&chosen.to_string()) {
        true => Err(ApiError::bad_request(format!(
            "`{chosen}` is turned off — pin a model that is on, or turn this one              back on first"
        ))),
        false => Ok(()),
    }
}

/// One effort against the model it is to run at: the model's own efforts where
/// the catalog lists them, and everything its agent CLI accepts where nothing
/// does — a hand-typed model id the catalog has never heard of.
async fn checked(standing: Standing<'_>, effort: &str) -> ApiResult<()> {
    let efforts = efforts_of(standing.agent_kind, standing.model).await;
    match effort_error(standing.agent_kind, efforts.as_deref(), effort) {
        Some(why) => Err(ApiError::bad_request(why)),
        None => Ok(()),
    }
}

/// The two columns a row keeps its model in, as the one string a response
/// carries. The effort rides beside it, in its own field.
pub fn spelled(agent_kind: AgentKind, model: &str) -> String {
    ModelRef {
        agent_kind,
        model: model.to_string(),
    }
    .to_string()
}
