//! The ACP adapter: translate an Ariadne spawn/resume request into the
//! launch of one ACP agent — the flags behind its registry command, the
//! environment, and the launch file the ACP runtime drives it from.

mod acp;
mod handoff;
pub mod prompts;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use ariadne_core::Seat;
use ariadne_core::acp::LaunchConfig;
use ariadne_store::{Store, skill_name_rejection};

pub use acp::{plan_resume, plan_spawn};
#[allow(unused_imports)] // the session-switch task wires handoff_text to its endpoint
pub(crate) use handoff::handoff_text;

/// Everything an adapter needs to plan a spawn. Prompt assembly happens in
/// the launcher; adapters only deal with delivery mechanics.
#[derive(Debug, Clone)]
pub struct SpawnCtx {
    /// Ariadne agent-session id (becomes ARIADNE_SESSION_ID).
    pub session_id: String,
    /// This launch of that session (becomes ARIADNE_LAUNCH_ID): fresh for
    /// every process started under the row, so that what this agent reports
    /// is told apart from what the agent it replaces is still reporting.
    pub launch_id: String,
    /// The goal the session works for (becomes ARIADNE_GOAL_ID); None for a
    /// pull request session, which works for no goal.
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    /// The pull request a pull request session watches (becomes
    /// ARIADNE_PULL_REQUEST_ID, 026).
    pub pull_request_id: Option<String>,
    pub seat: Seat,
    /// Per-session directory for generated files (`~/.ariadne/run/<id>/`).
    pub run_dir: PathBuf,
    /// Where the agent process runs: worktree (author/reviewer) or repo
    /// (orchestrator).
    pub cwd: PathBuf,
    pub socket_path: PathBuf,
    /// Path or name of the `ariadne` CLI binary (the MCP entry point).
    pub cli_bin: String,
    /// What the agent is briefed as: what its seat owes, and the index of the
    /// skills it loads.
    pub system_prompt: String,
    /// The index of the skills it loads, alone: the MCP server puts it in
    /// its instructions after the seat text (006). Empty for no skill.
    pub skill_index: String,
    /// Where this agent's skill documents are, one `<name>/SKILL.md` under it.
    /// The launcher links it to the shared home copy ([`link_skills`]) before
    /// the adapter plans anything, so the path the system prompt's index
    /// names resolves to the document, and an adapter only has to point its
    /// CLI at the directory. The launcher always links it, staffed with a
    /// skill or not; `None` is only a hand-built context that names no
    /// directory at all, as a hermetic test does.
    pub skills_dir: Option<PathBuf>,
    /// Task/goal briefing delivered as the first user prompt.
    pub initial_prompt: String,
    /// The model the session is pinned to, as the agent itself names it —
    /// the bare half of the pin, after the registry id. Always passed on: a
    /// launch never falls back to an agent default.
    pub model: String,
    /// The effort that model is run at, as the session pinned it. None = the
    /// agent's own default.
    pub effort: Option<String>,
    /// The agent's configured flags, read from the database on every launch
    /// and appended to its registry command. Everything structural — the
    /// session, the MCP server, the system prompt, the model and its effort —
    /// travels in the launch file instead.
    pub extra_flags: Vec<String>,
}

/// Link an agent's run-dir `skills` folder onto the daemon's shared home
/// copy ([`sync_skills`]), and answer with the directory the link resolves
/// to.
///
/// The home copy holds `<name>/SKILL.md` for every skill the store knows, so
/// an agent that opens a path under the link reads the home copy's current
/// text — including a skill reworded since the task was staffed, the same
/// way every other default text reaches the next launch.
///
/// Relinked on every launch rather than left alone, so a second launch of a
/// session never reads a stale target. On macOS and Linux this is always a
/// symlink; on Windows it is a symlink where the platform grants one, or
/// else a copy of the whole home folder ([`copy_skills_dir`]).
///
/// `skills_dir` is resolved to an absolute path first: a symlink's relative
/// target is read against the link's own directory, not against wherever a
/// relative `skills_dir` in `config.toml` was resolved from, so writing it
/// in unchanged would point every session's link somewhere under its own
/// run directory instead of at the home copy.
pub fn link_skills(run_dir: &Path, skills_dir: &Path) -> Result<PathBuf> {
    let root = run_dir.join("skills");
    clear_skills_entry(&root)?;
    std::fs::create_dir_all(run_dir).with_context(|| format!("creating {}", run_dir.display()))?;
    let skills_dir = std::fs::canonicalize(skills_dir)
        .with_context(|| format!("resolving {}", skills_dir.display()))?;
    link_or_copy(&skills_dir, &root)?;
    Ok(root)
}

