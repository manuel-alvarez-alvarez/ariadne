//! The published ACP registry index, and what each of its agents is called
//! on this machine.
//!
//! The index names every agent that speaks ACP and how each one is
//! distributed — a platform binary, an npm package, or a Python one. Ariadne
//! installs none of them: it reads the index for the command an agent is run
//! by, and the daemon's `PATH` decides which of those commands exists here.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use ariadne_core::probe::which;

/// One agent of the index that this machine holds: its registry id, the
/// command it is run and listed by, and the file the `PATH` search found.
#[derive(Debug)]
pub(crate) struct Installed {
    pub(crate) id: String,
    pub(crate) command: Vec<String>,
    /// Whether the `PATH` answered under the name of the agent's package
    /// rather than under a name the agent is known by. Such a name is a
    /// package's, and another package's command can carry it, so a probe has
    /// the last word on it.
    pub(crate) named_by_package: bool,
    /// Set where a name only the package gives passed neither the symlink
    /// check nor a package manager's own file, to the registry package,
    /// unpinned: what the probe's `agentInfo.name` has to answer before the
    /// entry is kept. `None` where the symlink or a manager file already
    /// proved it, or where the name is not the package's alone.
    pub(crate) unproven_package: Option<String>,
    /// What a probe starts. Discovery already chose one file out of the
    /// `PATH` it searched, and starting that file is what it decided —
    /// searching again by name would answer from whatever `PATH` the process
    /// carries, which is not always the one discovery read.
    pub(crate) program: PathBuf,
}

/// Where each package manager keeps its own record of what it installed.
/// Every check here only reads a file: proving a package this way starts no
/// process, unlike the ACP probe that decides what neither check reaches.
#[derive(Clone)]
pub struct ManagerHomes {
    /// Volta's own directory: `$VOLTA_HOME`, or `~/.volta`.
    pub volta: PathBuf,
    /// asdf's data directory: `$ASDF_DATA_DIR`, or `~/.asdf`.
    pub asdf: PathBuf,
    /// mise's data directory: `$MISE_DATA_DIR`, else `$XDG_DATA_HOME/mise`,
    /// else `~/.local/share/mise`.
    pub mise: PathBuf,
}

impl ManagerHomes {
    /// The homes the daemon's own environment names.
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let dir = |var: &str, default: PathBuf| {
            std::env::var_os(var)
                .filter(|value| !value.is_empty())
                .map_or(default, PathBuf::from)
        };
        Self {
            volta: dir("VOLTA_HOME", home.join(".volta")),
            asdf: dir("ASDF_DATA_DIR", home.join(".asdf")),
            mise: dir(
                "MISE_DATA_DIR",
                dir("XDG_DATA_HOME", home.join(".local/share")).join("mise"),
            ),
        }
    }
}

/// Accept a downloaded document only when the discovery parser can read it.
pub(crate) fn validate(document: &str) -> Result<(), serde_json::Error> {
    serde_json::from_str::<Index>(document).map(|_| ())
}

/// Every agent of `index` that `path` holds, in index order.
///
/// An index that cannot be read leaves the registry with the configured
/// agents alone: a snapshot Ariadne cannot parse is a defect in Ariadne, and
/// stopping a daemon over it would take the user's own agents down with it.
pub(crate) fn installed(index: &str, path: &OsStr, homes: &ManagerHomes) -> Vec<Installed> {
    let index: Index = match serde_json::from_str(index) {
        Ok(index) => index,
        Err(error) => {
            tracing::warn!(error = %error, "reading the ACP registry index failed");
            return Vec::new();
        }
    };
    // Nothing left to search is nothing found: an empty `PATH` is one empty
    // entry, and `which` would read that as the current directory again.
    let path = searchable(path);
    if path.is_empty() {
        return Vec::new();
    }
    index
        .agents
        .iter()
        .filter_map(|agent| found(agent, &path, homes))
        .filter(|installed| {
            let refuse = names_a_shell(&installed.command);
            if refuse {
                tracing::warn!(
                    agent = installed.id,
                    command = installed.command.join(" "),
                    "the index names a shell interpreter as this agent's command; refusing the entry"
                );
            }
            !refuse
        })
        .collect()
}

/// Program names of a shell interpreter: an index entry naming one as its
/// command runs whatever its arguments say rather than speaking ACP, so it
/// is refused whatever distribution it claims.
const SHELL_INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "cmd",
    "powershell",
    "pwsh",
];

