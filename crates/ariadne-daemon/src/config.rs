//! Daemon configuration.
//!
//! Everything lives under a single root directory (default `~/.ariadne`),
//! optionally overridden by `ARIADNE_HOME` or `--home`. A `config.toml` in
//! the root can override individual paths and enable the TCP listener.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

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
    pub typed_input_window: Duration,
}

/// How long a freshly launched pane is watched for a TUI to type a resume
/// instruction into (see `Launcher::deliver_typed_input`): two minutes,
/// because a slow CLI start draws its first frame well after the spawn.
///
/// Resolved here rather than written as a constant where it is used so that a
/// test can watch a pane that never draws without spending two real minutes
/// on it. There is no `config.toml` key behind it: nothing about it is the
/// user's to choose.
const DEFAULT_TYPED_INPUT_WINDOW: Duration = Duration::from_secs(120);

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
            typed_input_window: DEFAULT_TYPED_INPUT_WINDOW,
            root,
        };

        for dir in [&config.root, &config.worktree_root, &config.run_dir] {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A home with `config.toml` in it, written from `toml`.
    fn home_with(toml: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        std::fs::write(dir.path().join("home/config.toml"), toml).unwrap();
        dir
    }

    /// What the file does not say keeps its default — and what it may say is
    /// only ever what `FileConfig` names, since the reading is strict: a key
    /// this daemon no longer has stops it rather than being ignored (see
    /// `endpoint::parse_config`).
    #[test]
    fn a_config_file_that_says_nothing_keeps_every_default() {
        let dir = home_with("");
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert!(config.delete_merged_worktrees);
        assert!(config.delete_merged_branches);
        assert!(config.prevent_sleep);
    }

    #[test]
    fn a_config_file_is_read_into_the_daemons_own_shape() {
        let dir = home_with("log_filter = \"debug\"\nprevent_sleep = false\n");
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(config.log_filter, "debug");
        assert!(!config.prevent_sleep);
        assert!(
            config.delete_merged_worktrees,
            "and what the file does not say keeps its default"
        );
    }
}
