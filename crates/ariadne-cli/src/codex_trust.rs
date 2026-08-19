//! What codex still trusts of Ariadne's hook declaration.
//!
//! The declaration ([`ariadne_core::codex_hooks`]) travels with every spawned
//! session and codex runs none of it until the user has trusted it. Trust is
//! granted per event and keyed on a synthetic path, so it survives every
//! later session — and survives an Ariadne upgrade that *adds* an event, at
//! which point the six old hooks keep reporting and the new one silently does
//! not. Codex asks about it at the start of the next session, in a TUI nobody
//! is watching.
//!
//! So it is read back here, out of codex's own config, and reported by
//! `ariadne doctor` and by `ariadne setup codex-hooks`. Nothing is written:
//! only the user can grant trust, and only codex can record it.
//!
//! What is read is which events have a verdict, not what the verdict says.
//! The stored hash is codex's own normalization of the hook definition, and
//! reproducing it here would be one more thing to drift; a declaration whose
//! command changed — an `ariadne` that moved — keeps its key and changes its
//! hash, and codex is the only one that can see that. The failure it leaves
//! is the same one, and codex names it at the next session start.

use std::path::{Path, PathBuf};

use ariadne_core::codex_hooks;

/// Codex's home, resolved as codex itself resolves it.
pub fn codex_home() -> Option<PathBuf> {
    match std::env::var_os("CODEX_HOME") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => Some(dirs::home_dir()?.join(".codex")),
    }
}

/// Which of the declared events codex would actually run, and which it would
/// stop and ask about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trust {
    /// The config the verdicts were read from, whether or not it is there.
    pub config: PathBuf,
    /// Whether that file exists at all — a codex that has never run has no
    /// verdicts rather than negative ones.
    pub config_exists: bool,
    /// Declared events codex has a usable verdict for.
    pub trusted: Vec<&'static str>,
    /// Declared events it does not: never trusted, or trusted and since
    /// turned off.
    pub untrusted: Vec<&'static str>,
}

impl Trust {
    /// Read the verdicts out of `<codex_home>/config.toml`.
    ///
    /// Fail-soft: a missing or unparsable config means "nothing is trusted",
    /// which is the safe answer — it points at the command that fixes it.
    pub fn read(codex_home: &Path) -> Self {
        let config = codex_home.join("config.toml");
        let text = std::fs::read_to_string(&config);
        let (trusted, untrusted) = classify(text.as_deref().unwrap_or(""));
        Self {
            config,
            config_exists: text.is_ok(),
            trusted,
            untrusted,
        }
    }

    /// Everything Ariadne declares will run.
    pub fn is_complete(&self) -> bool {
        self.untrusted.is_empty()
    }

    /// Some hooks report and some do not — the shape an upgrade that added an
    /// event leaves behind, and the one worth naming, because everything
    /// looks fine until the new hook is the one that mattered.
    pub fn is_stale(&self) -> bool {
        !self.trusted.is_empty() && !self.untrusted.is_empty()
    }

    /// The untrusted events in the spelling codex uses for them.
    pub fn untrusted_keys(&self) -> Vec<String> {
        self.untrusted
            .iter()
            .map(|e| codex_hooks::event_kind(e))
            .collect()
    }
}

/// Split [`codex_hooks::EVENTS`] by what a codex config says about each.
///
/// An event counts as trusted when its `hooks.state` entry carries a
/// `trusted_hash` and has not been turned off since: a disabled hook does not
/// run whatever its hash says, and reporting it as trusted would promise
/// events that never arrive.
fn classify(config: &str) -> (Vec<&'static str>, Vec<&'static str>) {
    let state = toml::from_str::<toml::Table>(config)
        .ok()
        .and_then(|doc| doc.get("hooks")?.get("state").cloned());
    codex_hooks::EVENTS
        .into_iter()
        .partition(|event| is_trusted(state.as_ref(), &codex_hooks::trust_key(event)))
}

