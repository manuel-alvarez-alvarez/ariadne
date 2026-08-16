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
    if session.status().is_live()
        && let Some(status) = status_for_event(&req.kind)
        && status != session.status()
    {
        state.store.set_session_status(&session.id, status).await?;
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

#[cfg(test)]
mod tests {
    use super::{extract_internal_id, status_for_event};

    use ariadne_core::{AgentKind, SessionStatus};
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
}
