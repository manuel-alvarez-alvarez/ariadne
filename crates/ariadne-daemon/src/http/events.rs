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
    // running-mapped event on a live session clears it — going idle is
    // exactly when a permission prompt or a question is waiting, so
    // idle/exit must leave the flag be, and a stray event on an ended
    // session must not wipe the reason it ended needing attention.
    if let Some(reason) = attention_for_event(&req.kind, &req.payload) {
        state
            .store
            .set_session_attention(&session.id, reason)
            .await?;
    } else if session.status().is_live() && status == Some(ariadne_core::SessionStatus::Running) {
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
fn attention_for_event(
    kind: &str,
    payload: &serde_json::Value,
) -> Option<ariadne_core::AttentionReason> {
    use ariadne_core::AttentionReason as A;
    match kind {
        // OpenCode reports a failed turn (API error, aborted tool run) here;
        // the session itself stays alive, so only attention is raised.
        "session.error" => Some(A::AgentError),
        // Claude Code's Notification hook: the only place a permission
        // prompt or a pending question surfaces (the session just looks
        // idle otherwise).
        "notification" => attention_for_notification(payload),
        _ => None,
    }
}

/// Classify a Claude Code `Notification` hook payload.
///
/// `notification_type` is the reliable discriminator (it is also what the
/// hook matcher filters on); the message text is a fallback for CLI versions
/// that only send `message`. Both matches are deliberately narrow: an
/// unrecognized notification is recorded as an event and nothing more —
/// flagging a working agent as blocked is worse than missing a prompt.
fn attention_for_notification(
    payload: &serde_json::Value,
) -> Option<ariadne_core::AttentionReason> {
    use ariadne_core::AttentionReason as A;
    if let Some(kind) = payload.get("notification_type").and_then(|v| v.as_str()) {
        return match kind {
            "permission_prompt" | "worker_permission_prompt" => Some(A::WaitingPermission),
            "idle_prompt" | "agent_needs_input" => Some(A::WaitingInput),
            _ => None,
        };
    }
    let message = payload.get("message").and_then(|v| v.as_str())?;
    if message.contains("needs your permission") || message.contains("needs permission for") {
        Some(A::WaitingPermission)
    } else if message.contains("waiting for your input") || message.contains("needs your input") {
        Some(A::WaitingInput)
    } else {
        None
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
            attention_for_event("session.error", &json!({})),
            Some(AttentionReason::AgentError)
        );
        assert_eq!(status_for_event("session.error"), None);
    }

    #[test]
    fn ordinary_events_raise_no_attention() {
        for event in ["session_start", "post_tool_use", "stop", "session.idle"] {
            assert_eq!(attention_for_event(event, &json!({})), None, "{event}");
        }
    }

    /// A real Claude Code 2.1.235 Notification payload for a tool that is
    /// waiting on the permission dialog.
    fn permission_notification() -> serde_json::Value {
        json!({
            "session_id": "5f3b1c8e-1234-4a2b-9d0e-0123456789ab",
            "transcript_path": "/Users/me/.claude/projects/-tmp-wt/5f3b1c8e.jsonl",
            "cwd": "/tmp/wt",
            "permission_mode": "default",
            "hook_event_name": "Notification",
            "message": "Claude needs your permission to use Bash",
            "title": "Claude Code",
            "notification_type": "permission_prompt",
        })
    }

    /// The same hook after the prompt input sat idle past the threshold.
    fn idle_notification() -> serde_json::Value {
        json!({
            "session_id": "5f3b1c8e-1234-4a2b-9d0e-0123456789ab",
            "transcript_path": "/Users/me/.claude/projects/-tmp-wt/5f3b1c8e.jsonl",
            "cwd": "/tmp/wt",
            "permission_mode": "default",
            "hook_event_name": "Notification",
            "message": "Claude is waiting for your input",
            "title": "Claude Code",
            "notification_type": "idle_prompt",
        })
    }

    #[test]
    fn a_permission_notification_means_the_agent_is_blocked_on_the_user() {
        assert_eq!(
            attention_for_event("notification", &permission_notification()),
            Some(AttentionReason::WaitingPermission)
        );
        // A worker of an agent fleet asking for permission counts too.
        assert_eq!(
            attention_for_event(
                "notification",
                &json!({
                    "hook_event_name": "Notification",
                    "message": "agent_7 needs permission for Bash",
                    "notification_type": "worker_permission_prompt",
                })
            ),
            Some(AttentionReason::WaitingPermission)
        );
    }

    #[test]
    fn an_idle_notification_means_the_agent_is_waiting_for_an_answer() {
        assert_eq!(
            attention_for_event("notification", &idle_notification()),
            Some(AttentionReason::WaitingInput)
        );
        assert_eq!(
            attention_for_event(
                "notification",
                &json!({
                    "hook_event_name": "Notification",
                    "message": "docs-writer needs your input: pick a heading",
                    "notification_type": "agent_needs_input",
                })
            ),
            Some(AttentionReason::WaitingInput)
        );
    }

    #[test]
    fn unrecognized_notifications_raise_nothing() {
        for payload in [
            json!({"hook_event_name": "Notification", "notification_type": "auth_success",
                   "message": "Logged in as me@example.com"}),
            json!({"hook_event_name": "Notification", "notification_type": "agent_completed",
                   "message": "docs-writer finished"}),
            json!({"hook_event_name": "Notification", "message": "Task completed"}),
            json!({"hook_event_name": "Notification"}),
        ] {
            assert_eq!(
                attention_for_event("notification", &payload),
                None,
                "{payload}"
            );
        }
    }

    /// Older CLIs send only `message`; the text is the fallback discriminator.
    #[test]
    fn notifications_without_a_type_fall_back_to_the_message() {
        for (message, expected) in [
            (
                "Claude needs your permission to use Bash",
                AttentionReason::WaitingPermission,
            ),
            (
                "Claude is waiting for your input",
                AttentionReason::WaitingInput,
            ),
        ] {
            let payload = json!({"hook_event_name": "Notification", "message": message});
            assert_eq!(
                attention_for_event("notification", &payload),
                Some(expected),
                "{message}"
            );
        }
    }

    /// A blocked agent is not a working one: the notification itself must
    /// leave the lifecycle status alone (mapping it to `Running` would clear
    /// the very attention it raises), and the next real work event clears it.
    #[test]
    fn a_notification_implies_no_status_change() {
        assert_eq!(status_for_event("notification"), None);
        for event in ["pre_tool_use", "user_prompt_submit"] {
            assert_eq!(
                status_for_event(event),
                Some(SessionStatus::Running),
                "{event}"
            );
            assert_eq!(attention_for_event(event, &json!({})), None, "{event}");
        }
    }
}