#[cfg(unix)]
fn link_or_copy(skills_dir: &Path, root: &Path) -> Result<()> {
    std::os::unix::fs::symlink(skills_dir, root)
        .with_context(|| format!("linking {} to {}", root.display(), skills_dir.display()))
}

#[cfg(windows)]
fn link_or_copy(skills_dir: &Path, root: &Path) -> Result<()> {
    if std::os::windows::fs::symlink_dir(skills_dir, root).is_ok() {
        return Ok(());
    }
    copy_skills_dir(skills_dir, root)
}

/// Clear a stale `skills` entry in the run dir before relinking it.
///
/// A link is removed as a link — never walked through to delete the shared
/// copy it points at. A real directory or file, left behind by the Windows
/// copy fallback or by anything else that wrote there, is removed outright.
fn clear_skills_entry(root: &Path) -> Result<()> {
    let meta = match std::fs::symlink_metadata(root) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", root.display())),
    };
    if meta.is_symlink() {
        std::fs::remove_file(root)
            .or_else(|_| std::fs::remove_dir(root))
            .with_context(|| format!("removing the link at {}", root.display()))
    } else if meta.is_dir() {
        std::fs::remove_dir_all(root).with_context(|| format!("clearing {}", root.display()))
    } else {
        std::fs::remove_file(root).with_context(|| format!("clearing {}", root.display()))
    }
}

