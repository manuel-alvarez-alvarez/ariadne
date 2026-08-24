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

use rustix::fs::{Access, AtFlags};

use ariadne_api::doctor::{BinaryDto, DaemonReportDto, PathStateDto};
use ariadne_core::AgentKind;

use super::AppState;

/// How long a `--version` may take before we stop waiting for it. A hung or
/// half-installed binary must not hang the report.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// The non-agent binaries the daemon runs, with the flag each prints its
/// version for (tmux has never spelled it `--version`) and whether it holds
/// credentials worth asking about.
///
/// tmux and git are what a session is made of; `gh` and `glab` are how a
/// published task is watched, and they are here because a forge CLI that is
/// missing or signed out fails every poll of a pull request in a way nothing
/// else in the system shows — the task simply sits there looking watched.
const TOOLS: [(&str, &str, bool); 4] = [
    ("tmux", "-V", false),
    ("git", "--version", false),
    ("gh", "--version", true),
    ("glab", "--version", true),
];

/// What both forge CLIs answer "am I signed in?" with, by their exit status.
const AUTH_STATUS: [&str; 2] = ["auth", "status"];

/// What the daemon sees: its PATH, the binaries on it, and the state of the
/// directories it works in.
#[utoipa::path(get, path = "/v1/doctor", tag = "system",
    responses((status = 200, description = "The daemon's own environment", body = DaemonReportDto)))]
pub async fn report(State(state): State<AppState>) -> Json<DaemonReportDto> {
    let cfg = &state.launcher.cfg;

    let agents = join_all(
        AgentKind::ALL
            .into_iter()
            .map(|kind| probe(kind.binary(), "--version", Some(kind), false)),
    );
    let tools = join_all(
        TOOLS
            .into_iter()
            .map(|(name, flag, authenticates)| probe(name, flag, None, authenticates)),
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

/// Find a binary on the daemon's PATH, ask it for its version, and — for the
/// ones that sign in to anything — whether it is signed in.
///
/// Fail-soft throughout: a binary that is missing, refuses to run or never
/// answers is reported as such and stops nothing.
async fn probe(
    name: &str,
    version_flag: &str,
    agent_kind: Option<AgentKind>,
    authenticates: bool,
) -> BinaryDto {
    let path = which(name);
    let (version, authenticated) = match &path {
        Some(path) => tokio::join!(
            probe_version(path, version_flag),
            probe_auth(path, authenticates),
        ),
        None => (None, None),
    };
    BinaryDto {
        name: name.to_string(),
        agent_kind,
        path: path.map(|p| p.display().to_string()),
        version,
        authenticated,
    }
}

/// Whether a forge CLI holds credentials, as it answers `auth status`.
///
/// The exit status is the whole answer — both CLIs write their account, host
/// and token scopes to stderr, which is more than a report wants and none of
/// it a thing to record — and a probe that timed out is no answer at all
/// rather than a "no": a `gh` waiting on a network is not a `gh` signed out.
async fn probe_auth(binary: &Path, authenticates: bool) -> Option<bool> {
    if !authenticates {
        return None;
    }
    let status = tokio::time::timeout(
        PROBE_TIMEOUT,
        tokio::process::Command::new(binary)
            .args(AUTH_STATUS)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?
    .status;
    Some(status.success())
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
    let writable = match (exists, path.is_dir()) {
        (true, true) => takes_new_entries(path),
        (true, false) => has_access(path, Access::WRITE_OK),
        // Not there yet: whether it can be created is its directory's answer.
        (false, _) => path.parent().is_some_and(takes_new_entries),
    };
    PathStateDto {
        path: path.display().to_string(),
        exists,
        writable,
    }
}

/// Whether a worktree or a database file could be created in this directory.
///
/// Writing an entry into a directory is a lookup followed by a write, so it
/// takes search permission as well as write — a directory with `w` and no
/// `x` accepts nothing, however writable its mode looks.
fn takes_new_entries(dir: &Path) -> bool {
    has_access(dir, Access::WRITE_OK | Access::EXEC_OK)
}

/// What the kernel says this process may do with `path`: ownership, group
/// membership, ACLs, a read-only mount and running as root all included,
/// none of which the mode bits alone would have shown.
///
/// `EACCESS` asks about the effective user — the one a spawn would run as —
/// rather than the real one, which is what `access(2)` answers for.
fn has_access(path: &Path, access: Access) -> bool {
    rustix::fs::accessat(rustix::fs::CWD, path, access, AtFlags::EACCESS).is_ok()
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

    /// The only question about a forge CLI that the version does not answer:
    /// an installed `gh` nobody signed in fails every poll of a pull request,
    /// and looks from the outside exactly like one that works.
    #[tokio::test]
    async fn a_forge_cli_is_asked_whether_it_is_signed_in() {
        let dir = tempfile::tempdir().unwrap();
        let signed_in = dir.path().join("gh");
        let signed_out = dir.path().join("glab");
        std::fs::write(&signed_in, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(&signed_out, "#!/bin/sh\necho 'not logged in' >&2\nexit 1\n").unwrap();
        for bin in [&signed_in, &signed_out] {
            std::fs::set_permissions(bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(probe_auth(&signed_in, true).await, Some(true));
        assert_eq!(probe_auth(&signed_out, true).await, Some(false));
        // And nothing at all is asked of a binary with nothing to sign in to.
        assert_eq!(probe_auth(&signed_out, false).await, None);
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

    /// Write permission on a directory is not enough to put anything in it:
    /// creating an entry is a lookup followed by a write, so a directory
    /// with `w` and no `x` takes nothing, however writable its mode reads.
    #[test]
    fn a_directory_that_cannot_be_searched_takes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let unsearchable = dir.path().join("worktrees");
        std::fs::create_dir(&unsearchable).unwrap();
        std::fs::set_permissions(&unsearchable, std::fs::Permissions::from_mode(0o600)).unwrap();
        // Root searches anything, and is right to be told so.
        let expected = rustix::process::geteuid().is_root();
        assert_eq!(path_state(&unsearchable).writable, expected);
        // ...and neither is a database going to appear inside it.
        assert_eq!(
            path_state(&unsearchable.join("ariadne.db")).writable,
            expected
        );
        std::fs::set_permissions(&unsearchable, std::fs::Permissions::from_mode(0o755)).unwrap();
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
