//! Agent-event endpoints: public listing plus the internal ingestion sink for
//! hooks/plugins.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;

use ariadne_api::Page;
use ariadne_api::events::{AgentEventDto, EventListQuery, IngestEventRequest};
use ariadne_store::{EventFilter, NewAgentEvent};

use super::AppState;
use super::convert::event_dto;
use super::error::ApiResult;

/// List agent events (poll with `after` for tailing).
#[utoipa::path(get, path = "/v1/events", tag = "events",
    params(EventListQuery, Page),
    responses((status = 200, body = [AgentEventDto])))]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<EventListQuery>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<AgentEventDto>>> {
    let events = state
        .store
        .list_events(EventFilter {
            session_id: q.session,
            task_id: q.task,
            limit: page.limit(),
            after: page.after,
        })
        .await?;
    Ok(Json(events.into_iter().map(event_dto).collect()))
}

/// Ingest an event reported by an agent hook (`ariadne agent-event`).
/// Not part of the public OpenAPI surface.
pub async fn ingest(
    State(state): State<AppState>,
    Json(req): Json<IngestEventRequest>,
) -> ApiResult<StatusCode> {
    // The session must exist; its task link is copied onto the event.
    let session = state.store.get_session(&req.session_id).await?;
    state
        .store
        .create_event(NewAgentEvent {
            session_id: Some(session.id.clone()),
            task_id: session.task_id.clone(),
            agent_kind: Some(req.agent_kind),
            kind: req.kind.clone(),
            payload: req.payload.clone(),
        })
        .await?;

    // Capture the agent-internal session id as soon as an event carries it.
    if session.internal_session_id.is_none()
        && let Some(internal) = extract_internal_id(req.agent_kind, &req.payload)
    {
        tracing::info!(session = %session.id, internal, "captured internal session id");
        state
            .store
            .set_session_internal_id(&session.id, &internal)
            .await?;
    }

    // Track liveness from lifecycle events (never resurrect ended sessions).
    let status = status_for_event(&req.kind);
    if session.status().is_live()
        && let Some(status) = status
        && status != session.status()
    {
        state.store.set_session_status(&session.id, status).await?;
    }

    // Attention follows the event too: an agent that reported an error needs
    // the user, and one that is working again does not. Only a
    // running-mapped event clears — going idle is exactly when a permission
    // prompt or a question is waiting, so idle/exit must leave the flag be.
    if let Some(reason) = attention_for_event(&req.kind) {
        state
            .store
            .set_session_attention(&session.id, reason)
            .await?;
    } else if status == Some(ariadne_core::SessionStatus::Running) {
        state.store.clear_session_attention(&session.id).await?;
    }

    state.store.touch_session(&session.id).await?;
    state.notify_scheduler_session(&session.id).await;
    Ok(StatusCode::ACCEPTED)
}

/// Where each agent kind carries its internal session id in event payloads.
fn extract_internal_id(
    kind: ariadne_core::AgentKind,
    payload: &serde_json::Value,
) -> Option<String> {
    use ariadne_core::AgentKind;
    let candidates: &[&[&str]] = match kind {
        AgentKind::ClaudeCode | AgentKind::Codex => &[&["session_id"]],
        AgentKind::Opencode => &[&["info", "id"], &["id"], &["sessionID"], &["session", "id"]],
    };
    for path in candidates {
        let value = path.iter().try_fold(payload, |v, k| v.get(k));
        if let Some(s) = value.and_then(|v| v.as_str())
            && !s.is_empty()
        {
            return Some(s.to_string());
        }
    }
    None
}

/// Session status implied by a lifecycle event kind (None = no change).
fn status_for_event(kind: &str) -> Option<ariadne_core::SessionStatus> {
    use ariadne_core::SessionStatus as S;
    match kind {
        // NB: opencode's `session.updated` keeps firing after idle and must
        // not flip the status back to running.
        "session_start"
        | "user_prompt_submit"
        | "post_tool_use"
        | "pre_tool_use"
        | "session.created"
        | "tool.execute.after"
        | "tool.execute.before" => Some(S::Running),
        "stop" | "turn_complete" | "session.idle" => Some(S::Idle),
        "session_end" | "session.deleted" => Some(S::Exited),
        _ => None,
    }
}

/// Attention an event raises on its session (None = leave it alone).
fn attention_for_event(kind: &str) -> Option<ariadne_core::AttentionReason> {
    use ariadne_core::AttentionReason as A;
    match kind {
        // OpenCode reports a failed turn (API error, aborted tool run) here;
        // the session itself stays alive, so only attention is raised.
        "session.error" => Some(A::AgentError),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{attention_for_event, extract_internal_id, status_for_event};

    use ariadne_core::{AgentKind, AttentionReason, SessionStatus};
    use serde_json::json;

    #[test]
    fn codex_session_start_yields_the_internal_id() {
        // Shape of a real Codex 0.147 SessionStart hook payload.
        let payload = json!({
            "session_id": "01a00b36-1234-7890-abcd-ef0123456789",
            "transcript_path": "/tmp/rollout.jsonl",
            "cwd": "/tmp/worktree",
            "hook_event_name": "SessionStart",
            "source": "startup",
        });
        assert_eq!(
            extract_internal_id(AgentKind::Codex, &payload).as_deref(),
            Some("01a00b36-1234-7890-abcd-ef0123456789")
        );
    }

    #[test]
    fn codex_events_without_an_id_yield_nothing() {
        for payload in [
            json!({"hook_event_name": "Stop"}),
            json!({"session_id": ""}),
        ] {
            assert_eq!(extract_internal_id(AgentKind::Codex, &payload), None);
        }
    }

    #[test]
    fn every_codex_hook_event_maps_to_a_status() {
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
        }
    }

    #[test]
    fn session_error_raises_attention_without_touching_the_status() {
        assert_eq!(
            attention_for_event("session.error"),
            Some(AttentionReason::AgentError)
        );
        assert_eq!(status_for_event("session.error"), None);
    }

    #[test]
    fn ordinary_events_raise_no_attention() {
        for event in ["session_start", "post_tool_use", "stop", "session.idle"] {
            assert_eq!(attention_for_event(event), None, "{event}");
        }
    }
}
