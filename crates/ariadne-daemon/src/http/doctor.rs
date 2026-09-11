//! Daemon-side environment report: what `ariadned` sees of its own host.
//!
//! `ariadne doctor` asks the shell it runs in the same questions, and the two
//! answers differ more often than one would like: a launchd or systemd
//! service is handed the PATH baked into its service file at install time,
//! and a binary installed afterwards is on the user's PATH and nowhere else.
//! Sessions and worktrees are made by this process, so this is the answer
//! that decides whether they can be made at all — which is why the PATH
//! searched here is this process's own, and why [`ariadne_core::probe`]
//! takes it as a parameter instead of reading one. Which agents a session can
//! run on is what the registry's discovery found (`acp_agents`).

use std::ffi::OsStr;
use std::path::Path;

use axum::Json;
use axum::extract::State;
use futures_util::future::join_all;

use ariadne_api::doctor::{BinaryDto, DaemonReportDto, PathStateDto};
use ariadne_core::probe;

use super::AppState;

/// The binaries the daemon runs, with the flag each prints its version for
/// and whether it holds credentials worth asking about.
///
/// git is what a worktree is made of; `gh` and `glab` are how a published
/// task is watched, and they are here because a forge CLI that is missing or
/// signed out fails every poll of a pull request in a way nothing else in the
/// system shows — the task simply sits there looking watched.
const TOOLS: [(&str, &str, bool); 3] = [
    ("git", "--version", false),
    ("gh", "--version", true),
    ("glab", "--version", true),
];

/// What the daemon sees: its PATH, the binaries on it, and the state of the
/// directories it works in.
#[utoipa::path(get, path = "/v1/doctor", tag = "system",
    responses((status = 200, description = "The daemon's own environment", body = DaemonReportDto)))]
pub async fn report(State(state): State<AppState>) -> Json<DaemonReportDto> {
    let cfg = &state.launcher.cfg;
    let path = std::env::var_os("PATH");
    let path = path.as_deref();

    let tools = join_all(
        TOOLS
            .into_iter()
            .map(|(name, flag, authenticates)| report_on(path, name, flag, authenticates)),
    )
    .await;

    Json(DaemonReportDto {
        version: env!("CARGO_PKG_VERSION").into(),
        path: std::env::var("PATH").ok(),
        home: cfg.root.display().to_string(),
        socket_path: cfg.socket_path.display().to_string(),
        acp_agents: state.agent_registry.agents().await,
        tools,
        db: path_state(&cfg.db_path),
        worktree_root: path_state(&cfg.worktree_root),
    })
}

/// Find a binary on the daemon's PATH and ask it the questions that apply to
/// it: its version, and — for the ones that sign in to anything — whether it
/// is signed in. Both at once, because both spawn a process and a report is a
/// request somebody is waiting on. A binary that is not there is asked
/// nothing, and so is a daemon running without a PATH at all.
async fn report_on(
    path: Option<&OsStr>,
    name: &str,
    version_flag: &str,
    authenticates: bool,
) -> BinaryDto {
    let found = path.and_then(|path| probe::which(path, name));
    let (version, authenticated) = match &found {
        Some(binary) => tokio::join!(probe::probe_version(binary, version_flag), async {
            match authenticates {
                true => probe::probe_auth(binary).await,
                false => None,
            }
        }),
        None => (None, None),
    };
    BinaryDto {
        name: name.to_string(),
        path: found.map(|p| p.display().to_string()),
        version,
        authenticated,
    }
}

fn path_state(path: &Path) -> PathStateDto {
    let probe::PathState { exists, writable } = probe::path_state(path);
    PathStateDto {
        path: path.display().to_string(),
        exists,
        writable,
    }
}
