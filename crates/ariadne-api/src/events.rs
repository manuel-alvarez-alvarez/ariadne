//! Agent-event DTOs.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// The fields in a `permission.replied` event payload.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PermissionReplyDto {
    pub session_id: serde_json::Value,
    pub option_id: Option<String>,
    pub console_option_id: Option<String>,
    pub decided_by: String,
    pub label: Option<String>,
    pub danger: Option<f64>,
    pub allow_threshold: Option<f64>,
    pub deny_threshold: Option<f64>,
    pub ai_error: Option<String>,
    pub operation: Option<String>,
    pub risk_tags: Option<Vec<String>>,
    pub cap: Option<String>,
    pub probabilities: Option<serde_json::Value>,
    /// The row that answered without a console choice.
    #[schema(required = true)]
    pub learned_id: Option<String>,
    /// `command` or `family` when a row answered.
    #[schema(required = true)]
    pub learned_level: Option<String>,
    /// The normalized command key or family of the answering row.
    #[schema(required = true)]
    pub learned_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentEventDto {
    pub id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    /// e.g. session_start, post_tool_use, stop
    pub kind: String,
    pub payload: serde_json::Value,
    /// The one-line gist of `payload`, built by the daemon from the event's
    /// own vocabulary rather than stored: an action and its subject for a tool
    /// call, the agent's own words where it left any, and `…` where nothing
    /// of it can be read.
    pub summary: String,
    pub created_at: String,
}

/// An agent event as the domain stream carries it: everything of
/// [`AgentEventDto`] but the payload, which reaches 1 MB. A client that wants
/// the payload reads `GET /v1/events`, or the console stream.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AgentEventSummaryDto {
    pub id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    /// e.g. session_start, post_tool_use, stop
    pub kind: String,
    /// The one-line gist of the payload, as [`AgentEventDto::summary`].
    pub summary: String,
    pub created_at: String,
}

impl From<&AgentEventDto> for AgentEventSummaryDto {
    fn from(e: &AgentEventDto) -> Self {
        Self {
            id: e.id.clone(),
            session_id: e.session_id.clone(),
            task_id: e.task_id.clone(),
            kind: e.kind.clone(),
            summary: e.summary.clone(),
            created_at: e.created_at.clone(),
        }
    }
}

/// One event the ACP runtime reports for an agent session, on its way into
/// the store.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IngestEventRequest {
    /// The Ariadne session the agent runs under.
    pub session_id: String,
    /// Which launch of that session the reporting agent process is. Absent
    /// from a report that names none, which is one nothing is concluded from.
    #[serde(default)]
    pub launch: Option<String>,
    /// Normalized event kind.
    pub kind: String,
    /// The event's payload.
    pub payload: serde_json::Value,
}

/// Which end of the recorded events a page of `GET /v1/events` is taken from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EventOrder {
    /// Oldest first, which is what a sweep forward with `after` walks.
    #[default]
    Asc,
    /// Newest first, which is what a snapshot of the recent past asks for.
    Desc,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, IntoParams)]
pub struct EventListQuery {
    /// Filter by session id.
    pub session: Option<String>,
    /// Filter by task id.
    pub task: Option<String>,
    /// Filter by goal id: every event whose session, or whose task, belongs
    /// to that goal.
    pub goal: Option<String>,
    /// Return events with an id less than this one, which is how a descending
    /// page walks further back.
    pub before: Option<String>,
    /// Which end of the recorded events the page is taken from (default
    /// `asc`).
    pub order: Option<EventOrder>,
}

/// The phrase each `session.diagnosis` category reads as, beside the
/// original failure it never replaces (024). `None` for a category this
/// build does not recognise, read back by its raw spelling instead of
/// invented words.
fn diagnosis_label(category: &str) -> Option<&'static str> {
    Some(match category {
        "exhausted" => "quota exhaustion",
        "temporary" => "a temporary failure",
        "auth_config" => "an authentication or configuration problem",
        "task_error" => "a task failure",
        "insufficient" => "insufficient evidence",
        _ => return None,
    })
}

/// The advisory note a `session.diagnosis` event's payload reads as:
/// `AI suggests: quota exhaustion (72%)`, or without a percent where the
/// category's own probability did not validate. `None` where the payload
/// carries no category at all.
pub fn diagnosis_note(payload: &serde_json::Value) -> Option<String> {
    let category = payload.get("category")?.as_str()?;
    let label = diagnosis_label(category).unwrap_or(category);
    let percent = payload
        .get("probabilities")
        .and_then(|probabilities| probabilities.get(category))
        .and_then(serde_json::Value::as_f64)
        .map(|probability| (probability * 100.0).round() as i64);
    Some(match percent {
        Some(percent) => format!("AI suggests: {label} ({percent}%)"),
        None => format!("AI suggests: {label}"),
    })
}

#[cfg(test)]
mod tests {
    use super::diagnosis_note;
    use serde_json::json;

    /// A known category with a validated probability reads as its phrase
    /// and its whole percent.
    #[test]
    fn a_known_category_with_a_probability_reads_its_phrase_and_percent() {
        assert_eq!(
            diagnosis_note(&json!({
                "category": "exhausted",
                "probabilities": {"exhausted": 0.7231, "temporary": 0.1},
            })),
            Some("AI suggests: quota exhaustion (72%)".to_string())
        );
    }

    /// A category with no probabilities at all still reads its phrase,
    /// without a percent.
    #[test]
    fn a_category_without_probabilities_reads_its_phrase_alone() {
        assert_eq!(
            diagnosis_note(&json!({"category": "insufficient"})),
            Some("AI suggests: insufficient evidence".to_string())
        );
    }

    /// A payload with no category at all has no note.
    #[test]
    fn a_payload_without_a_category_has_no_note() {
        assert_eq!(diagnosis_note(&json!({})), None);
    }
}
