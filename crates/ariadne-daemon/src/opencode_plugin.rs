//! Installs the OpenCode events plugin shipped inside the binary.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const PLUGIN_JS: &str = include_str!("../../../assets/opencode-plugin/ariadne-events.js");

/// Where the plugin lives on disk (referenced from generated opencode configs).
pub fn plugin_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ariadne")
        .join("opencode-plugin")
        .join("ariadne-events.js")
}

/// The event types the plugin forwards, read out of the plugin source.
///
/// The JS `interesting` set is the single source of truth — parsing it here
/// keeps the ingest tests honest without a second list to drift.
#[cfg(test)]
pub fn declared_events() -> Vec<String> {
    let (_, rest) = PLUGIN_JS
        .split_once("const interesting = new Set([")
        .expect("plugin declares an `interesting` set");
    let (body, _) = rest.split_once("]);").expect("the set literal is closed");
    body.split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            entry
                .strip_prefix('"')
                .and_then(|e| e.strip_suffix('"'))
                .map(str::to_string)
        })
        .collect()
}

/// Write (or refresh) the plugin file. Called at daemon startup.
///
/// Content-addressed rather than versioned: an existing install is rewritten
/// whenever the shipped source differs, so upgrading the daemon upgrades the
/// plugin on the next start with nothing for the user to do.
pub fn install() -> Result<PathBuf> {
    let path = plugin_path();
    install_to(&path)?;
    Ok(path)
}

fn install_to(path: &Path) -> Result<()> {
    let dir = path.parent().expect("plugin path has a parent");
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // Only rewrite on change to keep mtimes stable.
    if std::fs::read_to_string(path)
        .map(|cur| cur == PLUGIN_JS)
        .unwrap_or(false)
    {
        return Ok(());
    }
    std::fs::write(path, PLUGIN_JS).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PLUGIN_JS, declared_events, install_to};

    #[test]
    fn the_forwarded_set_parses_out_of_the_plugin_source() {
        let events = declared_events();
        assert!(
            events.contains(&"session.created".to_string()),
            "{events:?}"
        );
        assert!(
            events.contains(&"permission.asked".to_string()),
            "{events:?}"
        );
        assert!(!events.iter().any(|e| e.contains('"')), "{events:?}");
    }

    /// The upgrade path for setups that already have a plugin file: nothing
    /// is versioned, so a stale install is only replaced if `install` looks
    /// at the content. It runs at every daemon start.
    #[test]
    fn an_existing_stale_plugin_is_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("ariadne-events.js");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "// an older ariadne wrote this\n").unwrap();

        install_to(&path).expect("install over the stale file");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PLUGIN_JS);
    }

    /// ...and a fresh setup gets the whole directory created for it.
    #[test]
    fn a_missing_plugin_is_written_from_scratch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("opencode-plugin").join("ariadne-events.js");

        install_to(&path).expect("install into an empty home");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PLUGIN_JS);
    }

    /// An unchanged install is left alone, mtime included — the daemon
    /// restarts often and opencode watches these files.
    #[test]
    fn an_up_to_date_plugin_is_not_rewritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ariadne-events.js");
        install_to(&path).expect("first install");
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();

        install_to(&path).expect("second install");
        let after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(before, after);
    }
}
