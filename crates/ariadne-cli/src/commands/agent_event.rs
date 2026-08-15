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

pub async fn run(kind: String, argv_json: Option<String>, json: Option<String>) {
    // Never propagate failures to the calling hook.
    let _ = tokio::time::timeout(Duration::from_secs(2), forward(kind, argv_json, json)).await;
}

async fn forward(kind: String, argv_json: Option<String>, json: Option<String>) {
    let Ok(session_id) = std::env::var(SESSION_ENV) else {
        return; // not spawned by ariadne
    };

    let (agent_kind, event_kind, payload) = match kind.as_str() {
        // Claude Code: hook JSON arrives on stdin; hook_event_name names it.
        "claude" => {
            let mut raw = String::new();
            if std::io::stdin().read_to_string(&mut raw).is_err() {
                return;
            }
            let payload: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
            let event = payload
                .get("hook_event_name")
                .and_then(|v| v.as_str())
                .map(camel_to_snake)
                .unwrap_or_else(|| "unknown".into());
            (AgentKind::ClaudeCode, event, payload)
        }
        // Codex: notify appends one JSON argv argument.
        "codex" => {
            let payload: serde_json::Value =
                serde_json::from_str(argv_json.as_deref().unwrap_or("{}")).unwrap_or_default();
            let event = match payload.get("type").and_then(|v| v.as_str()) {
                Some("agent-turn-complete") => "turn_complete".to_string(),
                Some(other) => other.replace('-', "_"),
                None => "unknown".into(),
            };
            (AgentKind::Codex, event, payload)
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
