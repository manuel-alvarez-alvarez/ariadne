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
    //
    // Raising it asks one thing more: whether anybody is still waiting on
    // this agent. A reviewer's approval dialog after it has voted, or a
    // planner's after the goal left planning, is nobody's to answer — the
    // event is recorded and the status still follows it, only the flag is
    // withheld. A prompt is asked the same about the session itself: a
    // dialog belongs to a pane, so a late `permission.asked` from a session
    // already recorded as ended raises nothing (and does not overwrite the
    // reason it ended with).
    if let Some(reason) = attention_for_event(&req.kind, &req.payload) {
        let answerable = !reason.is_prompt() || session.status().is_live();
        if answerable && crate::attention::work_is_active(&state.store, &session).await {
            state
                .store
                .set_session_attention(&session.id, reason)
                .await?;
        }
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
        // `sessionID` comes before the bare `id`: opencode's approval events
        // carry both, and there `id` is the permission's, not the session's.
        AgentKind::Opencode => &[&["info", "id"], &["sessionID"], &["session", "id"], &["id"]],
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
        | "tool.execute.before"
        // An answered approval hands control back to the agent, whichever way
        // it went: allowed it runs the call, rejected it gets the refusal as
        // the tool result and carries on. This is also what takes the
        // attention flag back down — see the clear in `ingest`.
        | "permission.replied"
        | "question.replied"
        | "question.rejected" => Some(S::Running),
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
        // Codex's PermissionRequest hook fires as it puts the approval dialog
        // up, after `pre_tool_use` for the same call. It runs before the
        // answer is known, so it marks the wait and not its outcome:
        // approved, `post_tool_use` follows and clears this; denied, nothing
        // follows until the user says what to do instead — which is still a
        // session waiting on them.
        "permission_request" => Some(A::WaitingPermission),
        // OpenCode puts its approval dialog on the event bus: `permission.asked`
        // while it is up, `permission.replied` when the user answers. Nothing
        // else distinguishes the wait — the session keeps emitting
        // `session.updated` throughout, exactly as it does while working.
        // `permission.updated` is the same ask under the name the generated
        // SDK types still use; the 1.18.15 runtime only emits `.asked`.
        "permission.asked" | "permission.updated" => Some(A::WaitingPermission),
        // Same shape for the `question` tool, which asks the user directly
        // instead of going through the approval layer.
        "question.asked" => Some(A::WaitingInput),
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

    /// Nothing may be declared and then dropped on the floor: every event
    /// Ariadne asks codex for has to move either the status or the attention,
    /// or the flag pair is paying a trust prompt for nothing.
    #[test]
    fn every_declared_codex_event_is_acted_on() {
        for event in ariadne_core::codex_hooks::EVENTS {
            let kind = ariadne_core::codex_hooks::event_kind(event);
            assert!(
                status_for_event(&kind).is_some()
                    || attention_for_event(&kind, &permission_request()).is_some(),
                "{event} is declared but ingested as a no-op"
            );
        }
    }

    /// A PermissionRequest payload captured from codex 0.147 sitting on an
    /// approval dialog: the hook runs while the dialog is up, so it carries
    /// the call it is about and no answer.
    fn permission_request() -> serde_json::Value {
        json!({
            "cwd": "/tmp/worktree",
            "hook_event_name": "PermissionRequest",
            "model": "gpt-5.6-sol",
            "permission_mode": "default",
            "session_id": "01a01a24-e62e-71c1-ba23-96c62f6acee1",
            "tool_input": {"command": "touch /tmp/probe"},
            "tool_name": "Bash",
            "transcript_path": "/tmp/codex/rollout.jsonl",
            "turn_id": "01a01a25-610d-7d50-927b-695c137814e7",
        })
    }

    /// The whole point of declaring the hook: a codex session sitting on an
    /// approval dialog is otherwise indistinguishable from one thinking.
    #[test]
    fn a_permission_request_means_the_agent_is_blocked_on_the_user() {
        assert_eq!(
            attention_for_event("permission_request", &permission_request()),
            Some(AttentionReason::WaitingPermission)
        );
    }

    /// ...and it must not read as liveness. `PreToolUse` fires just before
    /// it for the same call, so a `Running` mapping here would clear the
    /// attention the event itself raised.
    #[test]
    fn a_permission_request_implies_no_status_change() {
        assert_eq!(status_for_event("permission_request"), None);
    }

    /// What takes the flag back down. Approved, codex runs the call and
    /// reports `post_tool_use`; denied, no tool event follows at all and the
    /// wait stands until the user's next prompt — which is the truth of it,
    /// a denied command is a session still waiting to be told what to do.
    #[test]
    fn the_events_after_an_approval_clear_the_wait() {
        for event in ["post_tool_use", "user_prompt_submit"] {
            assert_eq!(
                status_for_event(event),
                Some(SessionStatus::Running),
                "{event}"
            );
            assert_eq!(attention_for_event(event, &json!({})), None, "{event}");
        }
    }

    /// A `permission.asked` payload captured off the plugin's own `event`
    /// hook, running under opencode 1.18.15 with `permission.bash = "ask"`.
    fn permission_asked() -> serde_json::Value {
        json!({
            "id": "per_01a3575b4001aOsIrUVWB44A4e",
            "sessionID": "ses_fe5cb9641ffeQPvwaIKtSsLAqP",
            "permission": "bash",
            "patterns": ["echo hello-from-bash"],
            "metadata": {"command": "echo hello-from-bash"},
            "always": ["echo *"],
            "tool": {
                "messageID": "msg_01a346a620011crwCE4oJgZDqr",
                "callID": "call_vt3e3umm",
            },
        })
    }

    /// The reply that followed it in the same run.
    fn permission_replied() -> serde_json::Value {
        json!({
            "sessionID": "ses_fe5cb9641ffeQPvwaIKtSsLAqP",
            "requestID": "per_01a3575b4001aOsIrUVWB44A4e",
            "reply": "reject",
        })
    }

    /// The whole point of forwarding them: an opencode session sitting on an
    /// approval dialog emits nothing else that a working one does not.
    #[test]
    fn a_permission_ask_means_the_agent_is_blocked_on_the_user() {
        assert_eq!(
            attention_for_event("permission.asked", &permission_asked()),
            Some(AttentionReason::WaitingPermission)
        );
        // The name the generated SDK types give the same ask.
        assert_eq!(
            attention_for_event("permission.updated", &permission_asked()),
            Some(AttentionReason::WaitingPermission)
        );
        // A question is a wait for an answer, not for an approval.
        assert_eq!(
            attention_for_event("question.asked", &json!({"sessionID": "ses_x"})),
            Some(AttentionReason::WaitingInput)
        );
    }

    /// ...and none of them may read as liveness: a `Running` mapping would
    /// clear the very attention the event raises.
    #[test]
    fn an_ask_implies_no_status_change() {
        for kind in ["permission.asked", "permission.updated", "question.asked"] {
            assert_eq!(status_for_event(kind), None, "{kind}");
        }
    }

    /// What takes the flag back down. Either way the user answered, so the
    /// agent has control again — allowed it runs the call, rejected it gets
    /// the refusal as the tool result and keeps going.
    #[test]
    fn an_answered_ask_resumes_the_session() {
        for (kind, payload) in [
            ("permission.replied", permission_replied()),
            ("question.replied", json!({"sessionID": "ses_x"})),
            ("question.rejected", json!({"sessionID": "ses_x"})),
        ] {
            assert_eq!(
                status_for_event(kind),
                Some(SessionStatus::Running),
                "{kind}"
            );
            assert_eq!(attention_for_event(kind, &payload), None, "{kind}");
        }
    }

    /// An approval event carries two ids and the wrong one is first: `id` is
    /// the permission's. Reading it as the session's would pin the whole
    /// session to `per_…` and break every resume after it.
    #[test]
    fn an_approval_event_yields_the_session_id_and_not_the_permission_id() {
        assert_eq!(
            extract_internal_id(AgentKind::Opencode, &permission_asked()).as_deref(),
            Some("ses_fe5cb9641ffeQPvwaIKtSsLAqP")
        );
        assert_eq!(
            extract_internal_id(AgentKind::Opencode, &permission_replied()).as_deref(),
            Some("ses_fe5cb9641ffeQPvwaIKtSsLAqP")
        );
    }

    /// The mirror of the codex check: every event the plugin is told to
    /// forward has to earn the round trip.
    #[test]
    fn every_forwarded_opencode_event_is_acted_on() {
        for kind in crate::opencode_plugin::declared_events() {
            let acted_on = status_for_event(&kind).is_some()
                || attention_for_event(&kind, &permission_asked()).is_some()
                // Forwarded for its payload alone: `info.id` is where the
                // internal session id comes from, and `session.updated`
                // deliberately maps to no status (it keeps firing after
                // idle, so `Running` would lie).
                || kind == "session.updated";
            assert!(acted_on, "{kind} is forwarded but ingested as a no-op");
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

    /// A Notification payload captured from Claude Code 2.1.235 sitting on a
    /// permission dialog.
    fn permission_notification() -> serde_json::Value {
        json!({
            "cwd": "/tmp/wt",
            "hook_event_name": "Notification",
            "message": "Claude needs your permission",
            "notification_type": "permission_prompt",
            "prompt_id": "c58b5911-1a83-4548-8d70-ba2e83ade968",
            "session_id": "5cf3f43d-6d22-42eb-8e44-8213bee346cd",
            "transcript_path": "/Users/me/.claude/projects/-tmp-wt/5cf3f43d.jsonl",
        })
    }

    /// The same hook after the prompt input sat idle past the threshold.
    fn idle_notification() -> serde_json::Value {
        json!({
            "cwd": "/tmp/wt",
            "hook_event_name": "Notification",
            "message": "Claude is waiting for your input",
            "notification_type": "idle_prompt",
            "prompt_id": "c58b5911-1a83-4548-8d70-ba2e83ade968",
            "session_id": "5cf3f43d-6d22-42eb-8e44-8213bee346cd",
            "transcript_path": "/Users/me/.claude/projects/-tmp-wt/5cf3f43d.jsonl",
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
