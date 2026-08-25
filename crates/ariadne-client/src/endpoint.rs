//! Where the daemon lives: home directory and socket resolution.
//!
//! The CLI, the MCP server and `ariadned` itself must land on the same socket
//! for a given environment, so the ordering lives here and nowhere else:
//! `--home` > `ARIADNE_HOME` > `~/.ariadne` for the home, then that home's
//! `config.toml` `socket_path` > `<home>/ariadne.sock` for the socket. Explicit
//! endpoint overrides sit in front of all of it — see
//! [`Client::resolve`](crate::Client::resolve).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Environment variable moving the whole ariadne home directory.
pub const HOME_ENV: &str = "ARIADNE_HOME";

/// The `config.toml` fields the endpoint depends on. Unknown keys are ignored
/// here: a socket has to be named even for a config that will not parse, so
/// that whoever reports the breakage can still say which daemon it is about.
#[derive(Debug, Default, Deserialize)]
struct SocketOnly {
    socket_path: Option<PathBuf>,
}

/// Everything `<home>/config.toml` may set, read strictly: an unknown key is
/// an error rather than a silently ignored line.
///
/// It lives here rather than in the daemon because the daemon is not the only
/// one that has to answer for it — `ariadne doctor` reports on a config whose
/// daemon refuses to start because of it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub socket_path: Option<PathBuf>,
    pub db_path: Option<PathBuf>,
    pub worktree_root: Option<PathBuf>,
    pub run_dir: Option<PathBuf>,
    /// e.g. "127.0.0.1:7676" — TCP listener is disabled unless set.
    pub tcp_listen: Option<SocketAddr>,
    /// tracing filter, e.g. "info,ariadne_daemon=debug"
    pub log_filter: Option<String>,
    /// Path to the `ariadne` CLI used for hooks and MCP (default: sibling of
    /// ariadned, else "ariadne" on PATH).
    pub cli_bin: Option<String>,
    /// Delete task branches after merge (default true). Only takes effect
    /// when the worktrees are deleted too: a kept engineer worktree has the
    /// task branch checked out, which pins it.
    pub delete_merged_branches: Option<bool>,
    /// Delete task worktrees after merge (default true). Set to false to keep
    /// them under worktree_root so merged work can be inspected later;
    /// cancelled tasks always keep theirs, salvageable work included.
    pub delete_merged_worktrees: Option<bool>,
    /// Keep the machine awake while agent sessions are live (default true).
    pub prevent_sleep: Option<bool>,
}

/// Why `<home>/config.toml` could not be read as configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// The config file of a home directory.
pub fn config_file(home: &Path) -> PathBuf {
    home.join("config.toml")
}

/// Read `<home>/config.toml` the way the daemon reads it. `Ok(None)` means
/// there is no config file at all, which is the ordinary case.
pub fn parse_config(home: &Path) -> Result<Option<FileConfig>, ConfigError> {
    let path = config_file(home);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    toml::from_str(&raw)
        .map(Some)
        .map_err(|source| ConfigError::Parse { path, source })
}

/// Ariadne home: `--home` > `ARIADNE_HOME` > `~/.ariadne`. `None` only when
/// no override was given and the user's home directory is unknown.
pub fn home(home_override: Option<PathBuf>) -> Option<PathBuf> {
    resolve_home(
        home_override,
        std::env::var_os(HOME_ENV).map(PathBuf::from),
        dirs::home_dir(),
    )
}

fn resolve_home(
    home_override: Option<PathBuf>,
    env_home: Option<PathBuf>,
    user_home: Option<PathBuf>,
) -> Option<PathBuf> {
    home_override
        .or(env_home)
        .or_else(|| user_home.map(|h| h.join(".ariadne")))
}

/// Socket of a home whose `config.toml` says nothing about it.
pub fn default_socket_path(home: &Path) -> PathBuf {
    home.join("ariadne.sock")
}

/// Socket the daemon of this home listens on: `config.toml` `socket_path`,
/// else `<home>/ariadne.sock`. A missing or unparseable config.toml resolves
/// to the default — reporting a broken config is the daemon's job.
pub fn socket_path(home: &Path) -> PathBuf {
    file_config(home)
        .socket_path
        .unwrap_or_else(|| default_socket_path(home))
}

/// Pidfile the daemon of this home writes.
pub fn pid_file(home: &Path) -> PathBuf {
    home.join("ariadned.pid")
}

fn file_config(home: &Path) -> SocketOnly {
    std::fs::read_to_string(config_file(home))
        .ok()
        .and_then(|raw| toml::from_str(&raw).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(home: &Path, body: &str) {
        std::fs::write(home.join("config.toml"), body).unwrap();
    }

    #[test]
    fn explicit_home_wins() {
        let root = resolve_home(
            Some("/flag".into()),
            Some("/env".into()),
            Some("/users/me".into()),
        );
        assert_eq!(root, Some(PathBuf::from("/flag")));
    }

    #[test]
    fn env_home_beats_user_home() {
        let root = resolve_home(None, Some("/env".into()), Some("/users/me".into()));
        assert_eq!(root, Some(PathBuf::from("/env")));
    }

    #[test]
    fn user_home_falls_back_to_dot_ariadne() {
        let root = resolve_home(None, None, Some("/users/me".into()));
        assert_eq!(root, Some(PathBuf::from("/users/me/.ariadne")));
    }

    #[test]
    fn no_home_at_all_is_unresolved() {
        assert_eq!(resolve_home(None, None, None), None);
    }

    #[test]
    fn socket_defaults_inside_the_home() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(socket_path(dir.path()), dir.path().join("ariadne.sock"));
    }

    #[test]
    fn config_socket_path_overrides_the_default() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "socket_path = \"/scratch/custom.sock\"\n");
        assert_eq!(
            socket_path(dir.path()),
            PathBuf::from("/scratch/custom.sock")
        );
    }

    #[test]
    fn other_config_keys_do_not_hide_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            "tcp_listen = \"127.0.0.1:7676\"\nsocket_path = \"/scratch/custom.sock\"\n",
        );
        assert_eq!(
            socket_path(dir.path()),
            PathBuf::from("/scratch/custom.sock")
        );
    }

    #[test]
    fn broken_config_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "socket_path = [not toml\n");
        assert_eq!(socket_path(dir.path()), dir.path().join("ariadne.sock"));
    }

    /// No config file is not a failure: it is how most homes are set up.
    #[test]
    fn a_home_without_a_config_parses_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(parse_config(dir.path()).unwrap().is_none());
    }

    #[test]
    fn a_config_parses_into_its_fields() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            "db_path = \"/scratch/ariadne.db\"\nprevent_sleep = false\n",
        );
        let config = parse_config(dir.path()).unwrap().expect("a config");
        assert_eq!(config.db_path, Some(PathBuf::from("/scratch/ariadne.db")));
        assert_eq!(config.prevent_sleep, Some(false));
        assert_eq!(config.socket_path, None);
    }

    /// A misspelled key is the whole point of parsing strictly: it would
    /// otherwise be a setting the user believes is in force and is not.
    #[test]
    fn an_unknown_key_is_an_error_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "prevent_slep = false\n");
        let err = parse_config(dir.path())
            .expect_err("unknown key")
            .to_string();
        assert!(err.contains("config.toml"), "{err}");
        assert!(err.contains("prevent_slep"), "{err}");
    }

    #[test]
    fn a_syntax_error_is_reported_rather_than_ignored() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "socket_path = [not toml\n");
        assert!(matches!(
            parse_config(dir.path()),
            Err(ConfigError::Parse { .. })
        ));
    }
}
