//! Subagents: the child sessions an agent announces on its own session, and
//! then runs under their own session ids on the same connection (021).
//!
//! Three shapes announce one. The draft protocol's `subagent_update` names
//! the child by `sessionId` and patches what it names. codex-acp and
//! claude-agent-acp send `subagent_spawned` with a `name` and a `task`, and
//! `subagent_state_update` with a state word, each naming the child by
//! `subagentSessionId`. All three are read as raw JSON, since the SDK keeps
//! the draft behind its `unstable_subagents` feature, and stored as one
//! kind, [`SUBAGENT_UPDATE`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// The one stored kind every subagent announcement and state change folds
/// into, on the parent session.
pub(crate) const SUBAGENT_UPDATE: &str = "subagent_update";

/// The children one launch's agent announced, by their session ids.
#[derive(Clone, Default)]
pub(crate) struct Subagents(Arc<Mutex<HashMap<String, Child>>>);

/// What the agent said of one child so far.
#[derive(Clone, Default)]
struct Child {
    title: Option<String>,
    description: Option<String>,
    state: Option<&'static str>,
    cancel: bool,
}

impl Subagents {
    /// The `subagent_update` payload one update folds into, where it is one
    /// of the three shapes and names a child: the parent session it came on,
    /// the child, its state and what the client may do to it. The first one
    /// for a child is its announcement.
    pub(crate) fn fold(&self, parent: &Value, update: &Value) -> Option<Value> {
        let kind = update.get("sessionUpdate").and_then(Value::as_str)?;
        let id_field = match kind {
            SUBAGENT_UPDATE => "sessionId",
            "subagent_spawned" | "subagent_state_update" => "subagentSessionId",
            _ => return None,
        };
        let id = update.get(id_field).and_then(Value::as_str)?.to_string();
        let mut children = self.0.lock().expect("subagents lock");
        let child = children.entry(id.clone()).or_default();
        match kind {
            SUBAGENT_UPDATE => {
                if let Some(title) = update.get("title") {
                    child.title = title.as_str().map(str::to_string);
                }
                if let Some(description) = update.get("description") {
                    child.description = description.as_str().map(str::to_string);
                }
                if let Some(capabilities) = update.get("capabilities") {
                    child.cancel = can_cancel(capabilities);
                }
                if let Some(state) = update.get("state") {
                    child.state = draft_state(state);
                }
            }
            "subagent_spawned" => {
                child.title = update
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                child.description = update
                    .get("task")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                child.cancel = update.get("capabilities").is_some_and(can_cancel);
                child.state = Some("running");
            }
            _ => {
                child.state = update
                    .get("state")
                    .and_then(Value::as_str)
                    .and_then(word_state)
            }
        }
        Some(json!({
            "session_id": parent,
            "subagent": {"id": id, "title": child.title, "description": child.description},
            "state": child.state,
            "capabilities": {"cancel": child.cancel},
        }))
    }

    /// The `{id, title}` every stored event of an announced child carries,
    /// where `session_id` is one. `None` for the parent's own session id,
    /// and for any other one nobody announced.
    pub(crate) fn tag(&self, session_id: &Value) -> Option<Value> {
        let id = session_id.as_str()?;
        let children = self.0.lock().expect("subagents lock");
        let child = children.get(id)?;
        Some(json!({"id": id, "title": child.title}))
    }

    /// Whether `id` may be cancelled now: announced, not ended, and
    /// advertising the capability, as its latest update said.
    pub(crate) fn cancel_eligibility(&self, id: &str) -> CancelEligibility {
        let children = self.0.lock().expect("subagents lock");
        let Some(child) = children.get(id) else {
            return CancelEligibility::Unknown;
        };
        if matches!(child.state, Some("completed" | "failed" | "cancelled")) {
            return CancelEligibility::Ended;
        }
        match child.cancel {
            true => CancelEligibility::Ready,
            false => CancelEligibility::NotAdvertised,
        }
    }
}

/// Whether a child may be cancelled now, and why not where it may not
/// (021): the route's `409` names one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelEligibility {
    /// No child of this id was ever announced.
    Unknown,
    /// The child's work already ended.
    Ended,
    /// The child never said it could be cancelled.
    NotAdvertised,
    Ready,
}

/// Whether a child's capabilities let the client cancel it: a `cancel`
/// object, even `{}`, says yes, and anything else no.
fn can_cancel(capabilities: &Value) -> bool {
    capabilities.get("cancel").is_some_and(Value::is_object)
}