/// Whether `command` names a shell interpreter, or carries `-c` among its
/// arguments — the flag every one of them runs a string of as a command.
fn names_a_shell(command: &[String]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    SHELL_INTERPRETERS.contains(&base) || command[1..].iter().any(|arg| arg == "-c")
}

/// The entries of `path` that name a directory outright, in their own order.
///
/// An empty entry — and an empty `PATH` is one — names the directory the
/// daemon happens to have been started in, as a relative entry does, and an
/// agent is not something a daemon picks up from there: a probe, which runs
/// in a directory of its own, would look for that file somewhere else again.
/// Only such an entry is dropped, never the search: an absolute entry behind
/// one is searched as it always was.
fn searchable(path: &OsStr) -> OsString {
    let absolute = std::env::split_paths(path).filter(|dir| dir.is_absolute());
    std::env::join_paths(absolute).unwrap_or_default()
}

/// The first name of `agent` that `path` holds, as the command it runs.
///
/// A name the package gives rather than the agent has to be the package's
/// own file as well: `@minimax-ai/code` installs `code`, and so does an
/// editor that has nothing to do with ACP. Three checks prove it, in order,
/// and the first to prove it stops the rest: the symlink chain
/// ([`installs`]), a package manager's own file ([`package_files`]), and —
/// unless the name is on [`SKIPPED_PROGRAMS`] — the ACP probe itself, which
/// decides from the entry's [`Installed::unproven_package`].
fn found(agent: &IndexAgent, path: &OsStr, homes: &ManagerHomes) -> Option<Installed> {
    candidates(agent).into_iter().find_map(|candidate| {
        let program = which(path, &candidate.name)?;
        let unproven_package = match &candidate.package {
            None => None,
            Some(package)
                if installs(&program, package)
                    || package_files(&program, &candidate.name, package, homes) =>
            {
                None
            }
            Some(_) if SKIPPED_PROGRAMS.contains(&candidate.name.as_str()) => {
                tracing::debug!(
                    agent = agent.id,
                    name = candidate.name,
                    program = %program.display(),
                    "a program of a skipped name proved nothing of its package; passing it over"
                );
                return None;
            }
            Some(package) => Some(package.join("/")),
        };
        Some(Installed {
            id: agent.id.clone(),
            named_by_package: candidate.package.is_some(),
            unproven_package,
            command: std::iter::once(candidate.name)
                .chain(candidate.args)
                .collect(),
            program,
        })
    })
}

/// Program names that never reach the ACP probe on an unproven package name:
/// another program of this name is common enough, and known enough by name,
/// that starting it on a guess is not worth the ACP probe getting to say no.
/// `code` is MiniMax Code's command, and also most editors'.
const SKIPPED_PROGRAMS: &[&str] = &["code"];

/// Whether a package manager's own file proves `program` — found under
/// `name` — is `package`'s, without starting anything: Volta's record of
/// what it linked, a pnpm shim naming the path it points into, or an asdf or
/// mise install that itself passes the symlink check.
fn package_files(program: &Path, name: &str, package: &[String], homes: &ManagerHomes) -> bool {
    volta_installs(name, package, &homes.volta)
        || pnpm_installs(program, package)
        || plugin_installs(&homes.asdf, name, package)
        || plugin_installs(&homes.mise, name, package)
}

/// Volta links every tool under one shim and tells them apart by name alone,
/// so the chain from `name` never reaches the package: only Volta's own
/// record of what it linked — `tools/user/bins/<name>.json`, whose
/// `package` field names the package — says whether this one does.
fn volta_installs(name: &str, package: &[String], volta_home: &Path) -> bool {
    let file = volta_home
        .join("tools/user/bins")
        .join(format!("{name}.json"));
    let Ok(contents) = std::fs::read_to_string(&file) else {
        return false;
    };
    let Ok(recorded) = serde_json::from_str::<VoltaBin>(&contents) else {
        return false;
    };
    package_dirs(&recorded.package).join("/") == package.join("/")
}

#[derive(Deserialize)]
struct VoltaBin {
    package: String,
}

