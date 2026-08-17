//! CLI command implementations.

pub mod agent_event;
pub mod attach;
pub mod goal;
pub mod mcp;
pub mod profile;
pub mod session;
pub mod setup;
pub mod task;

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use ariadne_client::{Client, endpoint};

/// Shared `--role` value parser (planner | engineer | reviewer).
pub fn parse_role(s: &str) -> Result<ariadne_core::Role, String> {
    s.parse()
}

/// `ariadne version` — client version always, daemon version when reachable.
pub async fn version(client: &Client) -> Result<()> {
    println!("client:  ariadne {}", env!("CARGO_PKG_VERSION"));
    match client.version().await {
        Ok(v) => println!("daemon:  {} {}", v.name, v.version),
        Err(e) => println!("daemon:  unreachable ({e})"),
    }
    Ok(())
}

/// `ariadne daemon status`
pub async fn daemon_status(client: &Client) -> Result<()> {
    match client.health().await {
        Ok(h) => {
            println!("status:  {}", h.status);
            println!("uptime:  {}s", h.uptime_secs);
            println!("socket:  {}", client.endpoint());
            Ok(())
        }
        Err(e) => bail!("daemon not running at {}: {e}", client.endpoint()),
    }
}

/// `ariadne daemon start` — spawn ariadned detached and wait for it to answer.
///
/// Builds its own client for `home` rather than taking the caller's: the
/// daemon it spawns listens on that home's socket, and `--host` /
/// `ARIADNE_SOCKET` — which are never passed to ariadned — would send both the
/// already-running check and the readiness poll at a different daemon.
pub async fn daemon_start(home: Option<PathBuf>) -> Result<()> {
    let client = Client::for_home(home.clone());
    if client.health().await.is_ok() {
        println!("daemon already running at {}", client.endpoint());
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
            println!("ariadned started (pid {})", child.id());
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
pub fn daemon_stop() -> Result<()> {
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
    println!("sent SIGTERM to ariadned (pid {pid})");
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

/// Resolve the ariadne home directory the same way the daemon does.
fn ariadne_home(home_override: Option<PathBuf>) -> PathBuf {
    endpoint::home(home_override).unwrap_or_else(|| PathBuf::from(".ariadne"))
}

/// Find the ariadned binary: next to the current executable, else on PATH.
fn find_ariadned() -> Result<PathBuf> {
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let sibling = dir.join("ariadned");
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    which("ariadned").context("ariadned not found next to ariadne or on PATH")
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|c| c.is_file())
}
