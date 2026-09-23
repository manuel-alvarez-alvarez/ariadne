//! The published ACP registry index, and what each of its agents is called
//! on this machine.
//!
//! The index names every agent that speaks ACP and how each one is
//! distributed — a platform binary, an npm package, or a Python one. Ariadne
//! installs none of them: it reads the index for the command an agent is run
//! by, and the daemon's `PATH` decides which of those commands exists here.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use serde::Deserialize;

use ariadne_core::probe::which;

/// One agent of the index that this machine holds: its registry id, the
/// command it is run and listed by, and the file the `PATH` search found.
pub(crate) struct Installed {
    pub(crate) id: String,
    pub(crate) command: Vec<String>,
    /// What a probe starts. Discovery already chose one file out of the
    /// `PATH` it searched, and starting that file is what it decided —
    /// searching again by name would answer from whatever `PATH` the process
    /// carries, which is not always the one discovery read.
    pub(crate) program: PathBuf,
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
pub(crate) fn installed(index: &str, path: &OsStr) -> Vec<Installed> {
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
        .filter_map(|agent| found(agent, &path))
        .collect()
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
fn found(agent: &IndexAgent, path: &OsStr) -> Option<Installed> {
    candidates(agent).into_iter().find_map(|candidate| {
        let program = which(path, &candidate.name)?;
        Some(Installed {
            id: agent.id.clone(),
            command: std::iter::once(candidate.name)
                .chain(candidate.args)
                .collect(),
            program,
        })
    })
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
fn candidates(agent: &IndexAgent) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let binary = platform_key().and_then(|key| agent.distribution.binary.get(&key));
    if let Some(target) = binary {
        candidates.push(Candidate {
            name: basename(&target.cmd).to_string(),
            args: target.args.clone(),
        });
    }
    let packaged = [&agent.distribution.npx, &agent.distribution.uvx];
    for package in packaged.into_iter().flatten() {
        for name in [agent.id.as_str(), command_name(&package.package)] {
            candidates.push(Candidate {
                name: name.to_string(),
                args: package.args.clone(),
            });
        }
    }
    candidates
}

struct Candidate {
    name: String,
    args: Vec<String>,
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
    let unpinned = unscoped.split_once("==").map_or(unscoped, |(name, _)| name);
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
