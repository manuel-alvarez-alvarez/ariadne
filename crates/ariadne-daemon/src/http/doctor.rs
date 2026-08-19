//! Daemon-side environment report: what `ariadned` sees of its own host.
//!
//! `ariadne doctor` asks the shell it runs in the same questions, and the two
//! answers differ more often than one would like: a launchd or systemd
//! service is handed the PATH baked into its service file at install time,
//! and an agent CLI installed afterwards is on the user's PATH and nowhere
//! else. Sessions are spawned by this process, so this is the answer that
//! decides whether a profile can run at all.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use futures_util::future::join_all;

use ariadne_api::doctor::{BinaryDto, DaemonReportDto, PathStateDto};
use ariadne_core::AgentKind;

use super::AppState;

/// How long a `--version` may take before we stop waiting for it. A hung or
/// half-installed binary must not hang the report.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// The non-agent binaries a session needs, with the flag each prints its
/// version for (tmux has never spelled it `--version`).
const TOOLS: [(&str, &str); 2] = [("tmux", "-V"), ("git", "--version")];

/// What the daemon sees: its PATH, the binaries on it, and the state of the
/// directories it works in.
#[utoipa::path(get, path = "/v1/doctor", tag = "system",
    responses((status = 200, description = "The daemon's own environment", body = DaemonReportDto)))]
pub async fn report(State(state): State<AppState>) -> Json<DaemonReportDto> {
    let cfg = &state.launcher.cfg;

    let agents = join_all(
        AgentKind::ALL
            .into_iter()
            .map(|kind| probe(kind.binary(), "--version", Some(kind))),
    );
    let tools = join_all(
        TOOLS
            .into_iter()
            .map(|(name, flag)| probe(name, flag, None)),
    );
    let (agents, tools) = tokio::join!(agents, tools);

    Json(DaemonReportDto {
        version: env!("CARGO_PKG_VERSION").into(),
        path: std::env::var("PATH").ok(),
        home: cfg.root.display().to_string(),
        socket_path: cfg.socket_path.display().to_string(),
        agents,
        tools,
        db: path_state(&cfg.db_path),
        worktree_root: path_state(&cfg.worktree_root),
    })
}

/// Find a binary on the daemon's PATH and ask it for its version.
///
/// Fail-soft throughout: a binary that is missing, refuses to run or never
/// answers is reported as such and stops nothing.
async fn probe(name: &str, version_flag: &str, agent_kind: Option<AgentKind>) -> BinaryDto {
    let path = which(name);
    let version = match &path {
        Some(path) => probe_version(path, version_flag).await,
        None => None,
    };
    BinaryDto {
        name: name.to_string(),
        agent_kind,
        path: path.map(|p| p.display().to_string()),
        version,
    }
}

/// First entry of `PATH` holding an executable of that name.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The first line of `<binary> <flag>`, bounded by [`PROBE_TIMEOUT`].
///
/// `kill_on_drop` matters as much as the timeout: without it the process
/// outlives the request that gave up on it.
async fn probe_version(binary: &Path, flag: &str) -> Option<String> {
    let output = tokio::time::timeout(
        PROBE_TIMEOUT,
        tokio::process::Command::new(binary)
            .arg(flag)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    // Most CLIs answer on stdout; the ones that answer on stderr are still
    // answering.
    let text = match output.stdout.is_empty() {
        true => String::from_utf8_lossy(&output.stderr),
        false => String::from_utf8_lossy(&output.stdout),
    };
    let line = text.lines().next().unwrap_or_default().trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Whether a path is there and whether the daemon's user can write it.
///
/// Writability is tested rather than read off the permission bits: those say
/// what the owner may do, and the daemon is not necessarily the owner. For a
/// directory that means creating a probe file and removing it again — the one
/// thing in the whole report that touches the filesystem, and it leaves it as
/// it found it.
fn path_state(path: &Path) -> PathStateDto {
    let exists = path.exists();
    let writable = if path.is_dir() {
        dir_is_writable(path)
    } else if exists {
        std::fs::OpenOptions::new().write(true).open(path).is_ok()
    } else {
        // Not there yet: what matters is whether it can be created.
        path.parent().is_some_and(dir_is_writable)
    };
    PathStateDto {
        path: path.display().to_string(),
        exists,
        writable,
    }
}

fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(".ariadne-doctor-write-probe");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        // A probe file left behind by a report that died between creating and
        // removing it: the directory was writable then, and nothing about a
        // stale file says it no longer is.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
