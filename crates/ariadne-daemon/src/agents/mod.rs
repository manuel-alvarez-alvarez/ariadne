//! The ACP adapter: translate an Ariadne spawn/resume request into the
//! launch of one ACP agent — the flags behind its registry command, the
//! environment, and the launch file the ACP runtime drives it from.

mod acp;
mod handoff;
pub mod prompts;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

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
    for entry in std::fs::read_dir(skills_dir)
        .with_context(|| format!("reading {}", skills_dir.display()))?
    {
        let entry = entry.with_context(|| format!("reading {}", skills_dir.display()))?;
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

    for skill in skills {
        if let Some(reason) = skill_name_rejection(&skill.name, &canonical_skills_dir) {
            tracing::error!(name = %skill.name, reason, "a stored skill name is invalid; not writing it");
            continue;
        }
        if let Err(error) =
            symlink_safe::write_skill(skills_dir, &skill.name, skill.document_text())
        {
            tracing::error!(name = %skill.name, error = %error, "writing the skill failed, a symlink in its path removed and retried once");
        }
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

    use rustix::fs::{AtFlags, Mode, OFlags, mkdirat, openat, unlinkat};
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

    /// Open `name` directly under `parent`, making it first if there is
    /// nothing there, and starting over once — unlinking `name` first —
    /// where `open` found a symlink instead: the entry this sync leaves
    /// behind under that name is never one, so one found there is removed
    /// outright, the same as a stray one `sync_skills`'s own cleanup pass
    /// removes.
    fn open_or_recreate(
        parent: &OwnedFd,
        name: &str,
        flags: OFlags,
        make: bool,
    ) -> Result<OwnedFd, Errno> {
        match openat_no_follow(parent, name, flags) {
            Err(Errno::LOOP) => {
                unlinkat(parent, name, AtFlags::empty())?;
                openat_no_follow(parent, name, flags)
            }
            Err(Errno::NOENT) if make => {
                mkdirat(parent, name, Mode::from_raw_mode(0o755))?;
                openat_no_follow(parent, name, flags)
            }
            other => other,
        }
    }

    /// Write `document` as `skills_dir/name/SKILL.md`, making the directory
    /// first if it does not exist, with neither step ever opening through a
    /// symlink.
    pub(super) fn write_skill(skills_dir: &Path, name: &str, document: &str) -> io::Result<()> {
        let root = open_dir_no_follow(skills_dir).map_err(io::Error::from)?;
        let dir =
            open_or_recreate(&root, name, OFlags::DIRECTORY, true).map_err(io::Error::from)?;
        let file = open_or_recreate(
            &dir,
            "SKILL.md",
            OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC,
            false,
        )
        .map_err(io::Error::from)?;
        File::from(file).write_all(document.as_bytes())
    }
}

/// The same contract on a platform with no `openat`: a symlink refused on a
/// path-based check just before the matching path-based write, which closes
/// the gap enough to catch every ordinary mistake, if not one timed to land
/// inside it.
#[cfg(not(unix))]
mod symlink_safe {
    use std::io;
    use std::path::Path;

    fn is_symlink(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    }

    pub(super) fn write_skill(skills_dir: &Path, name: &str, document: &str) -> io::Result<()> {
        let dir = skills_dir.join(name);
        if is_symlink(&dir) {
            std::fs::remove_file(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("SKILL.md");
        if is_symlink(&file) {
            std::fs::remove_file(&file)?;
        }
        std::fs::write(file, document)
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
