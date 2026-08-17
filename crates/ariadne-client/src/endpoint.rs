//! Where the daemon lives: home directory and socket resolution.
//!
//! The CLI, the MCP server and `ariadned` itself must land on the same socket
//! for a given environment, so the ordering lives here and nowhere else:
//! `--home` > `ARIADNE_HOME` > `~/.ariadne` for the home directory, then that
//! home's `config.toml` `socket_path` > `<home>/ariadne.sock` for the socket.
//! Explicit endpoint overrides (`--host` / `ARIADNE_SOCKET`) sit in front of
//! all of it — see [`Client::resolve`](crate::Client::resolve).

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Environment variable moving the whole ariadne home directory.
pub const HOME_ENV: &str = "ARIADNE_HOME";

/// The `config.toml` fields the endpoint depends on. Unknown keys are ignored
/// here; the daemon parses the same file strictly and reports on them.
#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    socket_path: Option<PathBuf>,
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
/// to the default — the daemon is the one that reports a broken config.
pub fn socket_path(home: &Path) -> PathBuf {
    file_config(home)
        .socket_path
        .unwrap_or_else(|| default_socket_path(home))
}

/// Pidfile the daemon of this home writes.
pub fn pid_file(home: &Path) -> PathBuf {
    home.join("ariadned.pid")
}

fn file_config(home: &Path) -> FileConfig {
    std::fs::read_to_string(home.join("config.toml"))
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
}
