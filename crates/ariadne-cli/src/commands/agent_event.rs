//! `ariadne agent-event` — invoked by coding-agent hooks to report events.
//!
//! MUST be fail-safe: a dead daemon or malformed payload must never block or
//! crash the agent, so every path exits 0 and the POST is capped at 2s.

use std::io::Read;
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

    let (agent_kind, event_kind, payload) = match kind.as_str() {
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
    use super::parse_hook_event;

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
}
