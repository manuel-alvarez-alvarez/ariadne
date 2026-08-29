//! What a request chose to run on, as the pin the store writes.
//!
//! A goal, a task and every reviewer slot carry the agent CLI, where one was
//! named the model, and where one was named the effort their agent runs at,
//! pinned when they are created off the profile behind them. Overriding the
//! first two is one string, `<agent_kind>[:<model>]` ([`ModelRef`]): the agent
//! CLI on its own runs it on its own default model, an agent with a model
//! after the `:` pins both, and a string naming no agent CLI is refused here —
//! nothing derives one from the other, and the model half is free text the CLI
//! is handed as typed (opencode discovers its own models at runtime, so no
//! catalog could vouch for it anyway).
//!
//! The effort is the field beside it, and it belongs to a model: it is checked
//! against the model that will be *effective* — the one the request names, or
//! else the one the row would run on anyway ([`Standing`]) — against that
//! model's own efforts where the catalog (`GET /v1/models`) lists them, and
//! against everything its CLI accepts where nothing does. So an effort written
//! on its own moves the effort and leaves the model where it is, and a model
//! written without one runs at that CLI's own default: the effort belonged to
//! the model that was left behind.
//!
//! The store keeps the halves in their own columns, so this module is also
//! where the two that make a model are put back together for a response:
//! [`spelled`].

use ariadne_core::AgentKind;
use ariadne_core::models::{ModelRef, effort_error};
use ariadne_store::AgentPin;

use super::catalog::models::efforts_of;
use super::error::{ApiError, ApiResult};

/// What a request writes to mean "not my choice": on a creation the profile's
/// own model, on an update the profile's own model again, and on a profile
/// itself auto — the first installed CLI on its own default model. One word
/// wherever a model is written, and the same word wherever an effort is —
/// where it means the CLI's own effort rather than the profile's.
const CLEAR: [&str; 2] = ["", "default"];

/// What a row runs on where the request names no model of its own: the profile
/// behind it on a creation, and the row as it stands on an edit.
///
/// An effort written with no model beside it is checked against this and
/// pinned to it, so that moving an effort alone leaves the model where it was.
#[derive(Debug, Clone, Copy)]
pub struct Standing<'a> {
    pub agent_kind: Option<AgentKind>,
    pub model: Option<&'a str>,
}

impl Standing<'_> {
    /// Nothing to fall back on: a profile being created is on auto until it is
    /// spawned, so there is no model an effort of its own could run at.
    pub fn auto() -> Self {
        Self {
            agent_kind: None,
            model: None,
        }
    }
}

/// What an edit says about the pin a row is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repin {
    /// The request said nothing about it: the row keeps the pin it has.
    Untouched,
    /// Back to the profile's own agent, model and effort as they stand now.
    Profile,
    /// Onto this agent, model and effort.
    To(AgentPin),
    /// The agent and model stay where they are and only the effort moves:
    /// `Some` to that effort, `None` back to the CLI's own.
    Effort(Option<String>),
}

/// The pin a creation asks for: None where it chose nothing — which is the
/// profile's own agent, model and effort for a goal, a task or a slot, and
/// auto for a profile itself.
pub async fn chosen(
    model: Option<&str>,
    effort: Option<&str>,
    standing: Standing<'_>,
) -> ApiResult<Option<AgentPin>> {
    match model {
        // Empty is a field somebody meant to fill in, not a way to say
        // "nothing chosen" — leaving it out is what says that.
        Some("") => Err(ApiError::bad_request(
            "model is empty — write the agent CLI it runs on, `<agent_kind>[:<model>]`, \
             or leave the field out to choose nothing at all",
        )),
        Some(model) if !CLEAR.contains(&model) => Ok(Some(pin(model, named(effort)).await?)),
        // No model chosen, so the row takes the profile's — at the effort it
        // was given, where it was given one, and at the profile's own where it
        // was not.
        _ => match named(effort) {
            None => Ok(None),
            Some(effort) => Ok(Some(at_standing(standing, effort).await?)),
        },
    }
}