fn is_trusted(state: Option<&toml::Value>, key: &str) -> bool {
    let Some(entry) = state.and_then(|s| s.get(key)) else {
        return false;
    };
    let hashed = entry
        .get("trusted_hash")
        .and_then(|h| h.as_str())
        .is_some_and(|h| !h.is_empty());
    let disabled = entry.get("enabled").and_then(|e| e.as_bool()) == Some(false);
    hashed && !disabled
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One `hooks.state` entry as codex writes it.
    fn entry(event: &str, body: &str) -> String {
        format!(
            "[hooks.state.\"{}\"]\n{body}\n",
            codex_hooks::trust_key(event)
        )
    }

    fn trusted(event: &str) -> String {
        entry(event, "trusted_hash = \"sha256:abc\"")
    }

    #[test]
    fn nothing_is_trusted_without_a_config() {
        let dir = tempfile::tempdir().unwrap();
        let trust = Trust::read(dir.path());
        assert!(!trust.config_exists);
        assert!(trust.trusted.is_empty());
        assert_eq!(trust.untrusted.len(), codex_hooks::EVENTS.len());
        assert!(!trust.is_complete());
        // Not "stale": nothing was ever trusted, so nothing went out of date.
        assert!(!trust.is_stale());
    }

    #[test]
    fn a_config_that_trusts_every_declared_event_is_complete() {
        let config: String = codex_hooks::EVENTS.iter().map(|e| trusted(e)).collect();
        let (trusted, untrusted) = classify(&config);
        assert_eq!(trusted.len(), codex_hooks::EVENTS.len());
        assert!(untrusted.is_empty());
    }

    /// The case this whole module exists for: trust granted before
    /// `PermissionRequest` was declared covers the other six and leaves the
    /// approval hook — the one that reports a blocked session — silent.
    #[test]
    fn trust_predating_a_new_event_reads_as_stale() {
        let config: String = codex_hooks::EVENTS
            .iter()
            .filter(|e| **e != "PermissionRequest")
            .map(|e| trusted(e))
            .collect();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), &config).unwrap();

        let trust = Trust::read(dir.path());
        assert!(trust.config_exists);
        assert_eq!(trust.untrusted, vec!["PermissionRequest"]);
        assert_eq!(trust.untrusted_keys(), vec!["permission_request"]);
        assert!(trust.is_stale());
        assert!(!trust.is_complete());
    }

    /// The verdicts codex files for the hooks a user keeps in `hooks.json`
    /// are for a different source and say nothing about ours.
    #[test]
    fn trust_for_a_hooks_json_of_the_users_own_does_not_count() {
        let config = "[hooks.state.\"/Users/me/.codex/hooks.json:stop:0:0\"]\n\
                      trusted_hash = \"sha256:abc\"\n";
        let (trusted, untrusted) = classify(config);
        assert!(trusted.is_empty());
        assert_eq!(untrusted.len(), codex_hooks::EVENTS.len());
    }

    /// A hook trusted once and turned off since does not run, and must not
    /// be reported as if it did.
    #[test]
    fn a_disabled_hook_is_not_trusted() {
        let config = entry("Stop", "enabled = false\ntrusted_hash = \"sha256:abc\"");
        let (trusted, _) = classify(&config);
        assert!(!trusted.contains(&"Stop"));
        // ...while an explicit `enabled = true` is just trust.
        let config = entry("Stop", "enabled = true\ntrusted_hash = \"sha256:abc\"");
        let (trusted, _) = classify(&config);
        assert!(trusted.contains(&"Stop"));
    }

    /// An entry with no hash is codex remembering the hook, not trusting it.
    #[test]
    fn an_entry_without_a_hash_is_not_trust() {
        let config = entry("Stop", "enabled = true");
        let (trusted, _) = classify(&config);
        assert!(trusted.is_empty());
    }

    /// A config we cannot read is not evidence of trust.
    #[test]
    fn an_unparsable_config_trusts_nothing() {
        let (trusted, untrusted) = classify("this is not toml {{{");
        assert!(trusted.is_empty());
        assert_eq!(untrusted.len(), codex_hooks::EVENTS.len());
    }

    #[test]
    fn codex_home_follows_the_env_var() {
        // Read-only assertion about the default shape; CODEX_HOME is not set
        // in the test process, and setting it would race the other tests.
        assert!(
            std::env::var_os("CODEX_HOME").is_some()
                || codex_home().is_none_or(|h| h.ends_with(".codex"))
        );
    }
}
