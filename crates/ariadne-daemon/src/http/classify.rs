//! What an agent event means: the internal session id it carries, the
//! lifecycle status it implies, whether it says a human has to act, and what
//! it reports having spent.
//!
//! Three CLIs report through [`super::events::ingest`] in three vocabularies,
//! and these tables are where each one is read. An event that matches nothing
//! is still recorded — it simply moves neither flag.

use ariadne_core::TokenUsage;

/// Where each agent kind carries its internal session id in event payloads.
pub(super) fn extract_internal_id(
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
pub(super) fn status_for_event(kind: &str) -> Option<ariadne_core::SessionStatus> {
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
pub(super) fn attention_for_event(
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
        // Claude Code's own way of asking, and the one raise that rides on a
        // running-mapped event: [`QUESTION_TOOL`] puts its choices in the pane
        // and blocks the call until somebody picks one, and the `pre_tool_use`
        // announcing that call is the first — and, for the daemon, the only
        // trustworthy — word of it. What the dialog itself fires is a
        // `permission_prompt` notification, indistinguishable from an approval
        // and half a minute late; the tool name is what says a person, not a
        // policy, is being asked. See [`question_for_event`] for what keeps
        // the flag up afterwards.
        "pre_tool_use" if is_a_question(payload) => Some(A::WaitingInput),
        // Claude Code's Notification hook: the only place a permission
        // prompt or a pending question surfaces (the session just looks
        // idle otherwise).
        "notification" => attention_for_notification(payload),
        _ => None,
    }
}

/// The Claude Code tool that puts a question to the user: it renders its
/// choices in the pane and does not return until one is picked.
pub(super) const QUESTION_TOOL: &str = "AskUserQuestion";

/// What an event does to a question standing in the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Question {
    /// The call was announced: from here on somebody has to answer.
    Asked,
    /// Nothing is waiting any more, however the question left the screen.
    Answered,
}

/// What this event says about a [`QUESTION_TOOL`] call (None = nothing).
///
/// A question is not a moment but a stretch of time, and the events of it
/// arrive interleaved with those of everything else the same turn is doing:
/// Claude announces the whole batch of tool calls at once, so `pre_tool_use`
/// and `post_tool_use` for the other calls keep coming while the choices sit
/// on the screen unanswered. Which of them ends the question is therefore a
/// question about the tool name and not about the kind, and the answer is
/// what [`super::events::ingest`] holds the flag against.
///
/// Three things end it. Its own `post_tool_use` carries the answer that was
/// picked. A `user_prompt_submit` is the user typing at the pane instead,
/// which dismisses the dialog. And a `stop` is the turn ending — Esc on the
/// choices does exactly that — after which there is nothing left on screen to
/// answer.
pub(super) fn question_for_event(kind: &str, payload: &serde_json::Value) -> Option<Question> {
    match kind {
        "pre_tool_use" if is_a_question(payload) => Some(Question::Asked),
        "post_tool_use" if is_a_question(payload) => Some(Question::Answered),
        "user_prompt_submit" | "stop" => Some(Question::Answered),
        _ => None,
    }
}

/// Whether a Claude Code tool event is about the question tool.
fn is_a_question(payload: &serde_json::Value) -> bool {
    payload.get("tool_name").and_then(|v| v.as_str()) == Some(QUESTION_TOOL)
}

/// The event kinds that begin a turn, in the two hook vocabularies that
/// carry [`last_assistant_message`].
///
/// The token usage an event reports, as `(source, totals)` — the transcript
/// the figures were read from, and its cumulative totals.
///
/// Any event of any agent kind may carry an `ariadne_usage`, and most carry
/// none: reporting is the job of the hooks and the plugin that read the
/// transcripts, and an event without one simply has no news. What is refused
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

