//! CLI command implementations.

pub mod agent;
pub mod agent_event;
pub mod attach;
pub mod attention;
pub mod doctor;
pub mod goal;
pub mod mcp;
pub mod profile;
pub mod repo;
pub mod session;
pub mod setup;
pub mod spawn;
pub mod task;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::json;

use ariadne_api::profiles::ProfileDto;
use ariadne_client::{Client, endpoint};

use crate::output::{Format, print_json};

/// `ariadne version` — client version always, daemon version when reachable.
pub async fn version(client: &Client, format: Format) -> Result<()> {
    let daemon = client.version().await;
    match format {
        Format::Json => print_json(&json!({
            "client": {"name": "ariadne", "version": env!("CARGO_PKG_VERSION")},
            "daemon": match &daemon {
                Ok(v) => json!({"name": v.name, "version": v.version}),
                Err(e) => json!({"error": e.human()}),
            },
            "endpoint": client.endpoint(),
        }))?,
        Format::Table => {
            println!("client:  ariadne {}", env!("CARGO_PKG_VERSION"));
            match daemon {
                Ok(v) => println!("daemon:  {} {}", v.name, v.version),
                // Not a failure of `version` itself, so it stays on stdout —
                // but it is still a line a person reads, so no "client error
                // (Connect)" in it.
                Err(e) => println!("daemon:  {}", e.human()),
            }
        }
    }
    Ok(())
}

/// `ariadne daemon status`
///
/// A failure is reported as the client's own error: "daemon not running at X"
/// on top of "cannot reach the ariadne daemon at X" said the endpoint twice.
pub async fn daemon_status(client: &Client, format: Format) -> Result<()> {
    let h = client.health().await?;
    match format {
        Format::Json => print_json(&json!({
            "status": h.status,
            "uptime_secs": h.uptime_secs,
            "endpoint": client.endpoint(),
        }))?,
        Format::Table => {
            println!("status:  {}", h.status);
            println!("uptime:  {}s", h.uptime_secs);
            println!("socket:  {}", client.endpoint());
        }
    }
    Ok(())
}

/// `ariadne daemon start` — spawn ariadned detached and wait for it to answer.
///
/// Builds its own client for `home` rather than taking the caller's: the
/// daemon it spawns listens on that home's socket, and `--endpoint` /
/// `ARIADNE_SOCKET` — which are never passed to ariadned — would send both the
/// already-running check and the readiness poll at a different daemon.
pub async fn daemon_start(home: Option<PathBuf>, format: Format) -> Result<()> {
    let client = Client::for_home(home.clone());
    if client.health().await.is_ok() {
        match format {
            Format::Json => print_json(&json!({
                "started": false,
                "endpoint": client.endpoint(),
            }))?,
            Format::Table => println!("daemon already running at {}", client.endpoint()),
        }
        return Ok(());
    }

    let binary = find_ariadned()?;
    let mut cmd = Command::new(&binary);
    if let Some(home) = &home {
        cmd.arg("--home").arg(home);
    }
    // Daemon output goes to a log file, readable via `ariadne daemon logs`.
    let log_dir = ariadne_home(home.clone());
    std::fs::create_dir_all(&log_dir).ok();
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("ariadned.log"))
        .with_context(|| format!("opening log file in {}", log_dir.display()))?;
    cmd.stdin(Stdio::null())
        .stdout(log.try_clone().context("cloning log handle")?)
        .stderr(log);
    let child = cmd
        .spawn()
        .with_context(|| format!("spawning {}", binary.display()))?;

    // Poll for readiness.
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if client.health().await.is_ok() {
            match format {
                Format::Json => print_json(&json!({
                    "started": true,
                    "pid": child.id(),
                    "endpoint": client.endpoint(),
                }))?,
                Format::Table => println!("ariadned started (pid {})", child.id()),
            }
            return Ok(());
        }
    }
    bail!(
        "ariadned (pid {}) did not answer on {} within 5s",
        child.id(),
        client.endpoint()
    );
}

/// `ariadne daemon stop` — SIGTERM via pidfile.
pub fn daemon_stop(format: Format) -> Result<()> {
    let pid_file = endpoint::pid_file(&ariadne_home(None));
    let pid = std::fs::read_to_string(&pid_file)
        .with_context(|| {
            format!(
                "no pidfile at {} — is the daemon running?",
                pid_file.display()
            )
        })?
        .trim()
        .to_string();

    let status = Command::new("kill")
        .arg(&pid)
        .status()
        .context("running kill")?;
    if !status.success() {
        bail!(
            "kill {pid} failed — stale pidfile? ({})",
            pid_file.display()
        );
    }
    match format {
        Format::Json => print_json(&json!({"signalled": "SIGTERM", "pid": pid}))?,
        Format::Table => println!("sent SIGTERM to ariadned (pid {pid})"),
    }
    Ok(())
}