/// The same for an edit, which has two more things to say: a model can be
/// handed back to the profile, and an effort can move on its own.
pub async fn rechosen(
    model: Option<&str>,
    effort: Option<&str>,
    standing: Standing<'_>,
) -> ApiResult<Repin> {
    match model {
        Some(model) if !CLEAR.contains(&model) => Ok(Repin::To(pin(model, named(effort)).await?)),
        // Handing the model back hands back the profile's effort with it:
        // which model that is, is the store's to read, so an effort named
        // beside it has nothing here to be checked against.
        Some(_) => {
            no_effort_alone(effort)?;
            Ok(Repin::Profile)
        }
        None => match effort {
            None => Ok(Repin::Untouched),
            Some(effort) if CLEAR.contains(&effort) => Ok(Repin::Effort(None)),
            Some(effort) => {
                checked(standing, effort).await?;
                Ok(Repin::Effort(Some(effort.to_string())))
            }
        },
    }
}

/// A model refused for how it is written, before anything it names is looked
/// up: that a string names no agent CLI is a fact about the request, not about
/// the profiles beside it. The whole check is [`chosen`], which also needs
/// what the row would run on where the request named no model of its own.
pub fn readable(model: Option<&str>) -> ApiResult<()> {
    match model {
        Some(model) if !CLEAR.contains(&model) => model
            .parse::<ModelRef>()
            .map(|_| ())
            .map_err(ApiError::bad_request),
        _ => Ok(()),
    }
}

/// The effort a field actually names: None where it is absent or where it
/// carries the word that clears it, which says "the CLI's own" in as many
/// words as leaving it out does.
fn named(effort: Option<&str>) -> Option<&str> {
    effort.filter(|effort| !CLEAR.contains(effort))
}

/// An effort named where the model is being handed back to a profile: what
/// that model is, is read when the row is written, so there is nothing here to
/// check the effort against.
fn no_effort_alone(effort: Option<&str>) -> ApiResult<()> {
    match named(effort) {
        None => Ok(()),
        Some(effort) => Err(ApiError::bad_request(format!(
            "`{effort}` is an effort beside a model handed back to its profile — \
             write the model it runs on too, `<agent_kind>[:<model>]`, or leave \
             the effort out to run at whatever that profile is on"
        ))),
    }
}

/// One `<agent_kind>[:<model>]` and the effort beside it as the pin they
/// spell, or the refusal naming what was typed and the form that would have
/// worked.
async fn pin(model: &str, effort: Option<&str>) -> ApiResult<AgentPin> {
    let chosen: ModelRef = model.parse().map_err(ApiError::bad_request)?;
    if let Some(effort) = effort {
        checked(
            Standing {
                agent_kind: Some(chosen.agent_kind),
                model: chosen.model.as_deref(),
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

/// An effort named with no model beside it: the row stays on the model it
/// would have run on and runs it at this effort.
async fn at_standing(standing: Standing<'_>, effort: &str) -> ApiResult<AgentPin> {
    checked(standing, effort).await?;
    Ok(AgentPin {
        // Checked above: a standing with no agent CLI is refused there.
        agent_kind: standing.agent_kind.expect("a standing model to run at"),
        model: standing.model.map(str::to_string),
        effort: Some(effort.to_string()),
    })
}

/// One effort against the model it is to run at: the model's own efforts where
/// the catalog lists them, and everything its agent CLI accepts where nothing
/// does — a hand-typed model id, or the CLI on its own default model.
///
/// A standing with no agent CLI at all is auto, resolved at spawn time to
/// whichever CLI is installed: nothing here knows what would accept the
/// effort, so it is refused rather than stored unchecked.
async fn checked(standing: Standing<'_>, effort: &str) -> ApiResult<()> {
    let Some(agent_kind) = standing.agent_kind else {
        return Err(ApiError::bad_request(format!(
            "`{effort}` is an effort with no model to run at — nothing here is \
             pinned to an agent CLI, so write the model it runs on too, \
             `<agent_kind>[:<model>]`"
        )));
    };
    let efforts = efforts_of(agent_kind, standing.model).await;
    match effort_error(agent_kind, efforts.as_deref(), effort) {
        Some(why) => Err(ApiError::bad_request(why)),
        None => Ok(()),
    }
}

/// The two columns a row keeps its model in, as the one string a response
/// carries: None where the row is on auto, which has no agent CLI to name and
/// so no spelling at all. The effort rides beside it, in its own field.
pub fn spelled(agent_kind: Option<AgentKind>, model: Option<&str>) -> Option<String> {
    Some(
        ModelRef {
            agent_kind: agent_kind?,
            model: model.map(str::to_string),
        }
        .to_string(),
    )
}
