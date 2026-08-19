//! Daemon-side environment report: what `ariadned` sees of its own host.
//!
//! `ariadne doctor` asks the shell it runs in the same questions, and the two
//! answers differ more often than one would like: a launchd or systemd
//! service is handed the PATH baked into its service file at install time,
//! and an agent CLI installed afterwards is on the user's PATH and nowhere
//! else. Sessions are spawned by this process, so this is the answer that
//! decides whether a profile can run at all.

use std::ffi::OsStr;
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
    which_in(&std::env::var_os("PATH")?, name)
}

/// The same lookup against a given `PATH`, so it can be tested without
/// rewriting the environment of the process running the tests.
fn which_in(path: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// A file the daemon could actually launch.
///
/// Presence is not enough: a `claude` on PATH with no execute bit is a file,
/// not an agent, and reporting it as available would let a profile pinned to
/// it pass a check its sessions then fail.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // Follows symlinks on purpose: what matters is what running it reaches.
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
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

/// Whether a path is there, and whether this process can write it.
///
/// Both questions are answered without touching anything: `access(2)` asks
/// the kernel whether *this* process may write, which is the only way to get
/// a true answer. The permission bits cannot give one — a `0755` directory
/// belonging to somebody else has a write bit and is closed to us — and
/// neither can creating a file to see whether it can be created, which a
/// report has no business doing. A path that is not there yet asks the
/// question of the directory it would be created in.
fn path_state(path: &Path) -> PathStateDto {
    let exists = path.exists();
    let subject = match exists {
        true => Some(path),
        false => path.parent(),
    };
    PathStateDto {
        path: path.display().to_string(),
        exists,
        writable: subject.is_some_and(is_writable),
    }
}

/// Whether this process may write `path`, as the kernel sees it: ownership,
/// group membership, ACLs, a read-only mount and running as root all
/// included, none of which the mode bits alone would have shown.
///
/// `EACCESS` asks about the effective user — the one a spawn would run as —
/// rather than the real one, which is what `access(2)` would answer for.
fn is_writable(path: &Path) -> bool {
    rustix::fs::accessat(
        rustix::fs::CWD,
        path,
        rustix::fs::Access::WRITE_OK,
        rustix::fs::AtFlags::EACCESS,
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::fs::PermissionsExt;

    fn write(path: &Path, mode: u32) {
        std::fs::write(path, "").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A file on PATH that cannot be executed is not a binary anyone can
    /// launch, and a lookup that says otherwise sends a profile into a spawn
    /// that fails.
    #[test]
    fn a_file_with_no_execute_bit_is_not_executable() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("claude");
        write(&plain, 0o644);
        assert!(!is_executable(&plain));
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&plain));
    }

    /// A directory named like the binary is not the binary either.
    #[test]
    fn a_directory_is_never_executable() {
        let dir = tempfile::tempdir().unwrap();
        let named = dir.path().join("codex");
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
        write(&first.join("claude"), 0o644);
        write(&second.join("claude"), 0o755);

        let path = std::env::join_paths([&first, &second]).unwrap();
        assert_eq!(which_in(&path, "claude"), Some(second.join("claude")));
        // ...and with nothing runnable anywhere, nothing is reported.
        let only_first = std::env::join_paths([&first]).unwrap();
        assert_eq!(which_in(&only_first, "claude"), None);
    }

    /// The report reads the filesystem and leaves it exactly as it was — no
    /// probe file, and nothing removed to make room for one.
    #[test]
    fn reporting_on_a_directory_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let state = path_state(dir.path());
        assert!(state.exists);
        assert!(state.writable);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// A directory nobody at all can write.
    #[test]
    fn a_directory_with_no_write_bit_is_not_writable() {
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("worktrees");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root writes it regardless, and is right to be told so — which the
        // permission bits on their own would have got wrong.
        assert_eq!(
            path_state(&locked).writable,
            rustix::process::geteuid().is_root()
        );
        // Leave it removable by the temp dir's own cleanup.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The case the permission bits get exactly backwards: a directory whose
    /// *owner* may write it, where the owner is somebody else. `/usr` is
    /// root's and 0755, so it looks writable to anything reading modes and
    /// is closed to the daemon — which is what a `worktree_root` pointed at
    /// the wrong place looks like, and what doctor exists to catch.
    #[test]
    fn a_directory_owned_by_another_user_is_not_writable() {
        if rustix::process::geteuid().is_root() {
            return; // Root writes everything; there is nothing to tell apart.
        }
        let system = Path::new("/usr");
        let mode = std::fs::metadata(system).unwrap().permissions().mode();
        assert_ne!(mode & 0o200, 0, "/usr has an owner write bit");
        let state = path_state(system);
        assert!(state.exists);
        assert!(!state.writable, "0{mode:o}, and not ours to write");
    }

    /// An existing file is asked about as it stands, and asking writes
    /// nothing to it.
    #[test]
    fn an_existing_file_is_asked_about_as_it_stands() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("ariadne.db");
        write(&db, 0o644);
        let state = path_state(&db);
        assert!(state.exists);
        assert!(state.writable);
        assert_eq!(std::fs::read(&db).unwrap(), b"", "read, never written");

        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert_eq!(
            path_state(&db).writable,
            rustix::process::geteuid().is_root()
        );
    }

    /// A database the daemon has not created yet asks its directory instead:
    /// what matters is whether it can still be created.
    #[test]
    fn a_missing_file_falls_back_to_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let state = path_state(&dir.path().join("ariadne.db"));
        assert!(!state.exists);
        assert!(state.writable, "its directory takes the file");
    }

    /// ...and one in a directory that is not ours will not be created either.
    #[test]
    fn a_missing_file_in_a_closed_directory_is_not_writable() {
        if rustix::process::geteuid().is_root() {
            return;
        }
        let state = path_state(Path::new("/usr/ariadne.db"));
        assert!(!state.exists);
        assert!(!state.writable);
    }
}
