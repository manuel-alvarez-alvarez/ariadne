//! `ariadne setup` — one-time host configuration that lives outside the
//! daemon's own home and therefore cannot be written per session.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{Value, json};

/// The events Ariadne asks Codex to report.
///
/// `SessionStart` is the one that matters: its payload carries `session_id`,
/// the only chance to capture Codex's internal id before a session can be
/// killed mid-turn. The rest are liveness: tool calls and prompts mean
/// running, `Stop` means idle, `SessionEnd` means gone.
const CODEX_EVENTS: [&str; 6] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "SessionEnd",
];

/// `ariadne setup codex-hooks` — declare Ariadne's hooks in Codex's global
/// hooks file, keeping whatever the user already has there.
///
/// The file must be the global one: Codex keys hook trust on the hooks-file
/// path, so hooks written into a worktree would raise a fresh trust prompt for
/// every task — and an agent waiting at a prompt reports nothing.
pub fn codex_hooks() -> Result<()> {
    let path = codex_hooks_path();
    let command = hook_command();

    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let merged = merged_codex_hooks(existing.as_deref(), &command)
        .with_context(|| format!("merging Ariadne's hooks into {}", path.display()))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, merged).with_context(|| format!("writing {}", path.display()))?;

    println!("hooks declared in {}", path.display());
    println!("  command: {command}");
    println!("  events:  {}", CODEX_EVENTS.join(", "));
    println!(
        "\nCodex runs hooks only after you trust them. Start `codex` once and \
         accept the\n\"Hooks need review\" prompt (\"Trust all and continue\"); \
         the trust is stored in\n~/.codex/config.toml and survives later runs."
    );
    Ok(())
}

/// Where Codex keeps its configuration, and so its hooks.
fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn codex_hooks_path() -> PathBuf {
    codex_home().join("hooks.json")
}

/// The command every hook runs. Trust is a hash of this string, so it must be
/// identical for every session — the session id comes from the payload, never
/// from the command line.
fn hook_command() -> String {
    let bin = std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "ariadne".to_string());
    format!("{bin} agent-event --kind codex")
}

/// Ariadne's hook entries merged into the body of an existing `hooks.json`.
///
/// Non-destructive (this is the user's file, not ours: their own entries and
/// any extra keys survive) and idempotent — a second run adds nothing, which
/// also keeps the entry indices Codex hashes its trust against stable.
fn merged_codex_hooks(existing: Option<&str>, command: &str) -> Result<String> {
    let mut root = match existing.map(str::trim).filter(|t| !t.is_empty()) {
        Some(text) => serde_json::from_str::<Value>(text).context("not valid JSON")?,
        None => json!({}),
    };
    let root_obj = root.as_object_mut().context("top level is not an object")?;
    let hooks = root_obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("`hooks` is not an object")?;

    let ours = json!({ "hooks": [{ "type": "command", "command": command }] });
    for event in CODEX_EVENTS {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .with_context(|| format!("`hooks.{event}` is not a list"))?;
        let present = entries.iter().any(|entry| {
            entry.pointer("/hooks/0/command").and_then(Value::as_str) == Some(command)
        });
        if !present {
            entries.push(ours.clone());
        }
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&root)?))
}

#[cfg(test)]
mod tests {
    use super::{CODEX_EVENTS, merged_codex_hooks};

    use serde_json::Value;

    const CMD: &str = "/usr/local/bin/ariadne agent-event --kind codex";

    fn commands_for(root: &Value, event: &str) -> Vec<String> {
        root["hooks"][event]
            .as_array()
            .unwrap_or_else(|| panic!("{event} is a list"))
            .iter()
            .map(|e| e["hooks"][0]["command"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn fresh_file_gets_every_event() {
        let merged = merged_codex_hooks(None, CMD).unwrap();
        let root: Value = serde_json::from_str(&merged).unwrap();
        for event in CODEX_EVENTS {
            assert_eq!(commands_for(&root, event), vec![CMD.to_string()]);
            assert_eq!(root["hooks"][event][0]["hooks"][0]["type"], "command");
        }
        assert!(merged.ends_with('\n'));
    }

    #[test]
    fn existing_user_entries_survive() {
        let existing = r#"{
            "description": "mine",
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "notify-me"}]}],
                "PreCompact": [{"hooks": [{"type": "command", "command": "archive"}]}]
            }
        }"#;
        let root: Value =
            serde_json::from_str(&merged_codex_hooks(Some(existing), CMD).unwrap()).unwrap();

        assert_eq!(root["description"], "mine");
        // Untouched event kept as-is, ours appended after theirs.
        assert_eq!(commands_for(&root, "PreCompact"), vec!["archive"]);
        assert_eq!(commands_for(&root, "Stop"), vec!["notify-me", CMD]);
        assert_eq!(commands_for(&root, "SessionStart"), vec![CMD]);
    }

    #[test]
    fn merging_twice_changes_nothing() {
        let once = merged_codex_hooks(None, CMD).unwrap();
        let twice = merged_codex_hooks(Some(&once), CMD).unwrap();
        assert_eq!(once, twice);

        let existing = r#"{"hooks": {"Stop": [{"hooks": [{"command": "notify-me"}]}]}}"#;
        let once = merged_codex_hooks(Some(existing), CMD).unwrap();
        let twice = merged_codex_hooks(Some(&once), CMD).unwrap();
        assert_eq!(once, twice);
        let root: Value = serde_json::from_str(&twice).unwrap();
        assert_eq!(commands_for(&root, "Stop"), vec!["notify-me", CMD]);
    }

    #[test]
    fn an_empty_file_is_treated_as_absent() {
        assert_eq!(
            merged_codex_hooks(Some("  \n"), CMD).unwrap(),
            merged_codex_hooks(None, CMD).unwrap()
        );
    }

    #[test]
    fn a_broken_file_is_never_clobbered() {
        assert!(merged_codex_hooks(Some("{not json"), CMD).is_err());
        assert!(merged_codex_hooks(Some(r#"{"hooks": []}"#), CMD).is_err());
        assert!(merged_codex_hooks(Some(r#"{"hooks": {"Stop": {}}}"#), CMD).is_err());
    }
}
