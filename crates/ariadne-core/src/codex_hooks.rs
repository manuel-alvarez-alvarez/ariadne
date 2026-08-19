//! The Codex hook declaration, passed to every session as `-c` overrides.
//!
//! Codex grants hook trust by hashing each hook's definition and storing the
//! verdict under a source-derived key. For hooks declared on the command line
//! that key is the synthetic `/<session-flags>/config.toml:<event>:0:0`, with
//! no worktree or session in it — so one approval covers every future session,
//! in any directory, as long as the definition is byte-identical.
//!
//! That is the whole reason this lives in the domain crate: the daemon's codex
//! adapter builds these flags for each spawn, and the CLI's `setup codex-hooks`
//! builds them to raise the trust prompt at install time. If the two ever
//! drifted, every spawned session would sit at an unanswerable prompt. Nothing
//! that varies per session may appear here — the session id and everything
//! else comes from the hook's stdin payload.

/// The events Ariadne asks Codex to report.
///
/// `SessionStart` is the one that matters: its payload carries `session_id`
/// before the first turn begins, which is the only chance to record it for a
/// session that may be killed mid-turn. Most of the rest are liveness —
/// prompts and tool calls mean running, `Stop` means idle, `SessionEnd` means
/// gone.
///
/// `PermissionRequest` is the exception: it fires when codex is about to ask
/// the user to approve a command, which is the only moment a blocked session
/// is distinguishable from an idle one. Codex 0.147 declares eleven hook
/// events in all (`PreToolUse`, `PermissionRequest`, `PostToolUse`,
/// `PreCompact`, `PostCompact`, `SessionStart`, `SessionEnd`,
/// `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, `Stop`) and none of
/// them reports a failed turn — an API or auth error reaches the TUI as a
/// thread event and never a hook — so `agent_error` has no codex source yet.
pub const EVENTS: [&str; 7] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "Stop",
    "SessionEnd",
];

/// The command every hook runs. One command for all events: the event name
/// arrives as `hook_event_name` in the payload.
pub fn command(cli_bin: &str) -> String {
    format!("{cli_bin} agent-event --kind codex")
}

/// The `-c hooks.<Event>=[...]` argv pairs declaring every [`EVENTS`] entry.
///
/// Deterministic in `cli_bin` alone — see the module docs on why that matters.
pub fn config_flags(cli_bin: &str) -> Vec<String> {
    // TOML basic string: the value goes straight into argv, so the only
    // escaping needed is TOML's own.
    let command = command(cli_bin).replace('\\', "\\\\").replace('"', "\\\"");
    EVENTS
        .iter()
        .flat_map(|event| {
            [
                "-c".to_string(),
                format!(r#"hooks.{event}=[{{hooks=[{{type="command",command="{command}"}}]}}]"#),
            ]
        })
        .collect()
}

/// The source path codex files command-line hook trust under.
///
/// Synthetic on purpose (see the module docs): no worktree and no session in
/// it, so one verdict covers every later spawn.
pub const TRUST_SOURCE: &str = "/<session-flags>/config.toml";

/// The `hooks.state` key holding codex's verdict on one declared event.
///
/// Trust is granted per event, not per declaration: adding an event to
/// [`EVENTS`] leaves the others trusted and makes codex ask about the new one
/// alone — at the start of the next session, where nobody is watching. That
/// is what `ariadne doctor` reads these keys to catch.
pub fn trust_key(event: &str) -> String {
    format!("{TRUST_SOURCE}:{}:0:0", event_kind(event))
}

/// `PermissionRequest` -> `permission_request`: the spelling codex keys trust
/// on, and the one `ariadne agent-event` reports the event under.
pub fn event_kind(event: &str) -> String {
    let mut out = String::with_capacity(event.len() + 4);
    for (i, c) in event.chars().enumerate() {
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
    use super::{EVENTS, command, config_flags, trust_key};

    #[test]
    fn one_flag_pair_per_event() {
        let flags = config_flags("/usr/local/bin/ariadne");
        assert_eq!(flags.len(), EVENTS.len() * 2);
        for (i, event) in EVENTS.iter().enumerate() {
            assert_eq!(flags[i * 2], "-c");
            assert!(flags[i * 2 + 1].starts_with(&format!("hooks.{event}=")));
        }
    }

    /// The exact string codex hashes for trust. A change here invalidates
    /// every user's approval, so it is spelled out rather than derived.
    #[test]
    fn the_declaration_is_exactly_this() {
        let flags = config_flags("/usr/local/bin/ariadne");
        assert_eq!(
            flags[1],
            r#"hooks.SessionStart=[{hooks=[{type="command",command="/usr/local/bin/ariadne agent-event --kind codex"}]}]"#
        );
        assert_eq!(
            flags[7],
            r#"hooks.PermissionRequest=[{hooks=[{type="command",command="/usr/local/bin/ariadne agent-event --kind codex"}]}]"#
        );
    }

    #[test]
    fn every_event_runs_the_same_command() {
        let flags = config_flags("/opt/ariadne");
        let expected = command("/opt/ariadne");
        assert_eq!(
            flags.iter().filter(|f| f.contains(&expected)).count(),
            EVENTS.len()
        );
    }

    #[test]
    fn quotes_and_backslashes_stay_valid_toml() {
        let flags = config_flags(r#"/o"dd\path/ariadne"#);
        assert_eq!(
            flags[1],
            r#"hooks.SessionStart=[{hooks=[{type="command",command="/o\"dd\\path/ariadne agent-event --kind codex"}]}]"#
        );
    }
    /// The keys codex writes its verdicts under, spelled out for the same
    /// reason the declaration is: doctor reads them to tell a user whose
    /// trust predates a new event that they must run setup again.
    #[test]
    fn trust_keys_are_the_snake_case_event_under_the_synthetic_source() {
        assert_eq!(
            trust_key("PermissionRequest"),
            "/<session-flags>/config.toml:permission_request:0:0"
        );
        assert_eq!(trust_key("Stop"), "/<session-flags>/config.toml:stop:0:0");
        // Every declared event has one, and no two share it.
        let keys: std::collections::BTreeSet<_> = EVENTS.iter().map(|e| trust_key(e)).collect();
        assert_eq!(keys.len(), EVENTS.len());
    }

    /// The approval hook is the point of the whole exercise: without it a
    /// codex session waiting on a permission prompt looks exactly like one
    /// thinking.
    #[test]
    fn the_approval_hook_is_declared() {
        assert!(EVENTS.contains(&"PermissionRequest"));
        let flags = config_flags("/usr/local/bin/ariadne");
        assert!(
            flags
                .iter()
                .any(|f| f.starts_with("hooks.PermissionRequest="))
        );
    }
}