/// A draft state object as one of the stored words. An idle child ended its
/// work: cancelled where it says so, failed on a refusal or an error, and
/// completed otherwise. A child waiting on the user is still running. An
/// unknown or cleared state is none.
fn draft_state(state: &Value) -> Option<&'static str> {
    match state.get("state").and_then(Value::as_str)? {
        "running" | "requires_action" => Some("running"),
        "idle" => Some(match state.get("stopReason").and_then(Value::as_str) {
            Some("cancelled") => "cancelled",
            Some("refusal" | "error") => "failed",
            _ => "completed",
        }),
        _ => None,
    }
}

/// An adapter's state word as one of the stored words. A child the adapter
/// lost track of, `disconnected`, ended without its end: failed.
fn word_state(state: &str) -> Option<&'static str> {
    match state {
        "running" => Some("running"),
        "completed" => Some("completed"),
        "failed" | "disconnected" => Some("failed"),
        "cancelled" => Some("cancelled"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CancelEligibility, Subagents};

    /// A draft idle state names why the work stopped, and the stored word
    /// follows it.
    #[test]
    fn an_idle_child_reads_as_how_its_work_ended() {
        let states = [
            (
                json!({"state": "idle", "stopReason": "end_turn"}),
                json!("completed"),
            ),
            (json!({"state": "idle"}), json!("completed")),
            (
                json!({"state": "idle", "stopReason": "cancelled"}),
                json!("cancelled"),
            ),
            (
                json!({"state": "idle", "stopReason": "refusal"}),
                json!("failed"),
            ),
            (
                json!({"state": "idle", "stopReason": "error", "message": "x"}),
                json!("failed"),
            ),
            (json!({"state": "requires_action"}), json!("running")),
            (json!({"state": "unknown"}), json!(null)),
            (json!(null), json!(null)),
        ];
        for (state, stored) in states {
            let subagents = Subagents::default();
            let update = json!({"sessionUpdate": "subagent_update", "sessionId": "c",
                                "state": state});
            let folded = subagents.fold(&json!("p"), &update).unwrap();
            assert_eq!(folded["state"], stored, "{state}");
        }
    }

    /// Only an announced child is tagged.
    #[test]
    fn only_an_announced_child_is_tagged() {
        let subagents = Subagents::default();
        assert_eq!(subagents.tag(&json!("c")), None);
        subagents.fold(
            &json!("p"),
            &json!({"sessionUpdate": "subagent_spawned", "subagentSessionId": "c",
                    "name": "explorer"}),
        );
        assert_eq!(
            subagents.tag(&json!("c")),
            Some(json!({"id": "c", "title": "explorer"}))
        );
        assert_eq!(subagents.tag(&json!("p")), None);
    }

    /// A later announcement with a concrete `capabilities` object replaces
    /// the value, even to clear a `cancel` the child had advertised before.
    #[test]
    fn a_later_concrete_capabilities_object_replaces_an_earlier_cancel() {
        let subagents = Subagents::default();
        let first = subagents
            .fold(
                &json!("p"),
                &json!({"sessionUpdate": "subagent_update", "sessionId": "c", "title": "x",
                        "capabilities": {"cancel": {}}}),
            )
            .unwrap();
        assert_eq!(first["capabilities"], json!({"cancel": true}));

        let second = subagents
            .fold(
                &json!("p"),
                &json!({"sessionUpdate": "subagent_update", "sessionId": "c",
                        "capabilities": {}}),
            )
            .unwrap();
        assert_eq!(
            second["capabilities"],
            json!({"cancel": false}),
            "a concrete object with no cancel key clears the earlier value"
        );
        assert_eq!(
            subagents.cancel_eligibility("c"),
            CancelEligibility::NotAdvertised
        );
    }

    /// A child may be cancelled only once it is announced, while it still
    /// runs, and only where it advertised the capability.
    #[test]
    fn a_child_may_be_cancelled_only_while_running_and_advertising_it() {
        let subagents = Subagents::default();
        assert_eq!(
            subagents.cancel_eligibility("c"),
            CancelEligibility::Unknown
        );

        subagents.fold(
            &json!("p"),
            &json!({"sessionUpdate": "subagent_update", "sessionId": "c", "title": "x",
                    "capabilities": {"cancel": {}}}),
        );
        assert_eq!(subagents.cancel_eligibility("c"), CancelEligibility::Ready);

        subagents.fold(
            &json!("p"),
            &json!({"sessionUpdate": "subagent_update", "sessionId": "c",
                    "state": {"state": "idle", "stopReason": "end_turn"}}),
        );
        assert_eq!(subagents.cancel_eligibility("c"), CancelEligibility::Ended);

        subagents.fold(
            &json!("p"),
            &json!({"sessionUpdate": "subagent_update", "sessionId": "d", "title": "y",
                    "capabilities": {}}),
        );
        assert_eq!(
            subagents.cancel_eligibility("d"),
            CancelEligibility::NotAdvertised
        );
    }
}
