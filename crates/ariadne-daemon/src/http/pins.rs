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
//! A discovered ACP agent is pinned by its catalog id whole,
//! `<agent-id>:<model>` (011): the id before the first colon names an agent
//! in the registry rather than a CLI, the pin is stored as agent kind `acp`
//! with that id as its model, and the launcher splits it again at the launch
//! (021). The disabled check and the effort check read the same catalog the
//! id came from.
//!
//! The store keeps the halves in their own columns, so this module is also
//! where the two that make a model are put back together for a response:
//! [`spelled`].

use ariadne_core::AgentKind;
use ariadne_core::models::{ModelRef, effort_error};
use ariadne_store::{AgentPin, Store};

use crate::acp_discovery::AgentRegistry;

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
/// up: that a model is missing, or that a string names no agent CLI and no
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
///
/// A string whose first segment names no agent CLI gets one more reading:
/// where it names an agent in the ACP registry, the pin is a discovered
/// catalog id, written whole as the model of kind `acp` — which is how the
/// launcher takes it back apart (021). Only a string neither reading takes
/// is refused.
///
/// The CLI reading always wins: a first segment that spells a native agent
/// kind is that CLI, whatever the registry holds — which is why discovery
/// refuses a configured registry id that spells one
/// (`acp_discovery::reserved_id`), rather than leave an agent nothing can
/// select.
fn parsed(model: &str, registry: &AgentRegistry) -> ApiResult<ModelRef> {
    let refused = match model.parse::<ModelRef>() {
        Ok(chosen) => {
            if chosen.agent_kind == AgentKind::Opencode && !chosen.model.contains('/') {
                return Err(ApiError::bad_request(format!(
                    "`{}` names no provider — an opencode model is written \
                     `provider/model`, as in `opencode:anthropic/{}`",
                    chosen.model, chosen.model
                )));
            }
            return Ok(chosen);
        }
        Err(refused) => refused,
    };
    if let Some((agent_id, rest)) = model.split_once(':')
        && registry.command_of(agent_id).is_some()
    {
        if rest.trim().is_empty() {
            return Err(ApiError::bad_request(format!(
                "no model after the `:` in `{model}` — a discovered agent is \
                 pinned `{agent_id}:<model>`, a model id its catalog lists"
            )));
        }
        return Ok(ModelRef {
            agent_kind: AgentKind::Acp,
            model: model.to_string(),
        });
    }
    Err(ApiError::bad_request(refused))
}

/// One `<agent_kind>:<model>` and the effort beside it as the pin they
/// spell, or the refusal naming what was typed and the form that would have
/// worked.
async fn pin(
    store: &Store,
    registry: &AgentRegistry,
    model: &str,
    effort: Option<&str>,
) -> ApiResult<AgentPin> {
    let chosen = parsed(model, registry)?;
    available(store, registry, &chosen).await?;
    if let Some(effort) = effort {
        checked(
            registry,
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
///
/// The id the switch stores is the catalog's, so what is asked is the same
/// spelling the request wrote: a registry pin's own model is that id whole.
async fn available(store: &Store, registry: &AgentRegistry, chosen: &ModelRef) -> ApiResult<()> {
    let off = store.disabled_models().await?;
    let disabled = off.contains(&chosen.to_string())
        || (chosen.agent_kind == AgentKind::Acp && off.contains(&chosen.model));
    match disabled {
        true => Err(ApiError::bad_request(format!(
            "`{}` is turned off — pin a model that is on, or turn this one \
             back on first",
            spelled(chosen.agent_kind, &chosen.model, registry)
        ))),
        false => Ok(()),
    }
}

/// One effort against the model it is to run at: the model's own efforts where
/// the catalog lists them, and everything its agent CLI accepts where nothing
/// does — a hand-typed model id the catalog has never heard of.
async fn checked(registry: &AgentRegistry, standing: Standing<'_>, effort: &str) -> ApiResult<()> {
    let efforts = efforts_of(registry, standing.agent_kind, standing.model).await;
    match effort_error(standing.agent_kind, efforts.as_deref(), effort) {
        Some(why) => Err(ApiError::bad_request(why)),
        None => Ok(()),
    }
}

/// The two columns a row keeps its model in, as the one string a response
/// carries. The effort rides beside it, in its own field.
///
/// A registry pin's model already is the one string — the discovered catalog
/// id, whose first segment the registry knows — so it is served whole rather
/// than under an `acp:` prefix nothing wrote. The registry is what tells it
/// from a fallback `acp` model that merely carries a colon: that one keeps
/// its `acp:` prefix, since its own spelling is the only one a re-submit
/// parses.
pub fn spelled(agent_kind: AgentKind, model: &str, registry: &AgentRegistry) -> String {
    if agent_kind == AgentKind::Acp
        && let Some((agent_id, _)) = model.split_once(':')
        && registry.command_of(agent_id).is_some()
    {
        return model.to_string();
    }
    ModelRef {
        agent_kind,
        model: model.to_string(),
    }
    .to_string()
}