/// `ariadne daemon logs [-f]` — show the daemon log via tail.
pub fn daemon_logs(follow: bool) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let log = ariadne_home(None).join("ariadned.log");
    if !log.is_file() {
        bail!("no daemon log at {}", log.display());
    }
    let mut cmd = Command::new("tail");
    cmd.arg("-n").arg("200");
    if follow {
        cmd.arg("-f");
    }
    let err = cmd.arg(&log).exec();
    bail!("failed to exec tail: {err}");
}

/// Ask before something irreversible, and take silence for "no".
///
/// `yes` (`-y`) answers for the caller, and so does a stdin that is not a
/// terminal: a script has nobody to ask, and a prompt written into a pipe
/// would hang a cron job rather than fail it. Declining is an error, so
/// `ariadne goal cancel x && deploy` does not run the second half.
pub fn confirm(question: &str, yes: bool) -> Result<()> {
    if yes || !std::io::stdin().is_terminal() {
        return Ok(());
    }
    // The prompt is not output: it belongs on stderr with the other notes.
    eprint!("{question} [y/N] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("reading your answer")?;
    match answer.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => bail!("aborted"),
    }
}

/// Profile ids paired with the names they are known by.
///
/// Profiles are name-addressable everywhere else in the CLI, so an inspect
/// block that prints a bare ULID names nobody.
pub struct ProfileNames(std::collections::HashMap<String, String>);

impl ProfileNames {
    /// One list call for the whole block. A name is a courtesy: a daemon that
    /// will not answer this leaves the ids bare rather than failing the
    /// inspect that asked for them.
    pub async fn fetch(client: &Client) -> Self {
        let profiles: Vec<ProfileDto> = client.get_json("/v1/profiles").await.unwrap_or_default();
        Self(profiles.into_iter().map(|p| (p.id, p.name)).collect())
    }

    /// `Name (id)`, or the bare id when no profile answers to it.
    pub fn label(&self, id: &str) -> String {
        match self.0.get(id) {
            Some(name) => format!("{name} ({id})"),
            None => id.to_string(),
        }
    }

    /// A list of ids, labelled, in the order they were given.
    pub fn labels(&self, ids: &[String]) -> String {
        ids.iter()
            .map(|id| self.label(id))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Resolve the ariadne home directory the same way the daemon does.
fn ariadne_home(home_override: Option<PathBuf>) -> PathBuf {
    endpoint::home(home_override).unwrap_or_else(|| PathBuf::from(".ariadne"))
}

/// Find the ariadned binary: next to the current executable, else on PATH.
pub fn find_ariadned() -> Result<PathBuf> {
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let sibling = dir.join("ariadned");
        if is_executable(&sibling) {
            return Ok(sibling);
        }
    }
    which("ariadned").context("ariadned not found next to ariadne or on PATH")
}

/// First entry of `PATH` holding an executable of that name.
pub fn which(name: &str) -> Option<PathBuf> {
    which_in(&std::env::var_os("PATH")?, name)
}

/// The same lookup against a given `PATH`, so it can be tested without
/// rewriting the environment of the process running the tests.
fn which_in(path: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// A file that can actually be run.
///
/// Presence is not enough: a `codex` on PATH with no execute bit is a file,
/// not an agent, and every caller here is about to run what it finds — or,
/// in `ariadne doctor`, to report that something else can.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // Follows symlinks on purpose: what matters is what running it reaches.
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::fs::PermissionsExt;

    fn write(path: &Path, mode: u32) {
        std::fs::write(path, "").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// `which` answers for callers that are about to run what it finds, so a
    /// file without an execute bit is not an answer.
    #[test]
    fn a_file_with_no_execute_bit_is_not_executable() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("codex");
        write(&plain, 0o644);
        assert!(!is_executable(&plain));
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&plain));
    }

    #[test]
    fn a_directory_is_never_executable() {
        let dir = tempfile::tempdir().unwrap();
        let named = dir.path().join("ariadned");
        std::fs::create_dir(&named).unwrap();
        assert!(!is_executable(&named));
    }

    /// PATH order stands, but a non-executable entry is skipped rather than
    /// shadowing the real thing further along.
    #[test]
    fn a_non_executable_entry_does_not_shadow_a_later_one() {
        let dir = tempfile::tempdir().unwrap();
        let (first, second) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        write(&first.join("codex"), 0o644);
        write(&second.join("codex"), 0o755);

        let path = std::env::join_paths([&first, &second]).unwrap();
        assert_eq!(which_in(&path, "codex"), Some(second.join("codex")));
        let only_first = std::env::join_paths([&first]).unwrap();
        assert_eq!(which_in(&only_first, "codex"), None);
    }
}
