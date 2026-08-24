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
    pub gh_bin: String,
    pub glab_bin: String,
    pub pr_poll_secs: u64,
    pub typed_input_window: Duration,
    pub running_quiet_flag_secs: u64,
    pub running_quiet_resume_secs: u64,
}

/// How often an integrating task's pull request is polled by default: a few
/// minutes, because what moves it is a human reading a diff.
const DEFAULT_PR_POLL_SECS: u64 = 180;

/// How long a freshly launched pane is watched for a TUI to type a resume
/// instruction into (see `Launcher::deliver_typed_input`): two minutes,
/// because a slow CLI start draws its first frame well after the spawn.
///
/// Resolved here rather than written as a constant where it is used so that a
/// test can watch a pane that never draws without spending two real minutes
/// on it. There is no `config.toml` key behind it: nothing about it is the
/// user's to choose.
const DEFAULT_TYPED_INPUT_WINDOW: Duration = Duration::from_secs(120);
/// How long a running agent may report nothing before the user is told about
/// it: twenty minutes, which is long enough for the slowest turn an agent
/// takes between two things worth reporting and short enough that a wedged
/// one is not left there for the afternoon.
const DEFAULT_RUNNING_QUIET_FLAG_SECS: u64 = 1_200;

/// And how long before that agent is relaunched: three quarters of an hour,
/// which is the flag plus enough of a wait for a person to have looked at it
/// first.
const DEFAULT_RUNNING_QUIET_RESUME_SECS: u64 = 2_700;

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
            typed_input_window: DEFAULT_TYPED_INPUT_WINDOW,
            running_quiet_flag_secs: file
                .running_quiet_flag_secs
                .unwrap_or(DEFAULT_RUNNING_QUIET_FLAG_SECS),
            running_quiet_resume_secs: file
                .running_quiet_resume_secs
                .unwrap_or(DEFAULT_RUNNING_QUIET_RESUME_SECS),
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

    #[test]
    fn the_watchdog_thresholds_default_to_twenty_and_forty_five_minutes() {
        let dir = home_with("");
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(config.running_quiet_flag_secs, 1_200);
        assert_eq!(config.running_quiet_resume_secs, 2_700);
    }

    #[test]
    fn the_watchdog_thresholds_are_read_from_the_config_file() {
        let dir = home_with("running_quiet_flag_secs = 60\nrunning_quiet_resume_secs = 120\n");
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(config.running_quiet_flag_secs, 60);
        assert_eq!(config.running_quiet_resume_secs, 120);
        assert_eq!(
            config.pr_poll_secs, DEFAULT_PR_POLL_SECS,
            "and what the file does not say keeps its default"
        );
    }
}
