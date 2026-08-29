//! `ariadne agent-event` — invoked by coding-agent hooks to report events.
//!
//! MUST be fail-safe: a dead daemon or malformed payload must never block or
//! crash the agent, so every path exits 0, and the whole of it — reading the
//! transcript for the token counts included — is capped at 2s.

mod usage;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ariadne_api::events::IngestEventRequest;
use ariadne_client::Client;
use ariadne_core::AgentKind;

const SESSION_ENV: &str = "ARIADNE_SESSION_ID";

pub async fn run(kind: AgentKind, json: Option<String>) {
    // Never propagate failures to the calling hook.
    let _ = tokio::time::timeout(Duration::from_secs(2), forward(kind, json)).await;
}

async fn forward(agent_kind: AgentKind, json: Option<String>) {
    let Ok(session_id) = std::env::var(SESSION_ENV) else {
        return; // not spawned by ariadne
    };

    let (event_kind, mut payload) = match agent_kind {
        // Claude Code and Codex share the hook protocol: the payload arrives
        // as JSON on stdin and `hook_event_name` names the event.
        AgentKind::ClaudeCode | AgentKind::Codex => {
            let mut raw = String::new();
            if std::io::stdin().read_to_string(&mut raw).is_err() {
                return;
            }
            parse_hook_event(&raw)
        }
        // OpenCode: the plugin passes {"kind": ..., "payload": ...} as --json.
        AgentKind::Opencode => {
            let value: serde_json::Value =
                serde_json::from_str(json.as_deref().unwrap_or("{}")).unwrap_or_default();
            let event = value
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let payload = value.get("payload").cloned().unwrap_or_default();
            (event, payload)
        }
    };

    attach_usage(agent_kind, &event_kind, &mut payload, &session_id);

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

/// The events that always read the transcript: the turn boundaries, where the
/// counters have just moved.
const TURN_BOUNDARIES: [&str; 3] = ["stop", "session_end", "user_prompt_submit"];

/// The event that reads it only when [`USAGE_INTERVAL`] has passed since the
/// last report. A turn is one long stretch of tool calls, and its figures
/// would otherwise sit at zero until it ends; reading on *every* call instead
/// would re-read a multi-megabyte file dozens of times a turn.
const THROTTLED_USAGE_EVENT: &str = "post_tool_use";

/// How stale the last report has to be before a [`THROTTLED_USAGE_EVENT`]
/// reads the transcript again.
const USAGE_INTERVAL: Duration = Duration::from_secs(10);

/// Add `ariadne_usage` to the payload, when the agent's transcript can be read.
///
/// The counters are cumulative for `source`, the transcript they came from,
/// so the daemon can store the latest report and ignore the order they arrive
/// in. Every failure — an event that reports no usage, no `transcript_path`, a
/// file that is gone or unreadable, a shape that has changed — leaves the
/// payload as it was: usage is a bonus, never a reason for the hook to do
/// anything different.
fn attach_usage(
    agent_kind: AgentKind,
    event: &str,
    payload: &mut serde_json::Value,
    session_id: &str,
) {
    attach_usage_within(
        agent_kind,
        event,
        payload,
        &Throttle::for_session(session_id),
    );
}

fn attach_usage_within(
    agent_kind: AgentKind,
    event: &str,
    payload: &mut serde_json::Value,
    throttle: &Throttle,
) {
    if !throttle.allows(event) {
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
        // OpenCode reports its own usage from the plugin.
        _ => return,
    };
    // The read is what the interval is there to space out, so the stamp marks
    // it whether or not the file had anything to say.
    throttle.record();
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

/// When this session last read its transcript, kept in a file because every
/// hook run is a process of its own that remembers nothing.
///
/// Anything the stamp cannot answer — no file yet, junk in it, a clock that
/// moved, a temp dir that cannot be written — is answered by reporting: a
/// missed throttle costs one read, a missed report costs the figure this is
/// all here to show.
struct Throttle {
    stamp: PathBuf,
    now: SystemTime,
    interval: Duration,
}

impl Throttle {
    fn for_session(session_id: &str) -> Self {
        Self {
            stamp: std::env::temp_dir().join(format!("ariadne-usage-{session_id}")),
            now: SystemTime::now(),
            interval: USAGE_INTERVAL,
        }
    }

    /// Whether `event` may read the transcript now.
    fn allows(&self, event: &str) -> bool {
        if TURN_BOUNDARIES.contains(&event) {
            return true;
        }
        if event != THROTTLED_USAGE_EVENT {
            return false;
        }
        self.last_report()
            .and_then(|last| self.now.duration_since(last).ok())
            .is_none_or(|since| since >= self.interval)
    }

    fn last_report(&self) -> Option<SystemTime> {
        let millis: u64 = std::fs::read_to_string(&self.stamp)
            .ok()?
            .trim()
            .parse()
            .ok()?;
        Some(SystemTime::UNIX_EPOCH + Duration::from_millis(millis))
    }

    fn record(&self) {
        let Ok(age) = self.now.duration_since(SystemTime::UNIX_EPOCH) else {
            return;
        };
        let millis = u64::try_from(age.as_millis()).unwrap_or(u64::MAX);
        let _ = std::fs::write(&self.stamp, millis.to_string());
    }
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
    use std::time::{Duration, SystemTime};

    use ariadne_core::AgentKind;
    use tempfile::TempDir;

    use super::{Throttle, USAGE_INTERVAL, attach_usage_within, parse_hook_event};

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

    fn payload_for(event: &str, transcript: &str) -> serde_json::Value {
        let raw = format!(r#"{{"hook_event_name": "{event}", "transcript_path": "{transcript}"}}"#);
        parse_hook_event(&raw).1
    }

    fn stop_payload(transcript: &str) -> serde_json::Value {
        payload_for("Stop", transcript)
    }

    /// A transcript with something to report, and room for the stamp beside it.
    fn fixture() -> (TempDir, String) {
        let dir = TempDir::new().expect("a temp dir");
        let transcript = dir.path().join("session.jsonl");
        fs::write(&transcript, TRANSCRIPT).expect("write the transcript");
        let source = transcript.to_str().expect("a utf-8 path").to_string();
        (dir, source)
    }

    /// The throttle a hook running `seconds` into the session would build:
    /// every call of one test shares the stamp file, as the hooks of one
    /// session share theirs.
    fn throttle_at(dir: &TempDir, seconds: u64) -> Throttle {
        Throttle {
            stamp: dir.path().join("stamp"),
            now: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds),
            interval: USAGE_INTERVAL,
        }
    }

    #[test]
    fn a_stop_reports_the_usage_its_transcript_holds() {
        let (dir, source) = fixture();

        let mut payload = stop_payload(&source);
        attach_usage_within(
            AgentKind::ClaudeCode,
            "stop",
            &mut payload,
            &throttle_at(&dir, 0),
        );

        assert_eq!(payload["ariadne_usage"]["source"], source);
        assert_eq!(payload["ariadne_usage"]["input_tokens"], 1110);
        assert_eq!(payload["ariadne_usage"]["cached_input_tokens"], 1000);
        assert_eq!(payload["ariadne_usage"]["output_tokens"], 7);
    }

    #[test]
    fn a_transcript_that_is_not_there_reports_nothing() {
        let dir = TempDir::new().expect("a temp dir");
        let mut payload = stop_payload("/nowhere/session.jsonl");
        attach_usage_within(
            AgentKind::ClaudeCode,
            "stop",
            &mut payload,
            &throttle_at(&dir, 0),
        );
        assert!(payload.get("ariadne_usage").is_none());
        // The event still goes out with everything it arrived with.
        assert_eq!(payload["transcript_path"], "/nowhere/session.jsonl");
    }

    #[test]
    fn a_tool_call_reports_mid_turn() {
        let (dir, source) = fixture();

        let mut payload = payload_for("PostToolUse", &source);
        attach_usage_within(
            AgentKind::ClaudeCode,
            "post_tool_use",
            &mut payload,
            &throttle_at(&dir, 0),
        );

        assert_eq!(payload["ariadne_usage"]["input_tokens"], 1110);
    }

    #[test]
    fn a_second_tool_call_within_the_interval_reports_nothing() {
        let (dir, source) = fixture();
        let mut first = payload_for("PostToolUse", &source);
        attach_usage_within(
            AgentKind::ClaudeCode,
            "post_tool_use",
            &mut first,
            &throttle_at(&dir, 0),
        );
        assert!(first.get("ariadne_usage").is_some());
        let stamp = fs::read_to_string(dir.path().join("stamp")).expect("a stamp");

        let mut second = payload_for("PostToolUse", &source);
        attach_usage_within(
            AgentKind::ClaudeCode,
            "post_tool_use",
            &mut second,
            &throttle_at(&dir, 9),
        );

        assert!(second.get("ariadne_usage").is_none());
        // The stamp only moves when the transcript is read, so one that has
        // not moved is the proof that the throttled call read nothing.
        assert_eq!(
            fs::read_to_string(dir.path().join("stamp")).expect("a stamp"),
            stamp
        );

        // Once the interval has passed, the next call reports again.
        let mut third = payload_for("PostToolUse", &source);
        attach_usage_within(
            AgentKind::ClaudeCode,
            "post_tool_use",
            &mut third,
            &throttle_at(&dir, 10),
        );
        assert!(third.get("ariadne_usage").is_some());
    }

    #[test]
    fn a_stop_right_after_a_tool_call_still_reports() {
        let (dir, source) = fixture();
        let mut tool_call = payload_for("PostToolUse", &source);
        attach_usage_within(
            AgentKind::ClaudeCode,
            "post_tool_use",
            &mut tool_call,
            &throttle_at(&dir, 0),
        );

        let mut stop = stop_payload(&source);
        attach_usage_within(
            AgentKind::ClaudeCode,
            "stop",
            &mut stop,
            &throttle_at(&dir, 1),
        );

        assert_eq!(stop["ariadne_usage"]["input_tokens"], 1110);
    }

    #[test]
    fn a_turn_boundary_refreshes_the_interval() {
        let (dir, source) = fixture();
        let mut stop = stop_payload(&source);
        attach_usage_within(
            AgentKind::ClaudeCode,
            "stop",
            &mut stop,
            &throttle_at(&dir, 0),
        );

        let mut tool_call = payload_for("PostToolUse", &source);
        attach_usage_within(
            AgentKind::ClaudeCode,
            "post_tool_use",
            &mut tool_call,
            &throttle_at(&dir, 5),
        );

        assert!(tool_call.get("ariadne_usage").is_none());
    }

    #[test]
    fn a_stamp_that_cannot_be_written_does_not_stop_a_report() {
        let (dir, source) = fixture();
        let unwritable = Throttle {
            stamp: dir.path().join("nowhere").join("stamp"),
            now: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
            interval: USAGE_INTERVAL,
        };

        // Twice in a row, with no interval between them: with nowhere to
        // remember the first report, the second one still goes out.
        for _ in 0..2 {
            let mut payload = payload_for("PostToolUse", &source);
            attach_usage_within(
                AgentKind::ClaudeCode,
                "post_tool_use",
                &mut payload,
                &unwritable,
            );
            assert_eq!(payload["ariadne_usage"]["input_tokens"], 1110);
        }
    }

    #[test]
    fn only_turn_boundaries_and_tool_calls_read_the_transcript() {
        // A fixture each, so every event is judged on a session that has
        // reported nothing yet.
        for event in ["pre_tool_use", "session_start", "unknown"] {
            let (dir, source) = fixture();
            let mut payload = stop_payload(&source);
            attach_usage_within(
                AgentKind::ClaudeCode,
                event,
                &mut payload,
                &throttle_at(&dir, 0),
            );
            assert!(payload.get("ariadne_usage").is_none(), "{event}");
        }
        for event in ["stop", "session_end", "user_prompt_submit", "post_tool_use"] {
            let (dir, source) = fixture();
            let mut payload = stop_payload(&source);
            attach_usage_within(
                AgentKind::ClaudeCode,
                event,
                &mut payload,
                &throttle_at(&dir, 0),
            );
            assert!(payload.get("ariadne_usage").is_some(), "{event}");
        }
    }

    /// OpenCode reports its own usage through the plugin; nothing here knows
    /// how to read what it writes.
    #[test]
    fn opencode_payloads_are_left_alone() {
        let dir = TempDir::new().expect("a temp dir");
        let mut payload = stop_payload("/nowhere/session.jsonl");
        attach_usage_within(
            AgentKind::Opencode,
            "stop",
            &mut payload,
            &throttle_at(&dir, 0),
        );
        assert!(payload.get("ariadne_usage").is_none());
    }
}
