//! Installs the OpenCode events plugin shipped inside the binary.

use std::path::PathBuf;

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

/// Write (or refresh) the plugin file. Called at daemon startup.
pub fn install() -> Result<PathBuf> {
    let path = plugin_path();
    let dir = path.parent().expect("plugin path has a parent");
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // Only rewrite on change to keep mtimes stable.
    if std::fs::read_to_string(&path)
        .map(|cur| cur == PLUGIN_JS)
        .unwrap_or(false)
    {
        return Ok(path);
    }
    std::fs::write(&path, PLUGIN_JS).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}
