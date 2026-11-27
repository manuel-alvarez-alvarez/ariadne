//! Trajectory DTOs for the tests, written as the daemon sends them.

use ariadne_api::trajectory::{TrajectorySnapshotDto, TrajectoryUpdateDto};
use serde_json::{Value, json};

const AT: &str = "2026-11-27T10:00:00Z";

/// Put the fields of `extra` over those of `base`.
fn merged(mut base: Value, extra: Value) -> Value {
    if let (Some(base), Value::Object(extra)) = (base.as_object_mut(), extra) {
        base.extend(extra);
    }
    base
}

/// A step of `kind`, with the fields `extra` gives over plain ones.
pub(super) fn step(kind: &str, id: &str, turn_id: &str, extra: Value) -> Value {
    let common =
        json!({"type": kind, "id": id, "turn_id": turn_id, "lane": "main", "created_at": AT});
    let own = match kind {
        "message" | "thought" => json!({"content": [], "streaming": false, "message_id": null}),
        "tool_call" => json!({
            "tool_call_id": id, "title": "Run", "kind": "execute", "status": "completed",
            "subject": null, "locations": [], "preview": null, "diff_stat": null,
            "started_at": AT, "ended_at": null, "permission": null, "detail_rev": 0
        }),
        "permission" => json!({
            "tool_call_id": "call", "tool": {"title": "Bash", "kind": "execute", "subject": "cargo test"},
            "question": "Allow Bash?", "state": "pending", "answer": null, "ai": null,
            "learned": null, "subagent": null,
            "options": [{"id": "yes", "name": "Allow", "kind": "allow_once"},
                        {"id": "no", "name": "Reject", "kind": "reject_once"}]
        }),
        "question" => json!({
            "request_id": id, "hint": "Base", "question": "Which base?", "options": [],
            "recommended": null, "recommendation_reason": null, "state": "open", "answer": null
        }),
        "note" => json!({"origin": "session_start", "severity": "info", "text": "note"}),
        "error" => json!({"text": "boom", "diagnosis": null}),
        "subagent" => json!({
            "subagent_id": id, "title": "Explore", "task": null, "state": "completed",
            "can_cancel": false, "started_at": AT, "ended_at": AT, "activity": activity()
        }),
        "compaction" => {
            json!({"compaction_id": id, "status": "completed", "summary": [], "error": null})
        }
        other => panic!("no fixture for a {other} step"),
    };
    merged(merged(common, own), extra)
}

fn activity() -> Value {
    json!({"steps": 0, "tool_calls": 0, "files_changed": 0, "failed": 0,
           "cancelled": 0, "thoughts": 0, "subagents": 0})
}

/// A turn in `state`, with the fields `extra` gives over plain ones.
pub(super) fn turn(id: &str, state: &str, extra: Value) -> Value {
    merged(
        json!({
            "id": id, "lane": "main",
            "prompt": {"id": id, "source": "console", "prompt_kind": null,
                       "content": [{"type": "text", "text": format!("prompt {id}")}]},
            "state": state, "stop_reason": null, "started_at": AT,
            "ended_at": if state == "running" { Value::Null } else { json!(AT) },
            "activity": activity(), "steps": [], "earlier_steps": 0,
            "attention": [], "attention_more": 0, "answer": []
        }),
        extra,
    )
}

pub(super) fn snapshot(rev: u64, turns: Vec<Value>) -> TrajectorySnapshotDto {
    serde_json::from_value(json!({
        "rev": rev,
        "session": {
            "status": "running", "attention_reason": null, "running_turn_id": null,
            "title": null, "mode": null, "config_options": [], "context": null,
            "usage": {"input_tokens": 0, "cached_input_tokens": 0, "output_tokens": 0},
            "cost": null, "commands": []
        },
        "turns": turns, "has_older": false, "subagents": [], "plans": []
    }))
    .expect("a snapshot fixture")
}

pub(super) fn update(rev: u64, ops: Value) -> TrajectoryUpdateDto {
    serde_json::from_value(json!({"rev": rev, "ops": ops})).expect("an update fixture")
}
