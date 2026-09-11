//! What an agent event means: the internal session id it carries, the
//! lifecycle status it implies, whether it says a human has to act, and what
//! it reports having spent.
//!
//! The ACP runtime reports through [`super::events::ingest_event`] in one
//! vocabulary (`crate::acp`), and these tables are where it is read. An event
//! that matches nothing is still recorded — it simply moves neither flag.

use ariadne_core::TokenUsage;

/// The agent's own session id, where an event carries one: every payload the
/// runtime reports names it as `session_id`.
pub(super) fn extract_internal_id(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Session status implied by a lifecycle event kind (None = no change).
pub(super) fn status_for_event(kind: &str) -> Option<ariadne_core::SessionStatus> {
    use ariadne_core::SessionStatus as S;
    match kind {
        "session_start"
        | "user_prompt_submit"
        | "post_tool_use"
        | "pre_tool_use"
        // An answered approval hands control back to the agent, whichever way
        // it went: allowed it runs the call, rejected it gets the refusal as
        // the tool result and carries on. This is also what takes the
        // attention flag back down — see the clear in `ingest_event`.
        | "permission.replied" => Some(S::Running),
        "stop" => Some(S::Idle),
        "session_end" => Some(S::Exited),
        _ => None,
    }
}

/// Attention an event raises on its session (None = leave it alone).
pub(super) fn attention_for_event(kind: &str) -> Option<ariadne_core::AttentionReason> {
    use ariadne_core::AttentionReason as A;
    match kind {
        // A failed turn, or a protocol failure the runtime reports before the
        // session ends: only the flag goes up.
        "session.error" => Some(A::AgentError),
        // The agent asked for a permission. The runtime reports the ask as it
        // arrives, before the answer is known, so it marks the wait and not
        // its outcome; `permission.replied` follows with the answer, and
        // clears it.
        "permission_request" => Some(A::WaitingPermission),
        _ => None,
    }
}

/// The token usage an event reports, as `(source, totals)` — the transcript
/// the figures were read from, and its cumulative totals.
///
/// Any event may carry an `ariadne_usage`, and most carry none: an event
/// without one simply has no news. What is refused
/// is refused quietly — a payload whose counters are missing, fractional or
/// negative is a bug in whatever composed it, and failing the agent's event
/// over it would cost the daemon the status and attention the same event
/// carries.
pub(super) fn usage_for_event(payload: &serde_json::Value) -> Option<(String, TokenUsage)> {
    let reported = payload.get("ariadne_usage")?;
    let source = reported.get("source").and_then(|v| v.as_str());
    let counter = |key: &str| reported.get(key).and_then(serde_json::Value::as_u64);
    match (
        source,
        counter("input_tokens"),
        counter("cached_input_tokens"),
        counter("output_tokens"),
    ) {
        (Some(source), Some(input_tokens), Some(cached_input_tokens), Some(output_tokens))
            if !source.is_empty() =>
        {
            Some((
                source.to_string(),
                TokenUsage {
                    input_tokens,
                    cached_input_tokens,
                    output_tokens,
                },
            ))
        }
        _ => {
            tracing::warn!(usage = %reported, "ignoring a malformed ariadne_usage");
            None
        }
    }
}

// -- summary ------------------------------------------------------------

/// How long a summary may run before [`summarize`] cuts it, ellipsis
/// character included.
const MAX_SUMMARY_LEN: usize = 200;

/// The `tool_input` fields that say what a tool call does, tried in this
/// order — the first one present is the summary's subject.
const TOOL_INPUT_FIELDS: [&str; 7] = [
    "command",
    "file_path",
    "path",
    "pattern",
    "url",
    "prompt",
    "description",
];

/// The one line an event's payload is worth, for `ariadne events` and the
/// desktop app's agent-activity tab alike: an action and its subject for a
/// tool call, the agent's own words where it left any — and `…`, visibly,
/// where nothing here can be read.
///
/// Built once, when the DTO is built, from the vocabulary the runtime
/// reports in (this module's own docs): `tool_name`/`tool_input` on every
/// tool event. `cwd` is read only to relativize a path already read off one
/// of those — it is never a summary of its own, and it never appears in one.
pub(super) fn summarize(kind: &str, payload: &serde_json::Value) -> String {
    let text = tool_call_summary(payload).or_else(|| agent_text(kind, payload));
    finish(&text.unwrap_or_else(|| "…".to_string()))
}

/// A payload's field, where it holds a non-empty string.
fn non_empty_str(value: Option<&serde_json::Value>) -> Option<&str> {
    value.and_then(|v| v.as_str()).filter(|s| !s.is_empty())
}

/// A tool call, off the two fields the runtime puts on every one, whichever
/// half of the pre/post pair it is: `Bash: cargo nextest run`,
/// `Edit: crates/ariadne-api/src/events.rs`.
///
/// A permission request carries the same two fields, so the same reading
/// covers it: it is a tool event too, just one whose outcome is not known
/// yet.
fn tool_call_summary(payload: &serde_json::Value) -> Option<String> {
    let tool = non_empty_str(payload.get("tool_name"))?;
    let input = payload.get("tool_input")?;
    let (field, value) = TOOL_INPUT_FIELDS
        .iter()
        .find_map(|field| non_empty_str(input.get(field)).map(|v| (*field, v)))?;
    let subject = match field {
        "file_path" | "path" => relative_to_cwd(value, payload),
        _ => value.to_string(),
    };
    Some(format!("{tool}: {subject}"))
}

/// The agent's own words, wherever the payload carries any: the prompt that
/// began the turn, the message it ended on, or an error's —
/// `{data: {message, …}}`, the shape `session.error` carries it in.
fn agent_text(kind: &str, payload: &serde_json::Value) -> Option<String> {
    let text = match kind {
        "user_prompt_submit" => non_empty_str(payload.get("prompt")),
        "stop" => non_empty_str(payload.get("last_assistant_message")),
        "session.error" => non_empty_str(
            payload
                .get("error")
                .and_then(|e| e.get("data"))
                .and_then(|d| d.get("message")),
        ),
        _ => None,
    }?;
    Some(text.to_string())
}

/// `value` relative to the `cwd` the payload names, when it lies inside it;
/// `value` unchanged otherwise — an absolute path elsewhere, or one already
/// relative. `cwd` is never what ends up in the summary, only ever the base
/// a path under it is trimmed against.
fn relative_to_cwd(value: &str, payload: &serde_json::Value) -> String {
    let Some(cwd) = non_empty_str(payload.get("cwd")) else {
        return value.to_string();
    };
    let cwd = cwd.trim_end_matches('/');
    match value.strip_prefix(cwd) {
        Some("") => ".".to_string(),
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/').to_string(),
        _ => value.to_string(),
    }
}

/// One line, cut at [`MAX_SUMMARY_LEN`] characters and marked where it was —
/// counted, like every other cut in Ariadne, so an accent is never split in
/// half.
fn finish(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if flat.chars().count() <= MAX_SUMMARY_LEN {
        return flat;
    }
    flat.chars()
        .take(MAX_SUMMARY_LEN - 1)
        .chain(['…'])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        attention_for_event, extract_internal_id, status_for_event, summarize, usage_for_event,
    };

    use ariadne_core::{AttentionReason, SessionStatus, TokenUsage};
    use serde_json::json;

    /// A permission request as the runtime reports it: the call it is about,
    /// the options the agent offered, and no answer.
    fn permission_request() -> serde_json::Value {
        json!({
            "session_id": "stub-session",
            "tool_name": "Bash",
            "tool_input": {"command": "touch /tmp/probe"},
            "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"}],
        })
    }

    /// The id an event carries is the agent's session id, and nothing else.
    #[test]
    fn an_events_internal_id_is_its_sessions_and_nothing_elses() {
        assert_eq!(
            extract_internal_id(&permission_request()).as_deref(),
            Some("stub-session")
        );
        // No id, and an empty one, both leave it unknown.
        for payload in [
            json!({"stop_reason": "end_turn"}),
            json!({"session_id": ""}),
        ] {
            assert_eq!(extract_internal_id(&payload), None, "{payload}");
        }
    }

    /// Nothing the runtime reports may be dropped on the floor: every event
    /// kind it emits moves the status or the attention, or is read for what
    /// its payload carries.
    #[test]
    fn every_event_the_runtime_reports_is_acted_on() {
        for kind in [
            "session_start",
            "user_prompt_submit",
            "pre_tool_use",
            "post_tool_use",
            "permission_request",
            "permission.replied",
            "stop",
            "session.error",
            "session_end",
        ] {
            let acted_on = status_for_event(kind).is_some() || attention_for_event(kind).is_some();
            assert!(acted_on, "{kind} is reported but ingested as a no-op");
        }
        assert!(crate::agents::compaction_done(
            "compaction_update",
            &json!({"status": "completed"})
        ));
    }

    /// The lifecycle events, and nothing about them raises attention.
    #[test]
    fn a_lifecycle_event_moves_the_status_and_raises_nothing() {
        use SessionStatus::*;
        for (event, expected) in [
            ("session_start", Running),
            ("user_prompt_submit", Running),
            ("pre_tool_use", Running),
            ("post_tool_use", Running),
            ("stop", Idle),
            ("session_end", Exited),
        ] {
            assert_eq!(status_for_event(event), Some(expected), "{event}");
            assert_eq!(attention_for_event(event), None, "{event}");
        }
    }

    /// Every way an agent says it is blocked on the user — and none of them
    /// may read as liveness, since a `Running` mapping would clear the very
    /// attention the event raises.
    #[test]
    fn a_wait_on_the_user_is_raised_and_never_reads_as_liveness() {
        for (kind, expected) in [
            ("permission_request", AttentionReason::WaitingPermission),
            ("session.error", AttentionReason::AgentError),
        ] {
            assert_eq!(attention_for_event(kind), Some(expected), "{kind}");
            assert_eq!(status_for_event(kind), None, "{kind}");
        }
    }

    /// What takes the flag back down: the answer hands the agent control
    /// again, whichever way it went.
    #[test]
    fn an_answered_ask_hands_control_back_to_the_agent() {
        for kind in ["permission.replied", "post_tool_use", "user_prompt_submit"] {
            assert_eq!(
                status_for_event(kind),
                Some(SessionStatus::Running),
                "{kind}"
            );
            assert_eq!(attention_for_event(kind), None, "{kind}");
        }
    }

    /// The usage contract, read whole.
    #[test]
    fn an_event_reports_the_totals_of_the_transcript_it_names() {
        let payload = json!({
            "ariadne_usage": {
                "source": "stub-session",
                "input_tokens": 100,
                "cached_input_tokens": 80,
                "output_tokens": 10,
            },
        });
        assert_eq!(
            usage_for_event(&payload),
            Some((
                "stub-session".to_string(),
                TokenUsage {
                    input_tokens: 100,
                    cached_input_tokens: 80,
                    output_tokens: 10,
                }
            ))
        );
    }

    /// Most events carry no figures at all, and one that carries figures
    /// nobody can read is not a reason to fail the agent's event: both are
    /// simply no news.
    #[test]
    fn an_absent_or_malformed_report_is_no_news() {
        for payload in [
            json!({"stop_reason": "end_turn"}),
            json!({"ariadne_usage": {}}),
            json!({"ariadne_usage": {"source": "/x.jsonl", "input_tokens": 100}}),
            json!({"ariadne_usage": {"source": "", "input_tokens": 1,
                                     "cached_input_tokens": 0, "output_tokens": 1}}),
            json!({"ariadne_usage": {"source": "/x.jsonl", "input_tokens": -1,
                                     "cached_input_tokens": 0, "output_tokens": 1}}),
            json!({"ariadne_usage": {"source": "/x.jsonl", "input_tokens": 1.5,
                                     "cached_input_tokens": 0, "output_tokens": 1}}),
            json!({"ariadne_usage": {"source": "/x.jsonl", "input_tokens": "100",
                                     "cached_input_tokens": 0, "output_tokens": 1}}),
            json!({"ariadne_usage": "none of it"}),
        ] {
            assert_eq!(usage_for_event(&payload), None, "{payload}");
        }
    }

    /// A tool call reads as its action and its subject, off `tool_name` and
    /// `tool_input` alone — whichever half of the pre/post pair it is, and on
    /// the permission request put up before one.
    #[test]
    fn a_tool_call_reads_as_its_action_and_its_subject() {
        let call = |tool: &str, input: serde_json::Value| {
            json!({
                "session_id": "stub-session",
                "cwd": "/tmp/wt",
                "tool_name": tool,
                "tool_input": input,
            })
        };
        for (payload, expected) in [
            (
                call("Bash", json!({"command": "cargo nextest run"})),
                "Bash: cargo nextest run",
            ),
            (
                call(
                    "Edit",
                    json!({"file_path": "/tmp/wt/crates/ariadne-api/src/events.rs"}),
                ),
                "Edit: crates/ariadne-api/src/events.rs",
            ),
            (
                call("Grep", json!({"pattern": "fn summarize"})),
                "Grep: fn summarize",
            ),
        ] {
            assert_eq!(summarize("pre_tool_use", &payload), expected, "{payload}");
        }
        assert_eq!(
            summarize("permission_request", &permission_request()),
            "Bash: touch /tmp/probe"
        );
    }

    /// The agent's own words, wherever the payload carries any — read
    /// verbatim and flattened to one line.
    #[test]
    fn the_agents_own_words_are_shown_where_the_payload_carries_them() {
        for (kind, payload, expected) in [
            (
                "user_prompt_submit",
                json!({"prompt": "run the tests"}),
                "run the tests",
            ),
            (
                "stop",
                json!({"last_assistant_message": "Which of the two do you want?"}),
                "Which of the two do you want?",
            ),
            // A newline in the words themselves is flattened, not cut short.
            (
                "stop",
                json!({"last_assistant_message": "Line one\nLine two"}),
                "Line one Line two",
            ),
            (
                "session.error",
                json!({"error": {"data": {"message": "rate limited"}}}),
                "rate limited",
            ),
        ] {
            assert_eq!(summarize(kind, &payload), expected, "{kind}");
        }
    }

    /// A payload nothing above can read — here, one carrying only its `cwd` —
    /// summarizes to `…`, visibly, rather than to nothing.
    #[test]
    fn a_payload_with_nothing_readable_but_its_cwd_summarizes_to_an_ellipsis() {
        let payload = json!({"session_id": "stub-session", "cwd": "/tmp/wt"});
        assert_eq!(summarize("session_start", &payload), "…");
    }

    /// Rules 5 and 6: a path under the payload's `cwd` prints relative to
    /// it, one outside is left exactly as it came, and the cwd itself is
    /// never what ends up in the summary either way.
    #[test]
    fn a_path_under_the_cwd_prints_relative_and_the_cwd_never_appears() {
        let inside = json!({
            "cwd": "/tmp/wt",
            "tool_name": "Edit",
            "tool_input": {"file_path": "/tmp/wt/crates/ariadne-api/src/events.rs"},
        });
        let summary = summarize("pre_tool_use", &inside);
        assert_eq!(summary, "Edit: crates/ariadne-api/src/events.rs");
        assert!(!summary.contains("/tmp/wt"), "{summary}");

        let outside = json!({
            "cwd": "/tmp/wt",
            "tool_name": "Read",
            "tool_input": {"file_path": "/etc/hosts"},
        });
        assert_eq!(summarize("pre_tool_use", &outside), "Read: /etc/hosts");
    }

    /// Rule 8: a summary longer than 200 characters is cut there, and the
    /// cut is marked.
    #[test]
    fn a_long_summary_is_cut_at_200_characters() {
        let payload = json!({"tool_name": "Bash", "tool_input": {"command": "x".repeat(250)}});
        let summary = summarize("pre_tool_use", &payload);
        assert_eq!(summary.chars().count(), 200);
        assert!(summary.starts_with("Bash: "), "{summary}");
        assert!(summary.ends_with('…'), "{summary}");
    }
}
