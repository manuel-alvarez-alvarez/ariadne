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
/// session that may be killed mid-turn. The rest are liveness — prompts and
/// tool calls mean running, `Stop` means idle, `SessionEnd` means gone.
pub const EVENTS: [&str; 6] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
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

#[cfg(test)]
mod tests {
    use super::{EVENTS, command, config_flags};

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
}