/// Classify a Claude Code `Notification` hook payload.
///
/// `notification_type` is the reliable discriminator (it is also what the
/// hook matcher filters on); the message text is a fallback for CLI versions
/// that only send `message`. Both matches are deliberately narrow: an
/// unrecognized notification is recorded as an event and nothing more —
/// flagging a working agent as blocked is worse than missing a prompt.
///
/// Narrow enough to leave `idle_prompt` out, which is the notification the
/// hook fires a minute after *any* turn ends. Under Ariadne an agent that
/// ended its turn with the work still in front of it is waiting for the
/// daemon's nudge, not for a person: reading that minute as "waiting for
/// input" put up a flag that said a human was needed, and took the session
/// out of the quiet watchdog — which skips one that is waiting on a person —
/// so it was never nudged, stalled or relaunched. The message text `waiting
/// for your input` is the same notification on an older CLI, and goes with it.
fn attention_for_notification(
    payload: &serde_json::Value,
) -> Option<ariadne_core::AttentionReason> {
    use ariadne_core::AttentionReason as A;
    if let Some(kind) = payload.get("notification_type").and_then(|v| v.as_str()) {
        return match kind {
            "permission_prompt" | "worker_permission_prompt" => Some(A::WaitingPermission),
            // A subagent asking a question of its own: somebody has to answer.
            "agent_needs_input" => Some(A::WaitingInput),
            _ => None,
        };
    }
    let message = payload.get("message").and_then(|v| v.as_str())?;
    if message.contains("needs your permission") || message.contains("needs permission for") {
        Some(A::WaitingPermission)
    } else if message.contains("needs your input") {
        Some(A::WaitingInput)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        QUESTION_TOOL, Question, attention_for_event, extract_internal_id,
        question_for_event, status_for_event, usage_for_event,
    };

    use ariadne_core::{AgentKind, AttentionReason, SessionStatus, TokenUsage};
    use serde_json::json;

    /// A Codex 0.147 `PermissionRequest` payload, captured while the approval
    /// dialog was up: it carries the call it is about and no answer.
    fn codex_permission_request() -> serde_json::Value {
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

    /// An opencode 1.18.15 `permission.asked`, off the plugin's own `event`
    /// hook with `permission.bash = "ask"`.
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

    /// A Claude Code 2.1.235 `Notification`, captured on a permission dialog.
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

    /// The id an event carries is the *session's*, whatever else is in the
    /// payload. An opencode approval carries two, and the wrong one is first:
    /// reading `id` would pin the session to `per_…` and break every resume
    /// after it.
    #[test]
    fn an_events_internal_id_is_its_sessions_and_nothing_elses() {
        assert_eq!(
            extract_internal_id(AgentKind::Codex, &codex_permission_request()).as_deref(),
            Some("01a01a24-e62e-71c1-ba23-96c62f6acee1")
        );
        for payload in [
            &permission_asked(),
            &json!({"sessionID": "ses_x", "id": "per_y"}),
        ] {
            assert!(
                extract_internal_id(AgentKind::Opencode, payload)
                    .is_some_and(|id| id.starts_with("ses_")),
                "{payload}"
            );
        }
        // No id, and an empty one, both leave it unknown.
        for payload in [
            json!({"hook_event_name": "Stop"}),
            json!({"session_id": ""}),
        ] {
            assert_eq!(extract_internal_id(AgentKind::Codex, &payload), None);
        }
    }

    /// Nothing may be declared and then dropped on the floor: every event
    /// Ariadne asks an agent for has to move either the status or the
    /// attention, or the hooks and the plugin are paying a trust prompt for
    /// nothing.
    #[test]
    fn every_declared_event_is_acted_on() {
        let codex = ariadne_core::codex_hooks::EVENTS
            .iter()
            .map(|e| ariadne_core::codex_hooks::event_kind(e));
        for kind in codex.chain(crate::opencode_plugin::declared_events()) {
            let acted_on = status_for_event(&kind).is_some()
                || attention_for_event(&kind, &permission_asked()).is_some()
                // Forwarded for its payload alone: `info.id` is where the
                // internal session id comes from, and `session.updated`
                // deliberately maps to no status (it keeps firing after
                // idle, so `Running` would lie).
                || kind == "session.updated";
            assert!(acted_on, "{kind} is declared but ingested as a no-op");
        }
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
            ("session.created", Running),
            ("stop", Idle),
            ("turn_complete", Idle),
            ("session.idle", Idle),
            ("session_end", Exited),
            ("session.deleted", Exited),
        ] {
            assert_eq!(status_for_event(event), Some(expected), "{event}");
            assert_eq!(attention_for_event(event, &json!({})), None, "{event}");
        }
    }

    /// Every way an agent says it is blocked on the user — and none of them
    /// may read as liveness, since a `Running` mapping would clear the very
    /// attention the event raises. Without these the session is
    /// indistinguishable from one that is thinking.
    #[test]
    fn a_wait_on_the_user_is_raised_and_never_reads_as_liveness() {
        use AttentionReason::*;
        for (kind, payload, expected) in [
            (
                "permission_request",
                codex_permission_request(),
                WaitingPermission,
            ),
            ("permission.asked", permission_asked(), WaitingPermission),
            // The name the generated opencode SDK types give the same ask.
            ("permission.updated", permission_asked(), WaitingPermission),
            (
                "question.asked",
                json!({"sessionID": "ses_x"}),
                WaitingInput,
            ),
            // A failed opencode turn: the session lives, so only the flag goes up.
            ("session.error", json!({}), AgentError),
            ("notification", permission_notification(), WaitingPermission),
            (
                "notification",
                json!({"hook_event_name": "Notification",
                       "message": "agent_7 needs permission for Bash",
                       "notification_type": "worker_permission_prompt"}),
                WaitingPermission,
            ),
            (
                "notification",
                json!({"hook_event_name": "Notification",
                       "message": "docs-writer needs your input: pick a heading",
                       "notification_type": "agent_needs_input"}),
                WaitingInput,
            ),
        ] {
            assert_eq!(
                attention_for_event(kind, &payload),
                Some(expected),
                "{kind}"
            );
            assert_eq!(status_for_event(kind), None, "{kind}");
        }
    }

    /// What takes the flag back down. Either way the user answered, so the
    /// agent has control again — allowed it runs the call, rejected it gets
    /// the refusal as the tool result and keeps going. A denied codex command
    /// reports nothing at all until the next prompt, which is the truth of it:
    /// a session still waiting to be told what to do.
    #[test]
    fn an_answered_ask_hands_control_back_to_the_agent() {
        for (kind, payload) in [
            (
                "permission.replied",
                json!({"sessionID": "ses_x", "reply": "reject"}),
            ),
            ("question.replied", json!({"sessionID": "ses_x"})),
            ("question.rejected", json!({"sessionID": "ses_x"})),
            ("post_tool_use", json!({})),
            ("user_prompt_submit", json!({})),
        ] {
            assert_eq!(
                status_for_event(kind),
                Some(SessionStatus::Running),
                "{kind}"
            );
            assert_eq!(attention_for_event(kind, &payload), None, "{kind}");
        }
    }

    /// Claude Code's question tool is the one wait that arrives on a
    /// running-mapped event, and the tool name is the whole of the signal: the
    /// same `pre_tool_use` for anything else is an agent at work.
    #[test]
    fn a_question_put_to_the_user_is_read_off_the_tool_it_calls() {
        let call = |tool: &str| json!({"hook_event_name": "PreToolUse", "tool_name": tool});
        assert_eq!(
            attention_for_event("pre_tool_use", &call(QUESTION_TOOL)),
            Some(AttentionReason::WaitingInput)
        );
        for payload in [call("Bash"), call("Task"), json!({})] {
            assert_eq!(
                attention_for_event("pre_tool_use", &payload),
                None,
                "{payload}"
            );
        }
        // Its own `post_tool_use` carries the answer and raises nothing.
        assert_eq!(
            attention_for_event("post_tool_use", &call(QUESTION_TOOL)),
            None
        );
    }

    /// What opens a question and what closes it. The events of the other tool
    /// calls of the same turn keep arriving throughout and settle nothing:
    /// that is exactly what the pending question has to be held against.
    #[test]
    fn a_question_stands_from_its_call_until_something_answers_it() {
        let call = |tool: &str| json!({"tool_name": tool});
        for (kind, payload, expected) in [
            ("pre_tool_use", call(QUESTION_TOOL), Some(Question::Asked)),
            (
                "post_tool_use",
                call(QUESTION_TOOL),
                Some(Question::Answered),
            ),
            // Typed at the pane instead of answered, and Esc on the choices,
            // which ends the turn.
            (
                "user_prompt_submit",
                json!({"prompt": "go on"}),
                Some(Question::Answered),
            ),
            ("stop", json!({}), Some(Question::Answered)),
            // The turn's other calls, which say nothing about the question.
            ("pre_tool_use", call("Bash"), None),
            ("post_tool_use", call("Bash"), None),
            (
                "notification",
                json!({"notification_type": "permission_prompt"}),
                None,
            ),
            ("session_start", json!({}), None),
        ] {
            assert_eq!(
                question_for_event(kind, &payload),
                expected,
                "{kind} {payload}"
            );
        }
    }

    /// Both notification matches are deliberately narrow: flagging a working
    /// agent as blocked is worse than missing a prompt.
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

    /// An agent sitting at its prompt is not an agent waiting on a person.
    ///
    /// Claude's hook fires `idle_prompt` a minute after any turn ends, and
    /// under Ariadne what such an agent waits for is the daemon's nudge: a
    /// flag here says a human is needed, and takes the session out of the
    /// watchdog that would have nudged, stalled and relaunched it. The message
    /// text is the same notification on an older CLI.
    #[test]
    fn an_agent_idle_at_its_prompt_is_waiting_on_nobody() {
        for payload in [
            json!({"hook_event_name": "Notification", "notification_type": "idle_prompt",
                   "message": "Claude is waiting for your input"}),
            json!({"hook_event_name": "Notification",
                   "message": "Claude is waiting for your input"}),
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
                "docs-writer needs your input: pick a heading",
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

    /// The contract the hooks and the plugin report under, read whole.
    #[test]
    fn an_event_reports_the_totals_of_the_transcript_it_names() {
        let payload = json!({
            "hook_event_name": "Stop",
            "ariadne_usage": {
                "source": "/Users/me/.claude/projects/-tmp-wt/5cf3f43d.jsonl",
                "input_tokens": 100,
                "cached_input_tokens": 80,
                "output_tokens": 10,
            },
        });
        assert_eq!(
            usage_for_event(&payload),
            Some((
                "/Users/me/.claude/projects/-tmp-wt/5cf3f43d.jsonl".to_string(),
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
            json!({"hook_event_name": "Stop"}),
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
}
