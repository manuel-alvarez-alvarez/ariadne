//! `ariadne agent-event` — invoked by coding-agent hooks to report events.
//!
//! MUST be fail-safe: a dead daemon or malformed payload must never block or
//! crash the agent, so every path exits 0, and the whole of it — reading the
//! transcript for the token counts included — is capped at 2s.

mod usage;

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use ariadne_api::events::IngestEventRequest;
use ariadne_client::Client;
use ariadne_core::AgentKind;

const SESSION_ENV: &str = "ARIADNE_SESSION_ID";

pub async fn run(kind: String, json: Option<String>) {
    // Never propagate failures to the calling hook.
    let _ = tokio::time::timeout(Duration::from_secs(2), forward(kind, json)).await;
}

async fn forward(kind: String, json: Option<String>) {
    let Ok(session_id) = std::env::var(SESSION_ENV) else {
        return; // not spawned by ariadne
    };

    let (agent_kind, event_kind, mut payload) = match kind.as_str() {
        // Claude Code and Codex share the hook protocol: the payload arrives
        // as JSON on stdin and `hook_event_name` names the event.
        "claude" | "codex" => {
            let agent_kind = if kind == "codex" {
                AgentKind::Codex
            } else {
                AgentKind::ClaudeCode
            };
            let mut raw = String::new();
            if std::io::stdin().read_to_string(&mut raw).is_err() {
                return;
            }
            let (event, payload) = parse_hook_event(&raw);
            (agent_kind, event, payload)
        }
        // OpenCode: the plugin passes {"kind": ..., "payload": ...} as --json.
        "opencode" => {
            let value: serde_json::Value =
                serde_json::from_str(json.as_deref().unwrap_or("{}")).unwrap_or_default();
            let event = value
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let payload = value.get("payload").cloned().unwrap_or_default();
            (AgentKind::Opencode, event, payload)
        }
        _ => return,
    };

    attach_usage(agent_kind, &event_kind, &mut payload);

    let client = Client::from_env();
    let _ = client
        .post_json::<serde_json::Value, _>(
            "/internal/agent-events",
            &IngestEventRequest {
                session_id,
                agent_kind,
                kind: event_kind,
                payload,
            },
        )
        .await;
}

/// The events a transcript is worth reading for: the turn boundaries, where
/// the counters have just moved. Reading on every tool call instead would
/// re-read a multi-megabyte file dozens of times a turn for an answer that
/// only changes when the turn ends.
const USAGE_EVENTS: [&str; 3] = ["stop", "session_end", "user_prompt_submit"];

/// Add `ariadne_usage` to the payload, when the agent's transcript can be read.
///
/// The counters are cumulative for `source`, the transcript they came from,
/// so the daemon can store the latest report and ignore the order they arrive
/// in. Every failure — an event that is not a turn boundary, no
/// `transcript_path`, a file that is gone or unreadable, a shape that has
/// changed — leaves the payload as it was: usage is a bonus, never a reason
/// for the hook to do anything different.
fn attach_usage(agent_kind: AgentKind, event: &str, payload: &mut serde_json::Value) {
    if !USAGE_EVENTS.contains(&event) {
        return;
    }
    let Some(source) = payload
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        return;
    };
    let usage = match agent_kind {
        AgentKind::ClaudeCode => usage::claude_usage(Path::new(&source)),
        AgentKind::Codex => usage::codex_usage(Path::new(&source)),
        _ => None,
    };
    let (Some(usage), Some(object)) = (usage, payload.as_object_mut()) else {
        return;
    };
    object.insert(
        "ariadne_usage".into(),
        serde_json::json!({
            "source": source,
            "input_tokens": usage.input_tokens,
            "cached_input_tokens": usage.cached_input_tokens,
            "output_tokens": usage.output_tokens,
        }),
    );
}

/// Split a hook payload into (event kind, payload). Unparsable input is
/// reported as an `unknown` event rather than dropped: the POST still marks
/// the session alive.
fn parse_hook_event(raw: &str) -> (String, serde_json::Value) {
    let payload: serde_json::Value = serde_json::from_str(raw).unwrap_or_default();
    let event = payload
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .map(camel_to_snake)
        .unwrap_or_else(|| "unknown".into());
    (event, payload)
}

fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ariadne_core::AgentKind;
    use tempfile::TempDir;

    use super::{attach_usage, parse_hook_event};

    /// A real Codex 0.147 SessionStart payload.
    const CODEX_SESSION_START: &str = r#"{
        "session_id": "01a00b36-1234-7890-abcd-ef0123456789",
        "transcript_path": "/Users/me/.codex/sessions/rollout.jsonl",
        "cwd": "/Users/me/work",
        "hook_event_name": "SessionStart",
        "model": "gpt-5",
        "permission_mode": "bypassPermissions",
        "source": "startup"
    }"#;

    #[test]
    fn codex_session_start_carries_the_session_id() {
        let (event, payload) = parse_hook_event(CODEX_SESSION_START);
        assert_eq!(event, "session_start");
        assert_eq!(
            payload["session_id"],
            "01a00b36-1234-7890-abcd-ef0123456789"
        );
    }

    #[test]
    fn event_names_become_snake_case() {
        for (name, expected) in [
            ("UserPromptSubmit", "user_prompt_submit"),
            ("PreToolUse", "pre_tool_use"),
            ("PermissionRequest", "permission_request"),
            ("PostToolUse", "post_tool_use"),
            ("Stop", "stop"),
            ("SessionEnd", "session_end"),
        ] {
            let raw = format!(r#"{{"hook_event_name": "{name}"}}"#);
            assert_eq!(parse_hook_event(&raw).0, expected);
        }
    }

    #[test]
    fn garbage_is_reported_as_unknown() {
        assert_eq!(parse_hook_event("not json").0, "unknown");
        assert_eq!(parse_hook_event("{}").0, "unknown");
    }

    /// One assistant message, over the two lines Claude writes it as.
    const TRANSCRIPT: &str = concat!(
        r#"{"type":"assistant","message":{"id":"msg_a","usage":{"input_tokens":10,"#,
        r#""cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"#,
        r#""output_tokens":7}}}"#,
        "\n",
        r#"{"type":"assistant","message":{"id":"msg_a","usage":{"input_tokens":10,"#,
        r#""cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"#,
        r#""output_tokens":7}}}"#,
        "\n",
    );

    fn stop_payload(transcript: &str) -> serde_json::Value {
        let raw = format!(r#"{{"hook_event_name": "Stop", "transcript_path": "{transcript}"}}"#);
        parse_hook_event(&raw).1
    }

    #[test]
    fn a_stop_reports_the_usage_its_transcript_holds() {
        let dir = TempDir::new().expect("a temp dir");
        let transcript = dir.path().join("session.jsonl");
        fs::write(&transcript, TRANSCRIPT).expect("write the transcript");
        let source = transcript.to_str().expect("a utf-8 path");

        let mut payload = stop_payload(source);
        attach_usage(AgentKind::ClaudeCode, "stop", &mut payload);

        assert_eq!(payload["ariadne_usage"]["source"], source);
        assert_eq!(payload["ariadne_usage"]["input_tokens"], 1110);
        assert_eq!(payload["ariadne_usage"]["cached_input_tokens"], 1000);
        assert_eq!(payload["ariadne_usage"]["output_tokens"], 7);
    }

    #[test]
    fn a_transcript_that_is_not_there_reports_nothing() {
        let mut payload = stop_payload("/nowhere/session.jsonl");
        attach_usage(AgentKind::ClaudeCode, "stop", &mut payload);
        assert!(payload.get("ariadne_usage").is_none());
        // The event still goes out with everything it arrived with.
        assert_eq!(payload["transcript_path"], "/nowhere/session.jsonl");
    }

    #[test]
    fn only_turn_boundaries_read_the_transcript() {
        let dir = TempDir::new().expect("a temp dir");
        let transcript = dir.path().join("session.jsonl");
        fs::write(&transcript, TRANSCRIPT).expect("write the transcript");
        let source = transcript.to_str().expect("a utf-8 path");

        for event in ["pre_tool_use", "post_tool_use", "session_start", "unknown"] {
            let mut payload = stop_payload(source);
            attach_usage(AgentKind::ClaudeCode, event, &mut payload);
            assert!(payload.get("ariadne_usage").is_none(), "{event}");
        }
        for event in ["stop", "session_end", "user_prompt_submit"] {
            let mut payload = stop_payload(source);
            attach_usage(AgentKind::ClaudeCode, event, &mut payload);
            assert!(payload.get("ariadne_usage").is_some(), "{event}");
        }
    }

    /// OpenCode reports its own usage through the plugin; nothing here knows
    /// how to read what it writes.
    #[test]
    fn opencode_payloads_are_left_alone() {
        let mut payload = stop_payload("/nowhere/session.jsonl");
        attach_usage(AgentKind::Opencode, "stop", &mut payload);
        assert!(payload.get("ariadne_usage").is_none());
    }
}
