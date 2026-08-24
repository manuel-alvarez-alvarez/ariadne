//! Daemon configuration.
//!
//! Everything lives under a single root directory (default `~/.ariadne`),
//! optionally overridden by `ARIADNE_HOME` or `--home`. A `config.toml` in
//! the root can override individual paths and enable the TCP listener.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};

use ariadne_client::endpoint;

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
    pub prevent_sleep: bool,
    pub gh_bin: String,
    pub glab_bin: String,
    pub pr_poll_secs: u64,
}

/// How often an integrating task's pull request is polled by default: a few
/// minutes, because what moves it is a human reading a diff.
const DEFAULT_PR_POLL_SECS: u64 = 180;

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
    ///
    /// The home and socket ordering comes from `ariadne_client::endpoint`, so
    /// the CLI and the MCP server address exactly the daemon this starts.
    pub fn load(home_override: Option<PathBuf>) -> Result<Self> {
        let root = endpoint::home(home_override)
            .context("cannot determine home directory; pass --home")?;

        // The strict reading of `config.toml` lives beside the socket
        // resolution it shares a file with, so `ariadne doctor` reports on
        // exactly what would stop this daemon from starting.
        let file = endpoint::parse_config(&root)?.unwrap_or_default();

        let config = Config {
            socket_path: file
                .socket_path
                .unwrap_or_else(|| endpoint::default_socket_path(&root)),
            db_path: file.db_path.unwrap_or_else(|| root.join("ariadne.db")),
            worktree_root: file.worktree_root.unwrap_or_else(|| root.join("worktrees")),
            run_dir: file.run_dir.unwrap_or_else(|| root.join("run")),
            pid_file: endpoint::pid_file(&root),
            tcp_listen: file.tcp_listen,
            log_filter: file.log_filter.unwrap_or_else(|| "info".to_string()),
            cli_bin: file.cli_bin.unwrap_or_else(default_cli_bin),
            delete_merged_branches: file.delete_merged_branches.unwrap_or(true),
            delete_merged_worktrees: file.delete_merged_worktrees.unwrap_or(true),
            prevent_sleep: file.prevent_sleep.unwrap_or(true),
            gh_bin: file.gh_bin.unwrap_or_else(|| "gh".to_string()),
            glab_bin: file.glab_bin.unwrap_or_else(|| "glab".to_string()),
            pr_poll_secs: file.pr_poll_secs.unwrap_or(DEFAULT_PR_POLL_SECS),
            root,
        };

        for dir in [&config.root, &config.worktree_root, &config.run_dir] {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }

        Ok(config)
    }
}
