//! Daemon configuration.
//!
//! Everything lives under a single root directory (default `~/.ariadne`),
//! optionally overridden by `ARIADNE_HOME` or `--home`. A `config.toml` in
//! the root can override individual paths and enable the TCP listener.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Optional overrides read from `<root>/config.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    socket_path: Option<PathBuf>,
    db_path: Option<PathBuf>,
    worktree_root: Option<PathBuf>,
    run_dir: Option<PathBuf>,
    /// e.g. "127.0.0.1:7676" — TCP listener is disabled unless set.
    tcp_listen: Option<SocketAddr>,
    /// tracing filter, e.g. "info,ariadne_daemon=debug"
    log_filter: Option<String>,
    /// Path to the `ariadne` CLI used for hooks and MCP (default: sibling of
    /// ariadned, else "ariadne" on PATH).
    cli_bin: Option<String>,
    /// Delete task branches after merge (default true). Only takes effect
    /// when the worktrees are deleted too: a kept engineer worktree has the
    /// task branch checked out, which pins it.
    delete_merged_branches: Option<bool>,
    /// Delete task worktrees after merge (default false: keep them under
    /// worktree_root so merged work can be inspected later).
    delete_merged_worktrees: Option<bool>,
}

/// Fully resolved daemon configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub root: PathBuf,
    pub socket_path: PathBuf,
    pub db_path: PathBuf,
    pub worktree_root: PathBuf,
    pub run_dir: PathBuf,
    pub pid_file: PathBuf,
    pub tcp_listen: Option<SocketAddr>,
    pub log_filter: String,
    pub cli_bin: String,
    pub delete_merged_branches: bool,
    pub delete_merged_worktrees: bool,
}

/// Default `ariadne` CLI: sibling of the running ariadned, else PATH lookup.
fn default_cli_bin() -> String {
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let sibling = dir.join("ariadne");
        if sibling.is_file() {
            return sibling.display().to_string();
        }
    }
    "ariadne".to_string()
}

impl Config {
    /// Resolve config: `--home` flag > `ARIADNE_HOME` > `~/.ariadne`,
    /// then apply `config.toml` overrides, then create the directories.
    pub fn load(home_override: Option<PathBuf>) -> Result<Self> {
        let root = home_override
            .or_else(|| std::env::var_os("ARIADNE_HOME").map(PathBuf::from))
            .or_else(|| dirs::home_dir().map(|h| h.join(".ariadne")))
            .context("cannot determine home directory; pass --home")?;

        let file: FileConfig = {
            let path = root.join("config.toml");
            if path.exists() {
                let raw = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?
            } else {
                FileConfig::default()
            }
        };

        let config = Config {
            socket_path: file
                .socket_path
                .unwrap_or_else(|| root.join("ariadne.sock")),
            db_path: file.db_path.unwrap_or_else(|| root.join("ariadne.db")),
            worktree_root: file.worktree_root.unwrap_or_else(|| root.join("worktrees")),
            run_dir: file.run_dir.unwrap_or_else(|| root.join("run")),
            pid_file: root.join("ariadned.pid"),
            tcp_listen: file.tcp_listen,
            log_filter: file.log_filter.unwrap_or_else(|| "info".to_string()),
            cli_bin: file.cli_bin.unwrap_or_else(default_cli_bin),
            delete_merged_branches: file.delete_merged_branches.unwrap_or(true),
            delete_merged_worktrees: file.delete_merged_worktrees.unwrap_or(false),
            root,
        };

        for dir in [&config.root, &config.worktree_root, &config.run_dir] {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }

        Ok(config)
    }
}
