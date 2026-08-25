//! Finding and interrogating the binaries Ariadne runs.
//!
//! Both `ariadne doctor` and the daemon's `/v1/doctor` ask the same questions
//! of the same binaries, and used to ask them twice. The `PATH` searched is a
//! parameter rather than something read from the environment here, because
//! the two answers legitimately differ: a launchd or systemd service carries
//! the `PATH` its service file was written with, so an agent CLI installed
//! afterwards is on the user's `PATH` and invisible to the process that
//! spawns sessions.
//!
//! Everything here is fail-soft. A binary that is missing, refuses to run or
//! never answers is reported as such and stops nothing.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use rustix::fs::{Access, AtFlags};

/// How long any probe may take before we stop waiting for it. A hung or
/// half-installed binary must not hang a report.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// What both forge CLIs answer "am I signed in?" with, by their exit status.
const AUTH_STATUS: [&str; 2] = ["auth", "status"];

/// First entry of `path` holding an executable of that name.
pub fn which(path: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// A file that can actually be run.
///
/// Presence is not enough: a `claude` on `PATH` with no execute bit is a
/// file, not an agent, and reporting it as available would let a profile
/// pinned to it pass a check its sessions then fail.
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // Follows symlinks on purpose: what matters is what running it reaches.
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The first line of `<binary> <flag>`, bounded by [`PROBE_TIMEOUT`].
pub async fn probe_version(binary: &Path, flag: &str) -> Option<String> {
    let output = run(binary.as_os_str(), std::slice::from_ref(&flag)).await?;
    // Most CLIs answer on stdout; the ones that answer on stderr are still
    // answering.
    let text = match output.stdout.is_empty() {
        true => String::from_utf8_lossy(&output.stderr),
        false => String::from_utf8_lossy(&output.stdout),
    };
    let line = text.lines().next().unwrap_or_default().trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Whether a forge CLI holds credentials, as it answers `auth status`.
///
/// The exit status is the whole answer — both CLIs write their account, host
/// and token scopes to stderr, which is more than a report wants and none of
/// it a thing to record — and a probe that timed out is no answer at all
/// rather than a "no": a `gh` waiting on a network is not a `gh` signed out,
/// and reporting it as one would send somebody to sign in again for nothing.
pub async fn probe_auth(binary: &Path) -> Option<bool> {
    Some(
        run(binary.as_os_str(), &AUTH_STATUS)
            .await?
            .status
            .success(),
    )
}

/// Run a command only for its exit status, bounded by [`PROBE_TIMEOUT`].
/// A probe that never answered counts as a failure.
pub async fn probe_status(program: impl AsRef<OsStr>, args: &[&str]) -> bool {
    run(program.as_ref(), args)
        .await
        .is_some_and(|o| o.status.success())
}

/// Run a program to completion under [`PROBE_TIMEOUT`], or give up on it.
///
/// `kill_on_drop` matters as much as the timeout: without it a binary that
/// hangs outlives the report that gave up on it.
async fn run(program: &OsStr, args: &[&str]) -> Option<Output> {
    tokio::time::timeout(
        PROBE_TIMEOUT,
        tokio::process::Command::new(program)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()
}

/// Whether a path is there, and whether this process can write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathState {
    pub exists: bool,
    pub writable: bool,
}

/// Ask the kernel about `path`, without touching anything.
///
/// `access(2)` is the only way to a true answer: the permission bits cannot
/// give one — a `0755` directory belonging to somebody else has a write bit
/// and is closed to us — and neither can creating a file to see whether it
/// can be created, which a report has no business doing. A path that is not
/// there yet asks the question of the directory it would be created in.
pub fn path_state(path: &Path) -> PathState {
    let exists = path.exists();
    let writable = match (exists, path.is_dir()) {
        (true, true) => takes_new_entries(path),
        (true, false) => has_access(path, Access::WRITE_OK),
        (false, _) => path.parent().is_some_and(takes_new_entries),
    };
    PathState { exists, writable }
}

/// Whether a worktree or a database file could be created in this directory.
///
/// Writing an entry into a directory is a lookup followed by a write, so it
/// takes search permission as well as write — a directory with `w` and no
/// `x` accepts nothing, however writable its mode looks.
fn takes_new_entries(dir: &Path) -> bool {
    has_access(dir, Access::WRITE_OK | Access::EXEC_OK)
}

/// `EACCESS` asks about the effective user — the one a spawn would run as —
/// rather than the real one, which is what `access(2)` answers for.
fn has_access(path: &Path, access: Access) -> bool {
    rustix::fs::accessat(rustix::fs::CWD, path, access, AtFlags::EACCESS).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::fs::PermissionsExt;

    /// An executable shell script that runs `body`.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn plain(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        path
    }

    /// A file on PATH that cannot be executed is not a binary anyone can
    /// launch, and a lookup that says otherwise sends a profile into a spawn
    /// that fails.
    #[test]
    fn only_a_file_with_an_execute_bit_is_executable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_executable(&plain(dir.path(), "claude")));
        assert!(is_executable(&script(dir.path(), "codex", "exit 0")));
        // A directory of that name, and a name with nothing on it at all.
        assert!(!is_executable(dir.path()));
        assert!(!is_executable(&dir.path().join("nothing")));
    }

    /// The lookup walks the given PATH in order and takes the first entry it
    /// could actually run, skipping the directories that only look right.
    #[test]
    fn the_lookup_takes_the_first_runnable_entry_of_the_path() {
        let empty = tempfile::tempdir().unwrap();
        let decoy = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        plain(decoy.path(), "claude");
        let wanted = script(real.path(), "claude", "exit 0");

        let path = std::env::join_paths([empty.path(), decoy.path(), real.path()]).unwrap();
        assert_eq!(which(&path, "claude"), Some(wanted));
        assert_eq!(which(&path, "codex"), None);
        assert_eq!(which(OsStr::new(""), "claude"), None);
    }

    /// Everything that runs a binary, in one test on one runtime: a `tokio`
    /// runtime per test means one reaping a child another one's `SIGCHLD`
    /// woke, and a probe that waits out its timeout for no reason.
    ///
    /// Only binaries every unix has, for the same reason: a script written
    /// and immediately executed is its own race.
    #[tokio::test]
    async fn probes_run_the_binary_and_read_what_it_answers() {
        let echo = Path::new("/bin/echo");
        let gone = Path::new("/nonexistent/gh");

        // The first line of stdout, and the flag reaching the binary as
        // written — tmux has never spelled it `--version`.
        assert_eq!(probe_version(echo, "1.2.3").await.as_deref(), Some("1.2.3"));
        assert_eq!(probe_version(echo, "-V").await.as_deref(), Some("-V"));
        // The ones that answer on stderr are still answering.
        let complaint = probe_version(Path::new("/bin/cat"), "/nonexistent/file").await;
        assert!(
            complaint.is_some_and(|line| line.contains("/nonexistent/file")),
            "the stderr line is the answer when stdout is empty"
        );
        // Answering with nothing is not an answer, and neither is not being
        // there: both leave the version unknown rather than empty.
        assert_eq!(probe_version(echo, "").await, None);
        assert_eq!(probe_version(gone, "--version").await, None);

        // Auth is the exit status of `auth status` and nothing else; a
        // binary that is not there is not a binary that is signed out.
        assert_eq!(probe_auth(Path::new("/usr/bin/true")).await, Some(true));
        assert_eq!(probe_auth(Path::new("/usr/bin/false")).await, Some(false));
        assert_eq!(probe_auth(gone).await, None);

        assert!(probe_status("/bin/sh", &["-c", "exit 0"]).await);
        assert!(!probe_status("/bin/sh", &["-c", "exit 3"]).await);
        assert!(!probe_status("/nonexistent/launchctl", &["list"]).await);
    }

    /// A binary that never answers must not hang the report it was asked
    /// for: the wait is bounded, and what comes back is "no answer".
    #[tokio::test(start_paused = true)]
    async fn a_probe_that_hangs_is_given_up_on() {
        let sleep = Path::new("/bin/sleep");
        assert_eq!(probe_version(sleep, "600").await, None);
        assert!(!probe_status(sleep, &["600"]).await);
    }

    #[test]
    fn a_directory_that_exists_and_takes_entries_is_writable() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            path_state(dir.path()),
            PathState {
                exists: true,
                writable: true
            }
        );
    }

    /// The question a not-yet-created database asks is whether it could be
    /// created, which is its directory's answer and not its own.
    #[test]
    fn a_path_that_is_not_there_yet_answers_from_its_parent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            path_state(&dir.path().join("ariadne.db")),
            PathState {
                exists: false,
                writable: true
            }
        );
        assert_eq!(
            path_state(&dir.path().join("gone/ariadne.db")),
            PathState {
                exists: false,
                writable: false
            }
        );
    }

    /// A file that exists is asked about itself, not about its directory: a
    /// read-only database in a writable directory is not writable.
    #[test]
    fn a_file_is_asked_about_itself() {
        let dir = tempfile::tempdir().unwrap();
        let db = plain(dir.path(), "ariadne.db");
        assert!(path_state(&db).writable);
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert_eq!(
            path_state(&db),
            PathState {
                exists: true,
                writable: false
            }
        );
    }

    /// A directory with a write bit and no search bit accepts nothing, which
    /// is the case the mode bits alone would have got wrong.
    #[test]
    fn a_directory_that_cannot_be_searched_takes_no_entries() {
        if rustix::process::geteuid().is_root() {
            // root bypasses the check the test is about.
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let closed = dir.path().join("closed");
        std::fs::create_dir(&closed).unwrap();
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!path_state(&closed).writable);
        assert!(!path_state(&closed.join("db")).writable);
        // Leave it removable by the tempdir drop.
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