/// A pnpm global shim is a script, not a symlink, so the chain from it never
/// reaches the package either: its own text names the path pnpm resolves it
/// through, and one of its path-like words has to stand under the
/// package's own directories — the same test [`installs`] runs on a link,
/// run here on a word of the text instead. A substring would answer yes to
/// `@scope/name-legacy` for `@scope/name`; a whole path component does not.
fn pnpm_installs(program: &Path, package: &[String]) -> bool {
    let Ok(file) = std::fs::File::open(program) else {
        return false;
    };
    let mut text = String::new();
    // A shim is a short script; bounding the read keeps a native binary
    // found under the same name from being read whole first.
    if file
        .take(PNPM_SHIM_READ_LIMIT)
        .read_to_string(&mut text)
        .is_err()
    {
        return false;
    }
    pnpm_shim_words(&text).any(|word| under(Path::new(word), package))
}

/// How much of a program's text [`pnpm_installs`] reads before giving up:
/// generous for a shim script, and not the whole of a native binary found
/// under the same name.
const PNPM_SHIM_READ_LIMIT: u64 = 64 * 1024;

/// The path-like words of a shim script's text: split on whitespace and the
/// characters a shell line quotes, assigns or separates arguments with.
fn pnpm_shim_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || "\"'=:;".contains(c))
        .filter(|word| !word.is_empty())
}

/// asdf and mise both shim by plugin and version rather than by symlink, and
/// each keeps the real install at `<data dir>/installs/<plugin>/<version>/bin/<name>`
/// — a file the symlink check ([`installs`]) still answers for, once it is
/// found rather than chained to.
fn plugin_installs(data_dir: &Path, name: &str, package: &[String]) -> bool {
    let Ok(plugins) = std::fs::read_dir(data_dir.join("installs")) else {
        return false;
    };
    for plugin in plugins.flatten() {
        let Ok(versions) = std::fs::read_dir(plugin.path()) else {
            continue;
        };
        for version in versions.flatten() {
            let candidate = version.path().join("bin").join(name);
            if installs(&candidate, package) {
                return true;
            }
        }
    }
    false
}

/// Whether `program` is a file `package` installs: it, or a link on the way
/// to it, stands under the package's own directories. That is what every
/// package manager leaves behind — npm links `bin/claude-agent-acp` to
/// `lib/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js`,
/// and uv puts its tools under a directory of the tool's name — and a
/// program of some other origin that happens to carry the name has none of
/// it.
///
/// The whole chain is read rather than the file it ends at, since the
/// package directory is as often on the link as on its target.
fn installs(program: &Path, package: &[String]) -> bool {
    let mut at = program.to_path_buf();
    // A link that points at a link, bounded: a cycle is otherwise forever.
    for _ in 0..LINKS {
        if under(&at, package) {
            return true;
        }
        let Ok(target) = std::fs::read_link(&at) else {
            return false;
        };
        at = match at.parent() {
            Some(dir) if target.is_relative() => dir.join(target),
            _ => target,
        };
    }
    false
}

/// How many links deep a program is followed while its package is looked
/// for.
const LINKS: usize = 16;

/// Whether the directories `path` stands under hold `package`'s own, one
/// after the other: `@scope` and then the name, or the name alone.
fn under(path: &Path, package: &[String]) -> bool {
    let directories: Vec<&str> = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    // The last component is the file itself, never a directory it is under.
    let directories = directories.split_last().map_or(&[][..], |(_, rest)| rest);
    directories
        .windows(package.len())
        .any(|window| window.iter().zip(package).all(|(held, want)| held == want))
}

/// What one entry could be installed as here, in the order the names are
/// tried.
///
/// A `binary` entry is a program of its own: the basename of the `cmd` of
/// this platform's build. An `npx` or `uvx` entry is a package, and the
/// command it installs is named either after the registry entry or after the
/// package, so both are tried — the entry id first, since that is the name
/// the agent is known by. The distribution's arguments follow whichever name
/// answers.
///
/// A name of the package's own carries that package, since a name only the
/// package gives says nothing about which program answers to it here.
fn candidates(agent: &IndexAgent) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let binary = platform_key().and_then(|key| agent.distribution.binary.get(&key));
    if let Some(target) = binary {
        candidates.push(Candidate {
            name: basename(&target.cmd).to_string(),
            args: target.args.clone(),
            package: None,
        });
    }
    let packaged = [&agent.distribution.npx, &agent.distribution.uvx];
    for package in packaged.into_iter().flatten() {
        candidates.push(Candidate {
            name: agent.id.clone(),
            args: package.args.clone(),
            package: None,
        });
        candidates.push(Candidate {
            name: command_name(&package.package).to_string(),
            args: package.args.clone(),
            package: Some(package_dirs(&package.package)),
        });
    }
    candidates
}