/// Copy the whole `skills_dir` tree into `dest`, for a Windows account the
/// platform refuses a directory symlink: an unprivileged one without
/// Developer Mode. Every file a skill's document names — a script beside
/// its `SKILL.md`, say — reaches the run dir the same way the document does.
pub fn copy_skills_dir(skills_dir: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    for entry in std::fs::read_dir(skills_dir)
        .with_context(|| format!("reading {}", skills_dir.display()))?
    {
        let entry = entry.with_context(|| format!("reading {}", skills_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading {}", entry.path().display()))?;
        let target = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_skills_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Serializes a skill mutation with the sync that follows it.
///
/// Two concurrent mutations each call the store, then `sync_skills`. Without
/// a shared lock, one call's directory removal can run between the other
/// call's store read and its write, deleting a skill the store still holds.
/// Holding this lock across a mutation and its sync rules that out: the two
/// pairs run one after the other, never interleaved.
pub static SKILLS_SYNC_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Rewrite the shared home copy of every skill the store holds.
///
/// A directory under `skills_dir` belongs to a skill only when its name is in
/// the store. Files directly under the root are left alone, and the root
/// itself is never removed.
pub async fn sync_skills(store: &Store, skills_dir: &Path) -> Result<()> {
    let skills = store.list_skills().await?;
    std::fs::create_dir_all(skills_dir)
        .with_context(|| format!("creating {}", skills_dir.display()))?;
    // Resolved once: a symlink in `skills_dir` itself is settled before any
    // name's own path is checked against it, rather than on every check.
    let canonical_skills_dir = std::fs::canonicalize(skills_dir)
        .with_context(|| format!("resolving {}", skills_dir.display()))?;

    let names: std::collections::HashSet<_> =
        skills.iter().map(|skill| skill.name.as_str()).collect();
    for entry in std::fs::read_dir(&canonical_skills_dir)
        .with_context(|| format!("reading {}", canonical_skills_dir.display()))?
    {
        let entry = entry.with_context(|| format!("reading {}", canonical_skills_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading {}", entry.path().display()))?;
        // A symlink is never a skill's own directory: every one of those is
        // made by `create_dir_all` below, which never makes one. Whatever
        // put a symlink here instead, following it — to decide whether its
        // name is still a skill's, or to write through it below — would
        // reach wherever it points, outside `skills_dir` included. It is
        // removed on sight, named or not; `remove_dir_all` on a symlink
        // unlinks the symlink alone, never descends into its target.
        let stale = file_type.is_symlink()
            || (file_type.is_dir()
                && !names.contains(entry.file_name().to_string_lossy().as_ref()));
        if stale {
            std::fs::remove_dir_all(entry.path())
                .with_context(|| format!("removing {}", entry.path().display()))?;
        }
    }

    // Every skill is attempted whatever an earlier one did: one name's
    // write failing is no reason to leave the rest unsynced. A failure is
    // still reported, after every attempt — a sync that answered `Ok` once
    // is read as "every skill here is on disk", by the HTTP route's own
    // caller as much as by a later run of this same function.
    let mut failed = Vec::new();
    for skill in skills {
        if let Some(reason) = skill_name_rejection(&skill.name, &canonical_skills_dir) {
            tracing::error!(name = %skill.name, reason, "a stored skill name is invalid; not writing it");
            continue;
        }
        if let Err(error) =
            symlink_safe::write_skill(&canonical_skills_dir, &skill.name, skill.document_text())
        {
            tracing::error!(name = %skill.name, error = %error, "writing the skill failed");
            failed.push(skill.name);
        }
    }
    if !failed.is_empty() {
        bail!(
            "writing the skill document failed for: {}",
            failed.join(", ")
        );
    }
    Ok(())
}

/// Create and write a skill's directory and `SKILL.md`, a symlink at either
/// refused rather than followed, with no gap between refusing one and
/// writing: a path-based check followed by a path-based write — `stat`
/// `skills_dir/name`, then separately open `skills_dir/name/SKILL.md` — reads
/// `name` off the directory twice, and a symlink dropped in between is
/// exactly the window the second read falls through. Every step below opens
/// its own name directly off a directory already held open by its file
/// descriptor, which a path swap after that cannot redirect: the descriptor
/// keeps pointing at the entry it was opened for, nothing else sees the open
/// call that follows it, and `O_NOFOLLOW` refuses a symlink there outright
/// rather than opening through it.
#[cfg(unix)]
mod symlink_safe {
    use std::fs::File;
    use std::io::{self, Write};
    use std::os::fd::OwnedFd;
    use std::path::Path;

    use rustix::fs::{AtFlags, Mode, OFlags, mkdirat, openat, renameat, unlinkat};
    use rustix::io::Errno;

    /// Open `path` itself as a directory, refusing a symlink at it.
    fn open_dir_no_follow(path: &Path) -> Result<OwnedFd, Errno> {
        rustix::fs::open(
            path,
            OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
    }

    /// Open `name` directly under the directory `parent` refers to, never
    /// resolving `parent`'s own path again: refusing a symlink at `name`,
    /// rather than following it, is the one purpose of every call site here.
    fn openat_no_follow(parent: &OwnedFd, name: &str, flags: OFlags) -> Result<OwnedFd, Errno> {
        openat(
            parent,
            name,
            flags | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o644),
        )
    }

    /// Open `name` as a directory directly under `parent`, making it first
    /// if there is nothing there, and clearing it first where something
    /// that is not a plain directory sits there instead — the same entry
    /// `sync_skills`'s own cleanup pass would already have removed, had it
    /// arrived before this call rather than after it.
    ///
    /// `O_DIRECTORY | O_NOFOLLOW` against a symlink answers `ELOOP` on
    /// Linux; on macOS the same open answers `ENOTDIR` instead, even where
    /// the symlink's own target is a directory — the two kernels disagree
    /// on which error a refused symlink gets, not on refusing it. `ENOTDIR`
    /// is also what a plain file at `name` answers, which this clears the
    /// same way: neither is ever what a skill's own directory should hold.
    fn open_or_recreate_dir(parent: &OwnedFd, name: &str) -> Result<OwnedFd, Errno> {
        let flags = OFlags::DIRECTORY;
        match openat_no_follow(parent, name, flags) {
            Ok(fd) => Ok(fd),
            Err(Errno::LOOP | Errno::NOTDIR) => {
                unlinkat(parent, name, AtFlags::empty())?;
                mkdirat(parent, name, Mode::from_raw_mode(0o755))?;
                openat_no_follow(parent, name, flags)
            }
            Err(Errno::NOENT) => {
                mkdirat(parent, name, Mode::from_raw_mode(0o755))?;
                openat_no_follow(parent, name, flags)
            }
            Err(e) => Err(e),
        }
    }

    /// An unpredictable name component for the temporary file below: a
    /// name built from the daemon's own PID, or any other value an outside
    /// caller could derive, lets that caller plant a file under the exact
    /// name first — `O_EXCL` then refuses the write outright, and the
    /// plant is the whole attack, nothing further needed. Sixteen random
    /// hex characters are not worth guessing.
    fn random_suffix() -> String {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("the OS random source is available");
        hex::encode(bytes)
    }

    /// How many unpredictable names `write_document_atomically` tries
    /// before giving up: one collision is bad luck, this many in a row
    /// means something else is wrong.
    const TEMP_NAME_ATTEMPTS: u32 = 8;

    /// Write `document` as a file of its own directly under `dir`, then
    /// `renameat` it onto `SKILL.md`, replacing whatever was there in one
    /// step rather than opening that name at all.
    ///
    /// `SKILL.md` existing as a hard link — sharing its data with some
    /// other name entirely, outside `skills_dir` as easily as in it — is
    /// not a symlink, and `O_NOFOLLOW` does nothing for it: opening it
    /// with `O_TRUNC` would empty the data every one of its names shares,
    /// written outside `skills_dir` from a name inside it. Writing a fresh
    /// file under a name of its own first, then swapping it onto
    /// `SKILL.md` by name alone, never opens the old entry either way —
    /// hard link, symlink or a plain file already there — so nothing of
    /// its data is ever touched.
    ///
    /// The temporary name is unpredictable and `O_EXCL`'d, so a name
    /// planted there first is refused rather than opened; a refusal tries
    /// a fresh name rather than giving up on the first collision. Every
    /// error after the temporary file exists unlinks it before returning,
    /// so a write or a rename that fails never leaves it behind.
    fn write_document_atomically(dir: &OwnedFd, document: &str) -> io::Result<()> {
        for _ in 0..TEMP_NAME_ATTEMPTS {
            let temp_name = format!(".skill-{}.tmp", random_suffix());
            let temp = match openat_no_follow(
                dir,
                &temp_name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
            ) {
                Ok(fd) => fd,
                Err(Errno::EXIST) => continue,
                Err(e) => return Err(e.into()),
            };
            if let Err(error) = File::from(temp).write_all(document.as_bytes()) {
                let _ = unlinkat(dir, &temp_name, AtFlags::empty());
                return Err(error);
            }
            return match renameat(dir, temp_name.as_str(), dir, "SKILL.md") {
                Ok(()) => Ok(()),
                Err(error) => {
                    let _ = unlinkat(dir, &temp_name, AtFlags::empty());
                    Err(error.into())
                }
            };
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not find an unused name for the skill document's temporary file",
        ))
    }

    /// Create and write a skill's directory and `SKILL.md`, a symlink or a
    /// hard link at either refused rather than followed or written
    /// through.
    pub(super) fn write_skill(skills_dir: &Path, name: &str, document: &str) -> io::Result<()> {
        let root = open_dir_no_follow(skills_dir).map_err(io::Error::from)?;
        let dir = open_or_recreate_dir(&root, name).map_err(io::Error::from)?;
        write_document_atomically(&dir, document)
    }

    #[cfg(test)]
    mod tests {
        use super::write_skill;

        /// A hard link shares its data with every name that holds it, `SKILL.md`
        /// included: opening it with `O_TRUNC` would empty that data from
        /// outside `skills_dir` as easily as from in it. `write_skill` never
        /// opens the old entry at all, so the file the link's other name reads
        /// is untouched, and the skill's own text lands at its own directory,
        /// now a fresh file under the same name.
        #[test]
        fn a_hard_linked_skill_md_is_replaced_rather_than_written_through() {
            let root = tempfile::tempdir().unwrap();
            let skills_dir = root.path().join("skills");
            let dir = skills_dir.join("safe");
            std::fs::create_dir_all(&dir).unwrap();
            let shared = root.path().join("shared.md");
            std::fs::write(&shared, "not a skill").unwrap();
            std::fs::hard_link(&shared, dir.join("SKILL.md")).unwrap();

            write_skill(&skills_dir, "safe", "the real skill text").unwrap();

            assert_eq!(
                std::fs::read_to_string(&shared).unwrap(),
                "not a skill",
                "wrote through the hard link to the file it shares data with"
            );
            assert_eq!(
                std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
                "the real skill text"
            );
        }

        /// A directory symlink at a skill's own name — placed after whatever
        /// earlier pass might have cleared one, before this write ever runs —
        /// answers `ELOOP` on Linux and `ENOTDIR` on macOS for the same
        /// `O_DIRECTORY | O_NOFOLLOW` open; both are cleared and replaced
        /// with a real directory of the same name, and the document lands
        /// there, never through the symlink.
        #[test]
        fn a_directory_symlink_left_at_write_time_is_replaced_rather_than_followed() {
            let root = tempfile::tempdir().unwrap();
            let skills_dir = root.path().join("skills");
            std::fs::create_dir_all(&skills_dir).unwrap();
            let outside = root.path().join("outside");
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, skills_dir.join("safe")).unwrap();

            write_skill(&skills_dir, "safe", "the real skill text").unwrap();

            assert!(
                !outside.join("SKILL.md").exists(),
                "wrote through the directory symlink to outside the skills directory"
            );
            assert!(
                !std::fs::symlink_metadata(skills_dir.join("safe"))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "the directory symlink survived the write"
            );
            assert_eq!(
                std::fs::read_to_string(skills_dir.join("safe").join("SKILL.md")).unwrap(),
                "the real skill text"
            );
        }

        /// A temporary file planted under the old, predictable name —
        /// built from the daemon's own PID, which anything local could
        /// read — would have `O_EXCL` refuse the write outright before
        /// this fix, the plant alone enough to keep a skill's document
        /// from ever being written. An unpredictable name is not worth
        /// guessing, so the plant here changes nothing.
        #[test]
        fn a_file_at_the_old_predictable_temp_name_does_not_block_the_write() {
            let root = tempfile::tempdir().unwrap();
            let skills_dir = root.path().join("skills");
            let dir = skills_dir.join("safe");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!(".skill-{}.tmp", std::process::id())),
                "planted",
            )
            .unwrap();

            write_skill(&skills_dir, "safe", "the real skill text").unwrap();

            assert_eq!(
                std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
                "the real skill text"
            );
        }

        /// A write that keeps failing is reported, not merely logged: the
        /// skill's own directory refusing new entries fails every attempt
        /// at a temporary file the same way a permission refusal or a full
        /// disk would, and `write_skill` answers `Err` for it rather than
        /// swallowing the failure the way `sync_skills` used to.
        #[test]
        fn a_persistently_failing_write_is_reported() {
            use std::os::unix::fs::PermissionsExt;

            let root = tempfile::tempdir().unwrap();
            let skills_dir = root.path().join("skills");
            let dir = skills_dir.join("safe");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();

            let result = write_skill(&skills_dir, "safe", "the real skill text");

            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert!(
                result.is_err(),
                "a directory that refuses every new entry must fail the write, not succeed silently"
            );
        }
    }
}

/// The same contract on a platform with no `openat`: a symlink at a skill
/// directory is refused on a path-based check, then a fresh document replaces
/// `SKILL.md`. This catches every ordinary mistake, if not one timed between
/// the path check and directory write.
#[cfg(not(unix))]
mod symlink_safe {
    use std::fs::OpenOptions;
    use std::io::{self, Write};
    use std::path::Path;

    fn is_symlink(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    }

    /// A temporary name must not be predictable: another process could
    /// otherwise plant it first and stop the skill write with no further
    /// access to the directory.
    fn random_suffix() -> String {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("the OS random source is available");
        hex::encode(bytes)
    }

    const TEMP_NAME_ATTEMPTS: u32 = 8;

    /// Write a fresh document and replace the old name in one rename. This
    /// never opens the prior entry, so a hard link cannot change the file it
    /// aliases outside the skills directory.
    fn write_document_atomically(dir: &Path, document: &str) -> io::Result<()> {
        for _ in 0..TEMP_NAME_ATTEMPTS {
            let temporary = dir.join(format!(".skill-{}.tmp", random_suffix()));
            let mut file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            if let Err(error) = file.write_all(document.as_bytes()) {
                let _ = std::fs::remove_file(&temporary);
                return Err(error);
            }
            drop(file);
            return match std::fs::rename(&temporary, dir.join("SKILL.md")) {
                Ok(()) => Ok(()),
                Err(error) => {
                    let _ = std::fs::remove_file(&temporary);
                    Err(error)
                }
            };
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not find an unused name for the skill document's temporary file",
        ))
    }

    pub(super) fn write_skill(skills_dir: &Path, name: &str, document: &str) -> io::Result<()> {
        let dir = skills_dir.join(name);
        if is_symlink(&dir) {
            std::fs::remove_file(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        write_document_atomically(&dir, document)
    }

    #[cfg(test)]
    mod tests {
        use super::write_skill;

        /// The replacement write does not open the existing hard link. The
        /// linked file stays unchanged and the skill receives a fresh file.
        #[test]
        fn a_hard_linked_skill_md_is_replaced_rather_than_written_through() {
            let root = tempfile::tempdir().unwrap();
            let skills_dir = root.path().join("skills");
            let dir = skills_dir.join("safe");
            std::fs::create_dir_all(&dir).unwrap();
            let shared = root.path().join("shared.md");
            std::fs::write(&shared, "not a skill").unwrap();
            std::fs::hard_link(&shared, dir.join("SKILL.md")).unwrap();

            write_skill(&skills_dir, "safe", "the real skill text").unwrap();

            assert_eq!(std::fs::read_to_string(&shared).unwrap(), "not a skill");
            assert_eq!(
                std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
                "the real skill text"
            );
        }
    }
}

/// A fully planned launch of one ACP agent.
#[derive(Debug, Clone)]
pub struct SpawnPlan {
    /// What rides behind the agent's registry command: its configured flags.
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// The agent session a resume continues; `None` for a fresh spawn.
    pub internal_session_id: Option<String>,
    /// The protocol half of the launch, also written to the run dir as
    /// `acp.json`, the record of what the agent was told.
    pub config: LaunchConfig,
}

/// Env vars every agent is launched with. The MCP server reads these to know
/// which session it acts for.
pub(crate) fn base_env(ctx: &SpawnCtx) -> Vec<(String, String)> {
    let mut env = vec![
        ("ARIADNE_SESSION_ID".into(), ctx.session_id.clone()),
        ("ARIADNE_LAUNCH_ID".into(), ctx.launch_id.clone()),
        ("ARIADNE_SEAT".into(), ctx.seat.as_str().to_string()),
        (
            "ARIADNE_SOCKET".into(),
            ctx.socket_path.display().to_string(),
        ),
    ];
    if let Some(goal) = &ctx.goal_id {
        env.push(("ARIADNE_GOAL_ID".into(), goal.clone()));
    }
    if let Some(task) = &ctx.task_id {
        env.push(("ARIADNE_TASK_ID".into(), task.clone()));
    }
    if let Some(pull_request) = &ctx.pull_request_id {
        env.push(("ARIADNE_PULL_REQUEST_ID".into(), pull_request.clone()));
    }
    env
}