struct Candidate {
    name: String,
    args: Vec<String>,
    /// The package this name comes from, as the directories it is unpacked
    /// into, or `None` for a name the agent itself is known by — its
    /// registry id, or the command of its own build — which stands for the
    /// agent wherever it is installed from.
    package: Option<Vec<String>>,
}

/// This machine's key in a `binary` distribution, or `None` where the index
/// names no build for it.
fn platform_key() -> Option<String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        arch @ ("aarch64" | "x86_64") => arch,
        _ => return None,
    };
    Some(format!("{os}-{arch}"))
}

/// The file `cmd` names, without the directory it is unpacked into: `./goose`
/// is the command `goose`.
fn basename(cmd: &str) -> &str {
    cmd.rsplit('/').next().unwrap_or(cmd)
}

/// The command a package installs, by convention its own name: without the
/// `@scope/` it is published under and without the version it is pinned to —
/// `@google/gemini-cli@0.60.0` is `gemini-cli`, `fast-agent-acp==0.10.1` is
/// `fast-agent-acp`.
fn command_name(package: &str) -> &str {
    let unscoped = match package.strip_prefix('@') {
        Some(rest) => rest.split_once('/').map_or(rest, |(_, name)| name),
        None => package,
    };
    unpinned(unscoped)
}

/// The directories a package is unpacked into, outermost first: the
/// `@scope` it is published under, where it has one, and its own name.
/// `@agentclientprotocol/claude-agent-acp@0.79.0` is unpacked into
/// `@agentclientprotocol/claude-agent-acp`, and `fast-agent-acp==0.10.1`
/// into `fast-agent-acp`.
fn package_dirs(package: &str) -> Vec<String> {
    match package.strip_prefix('@') {
        Some(rest) => match rest.split_once('/') {
            Some((scope, name)) => vec![format!("@{scope}"), unpinned(name).to_string()],
            None => vec![format!("@{}", unpinned(rest))],
        },
        None => vec![unpinned(package).to_string()],
    }
}

/// A package without the version it is pinned to: npm pins with `@`, PyPI
/// with `==`.
fn unpinned(package: &str) -> &str {
    let unpinned = package.split_once("==").map_or(package, |(name, _)| name);
    unpinned.split_once('@').map_or(unpinned, |(name, _)| name)
}

/// The index as it is published: everything Ariadne reads of it, and
/// everything else ignored.
#[derive(Deserialize)]
struct Index {
    agents: Vec<IndexAgent>,
}

#[derive(Deserialize)]
struct IndexAgent {
    id: String,
    #[serde(default)]
    distribution: Distribution,
}

#[derive(Default, Deserialize)]
struct Distribution {
    #[serde(default)]
    binary: BTreeMap<String, Target>,
    npx: Option<Package>,
    uvx: Option<Package>,
}

/// One platform's build of a `binary` distribution.
#[derive(Deserialize)]
struct Target {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
}

/// A distribution installed from a package registry, npm or PyPI.
#[derive(Deserialize)]
struct Package {
    package: String,
    #[serde(default)]
    args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file on `dir` that `which` accepts: present, and executable.
    fn executable(dir: &Path, name: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// An index can name a shell as an agent's command — directly, as its
    /// own id, or hidden behind `-c` among its arguments. Either is refused,
    /// and an entry beside it that names neither still registers: refusing
    /// one program the `PATH` holds is not reason to distrust the rest of
    /// the index.
    #[test]
    fn an_index_entry_naming_a_shell_is_refused_and_the_rest_of_the_index_still_installs() {
        let dir = tempfile::tempdir().unwrap();
        executable(dir.path(), "sh");
        executable(dir.path(), "innocuous");
        executable(dir.path(), "widget");
        let path = std::env::join_paths([dir.path()]).unwrap();

        let index = serde_json::json!({"agents": [
            {"id": "sh", "distribution": {"npx": {"package": "evil-cli@1"}}},
            {"id": "dash-c", "distribution": {
                "npx": {"package": "innocuous@1", "args": ["-c", "rm -rf /"]}
            }},
            {"id": "widget", "distribution": {"npx": {"package": "widget@1"}}}
        ]})
        .to_string();

        let found = installed(&index, &path, &ManagerHomes::from_env());
        let ids: Vec<&str> = found.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["widget"], "{found:#?}");
    }
}
