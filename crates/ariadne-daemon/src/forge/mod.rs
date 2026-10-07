//! The forge a repository's remote is on, and the CLI that speaks to it (025).
//!
//! Ariadne talks to a forge only through its own CLI, `gh` for GitHub and
//! `glab` for GitLab, run as a child process: the user signs in once, with the
//! tool they already have, and Ariadne holds no token of its own. Each forge
//! is a directory of its own, so what a later change teaches one forge goes
//! beside the rest of it.

pub mod github;
pub mod gitlab;

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use ariadne_api::issues::IssueDto;
use tracing::warn;

pub use ariadne_core::ForgeKind;
use ariadne_store::{ForgeIntegration, ForgeWrite, Repository, SetForgeIntegration, Store};

use crate::config::Config;

/// How long one forge CLI call may take. `gh api` goes over the network, so
/// this is longer than a local probe, and still bounded: a CLI waiting on a
/// prompt must not hold a request open.
const CLI_TIMEOUT: Duration = Duration::from_secs(30);

/// The remote Ariadne reads, where the checkout has it.
const ORIGIN: &str = "origin";

/// A forge repository, as a remote URL names it: lower-cased throughout. On
/// GitLab the owner can be a group path, `group/subgroup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub host: String,
    pub owner: String,
    pub name: String,
}

impl Remote {
    /// Read a remote URL: `git@host:owner/name.git`, `ssh://git@host/owner/name`,
    /// `https://host/owner/name.git/` and the like, with or without `.git` and
    /// a trailing slash. None for anything that names no host and repository —
    /// a local path, a `file://` URL.
    pub fn parse(url: &str) -> Option<Remote> {
        let url = url.trim();
        let (authority, path) = match url.split_once("://") {
            Some((scheme, rest)) => {
                if !matches!(scheme, "https" | "http" | "ssh" | "git" | "git+ssh") {
                    return None;
                }
                rest.split_once('/')?
            }
            // The scp form, `[user@]host:path`. A path with a slash before
            // the colon is a local path, not a host.
            None => {
                let (authority, path) = url.split_once(':')?;
                if authority.contains('/') {
                    return None;
                }
                (authority, path)
            }
        };
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host.split_once(':').map_or(host, |(h, _)| h);
        let path = path.trim_matches('/');
        let path = path
            .strip_suffix(".git")
            .unwrap_or(path)
            .trim_end_matches('/');
        let (owner, name) = path.rsplit_once('/')?;
        let owner = owner.trim_start_matches('/');
        if host.is_empty() || owner.is_empty() || name.is_empty() {
            return None;
        }
        Some(Remote {
            host: host.to_lowercase(),
            owner: owner.to_lowercase(),
            name: name.to_lowercase(),
        })
    }
}

/// What a checkout's remote says: the forge, the remote's name and the
/// repository on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub kind: ForgeKind,
    pub remote: String,
    pub repo: Remote,
}

/// Read the forge off a checkout: `origin`, or the one remote where there is
/// no `origin`. `github.com` is GitHub and `gitlab.com` GitLab; another host
/// is whichever CLI is signed in to it, `gh` first. None where there is no
/// such remote, its URL names no repository, or neither CLI knows the host.
pub async fn detect(cfg: &Config, checkout: &Path) -> Option<Detected> {
    let remote = remote_name(checkout).await?;
    let url = git(checkout, &["remote", "get-url", &remote]).await?;
    let repo = Remote::parse(&url)?;
    let kind = match repo.host.as_str() {
        "github.com" => ForgeKind::Github,
        "gitlab.com" => ForgeKind::Gitlab,
        host => {
            if ForgeClient::new(cfg, ForgeKind::Github)
                .auth_status(host)
                .await
                .is_ok()
            {
                ForgeKind::Github
            } else if ForgeClient::new(cfg, ForgeKind::Gitlab)
                .auth_status(host)
                .await
                .is_ok()
            {
                ForgeKind::Gitlab
            } else {
                return None;
            }
        }
    };
    Some(Detected { kind, remote, repo })
}

/// Detect a repository's forge again and write what changed: a row for a
/// remote that appeared, none for one that went away, and the new remote for
/// one that moved. A remote that now names another forge repository is
/// disabled, keeping its pins: the sign-in and the one enabled row per forge
/// repository were both checked against the old one. Answers the repository
/// as it now stands.
pub async fn redetect(
    store: &Store,
    cfg: &Config,
    repository: Repository,
) -> ariadne_store::Result<Repository> {
    let detected = detect(cfg, Path::new(&repository.path)).await;
    let row = merged(&repository.id, repository.forge.as_ref(), detected.as_ref());
    written(store, repository, row).await
}

/// Write the forge row a repository is to have, where it differs from the
/// one it has. Answers the repository as it now stands.
async fn written(
    store: &Store,
    repository: Repository,
    row: Option<SetForgeIntegration>,
) -> ariadne_store::Result<Repository> {
    match change(&repository, row) {
        ForgeWrite::Keep => Ok(repository),
        ForgeWrite::Clear(id) => {
            store.clear_forge_integration(&id).await?;
            store.get_repository(&id).await
        }
        ForgeWrite::Set(row) => store.set_forge_integration(*row).await,
    }
}

/// What it takes to leave `repository` with the forge row `row`: nothing,
/// a dropped row, or a written one.
pub fn change(repository: &Repository, row: Option<SetForgeIntegration>) -> ForgeWrite {
    match row {
        None if repository.forge.is_none() => ForgeWrite::Keep,
        None => ForgeWrite::Clear(repository.id.clone()),
        Some(row) if repository.forge.as_ref().map(stored).as_ref() == Some(&row) => {
            ForgeWrite::Keep
        }
        Some(row) => ForgeWrite::Set(Box::new(row)),
    }
}

/// The row a detection leaves: the detected remote, over what the repository
/// already keeps (`current`). None where nothing was detected.
pub fn merged(
    repository_id: &str,
    current: Option<&ForgeIntegration>,
    detected: Option<&Detected>,
) -> Option<SetForgeIntegration> {
    let detected = detected?;
    let fresh = SetForgeIntegration {
        repository_id: repository_id.to_string(),
        kind: detected.kind,
        host: detected.repo.host.clone(),
        owner: detected.repo.owner.clone(),
        name: detected.repo.name.clone(),
        remote: detected.remote.clone(),
        enabled: false,
        login: None,
        babysit_model: None,
        babysit_effort: None,
        review_model: None,
        review_effort: None,
    };
    let Some(current) = current else {
        return Some(fresh);
    };
    let same = current.kind() == fresh.kind
        && current.host == fresh.host
        && current.owner == fresh.owner
        && current.name == fresh.name;
    Some(SetForgeIntegration {
        enabled: same && current.enabled,
        login: current.login.clone().filter(|_| same),
        babysit_model: current.babysit_model.clone(),
        babysit_effort: current.babysit_effort.clone(),
        review_model: current.review_model.clone(),
        review_effort: current.review_effort.clone(),
        ..fresh
    })
}

/// A stored row as the write that would leave it.
fn stored(row: &ForgeIntegration) -> SetForgeIntegration {
    SetForgeIntegration {
        repository_id: row.repository_id.clone(),
        kind: row.kind(),
        host: row.host.clone(),
        owner: row.owner.clone(),
        name: row.name.clone(),
        remote: row.remote.clone(),
        enabled: row.enabled,
        login: row.login.clone(),
        babysit_model: row.babysit_model.clone(),
        babysit_effort: row.babysit_effort.clone(),
        review_model: row.review_model.clone(),
        review_effort: row.review_effort.clone(),
    }
}

/// Detect the forge of every registered repository, once, as the daemon
/// starts: a remote changed while the daemon was down is read here.
pub async fn detect_all(store: &Store, cfg: &Config) {
    let repositories = match store.list_repositories().await {
        Ok(repositories) => repositories,
        Err(e) => {
            warn!(error = %e, "cannot list the repositories to detect their forges");
            return;
        }
    };
    for repository in repositories {
        let id = repository.id.clone();
        if let Err(e) = redetect(store, cfg, repository).await {
            warn!(repository = %id, error = %e, "cannot detect the forge of a repository");
        }
    }
}

/// The remote to read: `origin`, else the only one there is.
async fn remote_name(checkout: &Path) -> Option<String> {
    let names = git(checkout, &["remote"]).await?;
    let names: Vec<&str> = names
        .lines()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    match names.as_slice() {
        names if names.contains(&ORIGIN) => Some(ORIGIN.to_string()),
        [only] => Some(only.to_string()),
        _ => None,
    }
}

/// What a git command printed, trimmed, or None where it failed.
async fn git(checkout: &Path, args: &[&str]) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The CLI of one forge.
#[derive(Debug, Clone)]
pub enum ForgeClient {
    Github(github::Github),
    Gitlab(gitlab::Gitlab),
}

impl ForgeClient {
    pub async fn list_open_issues(
        &self,
        repository: &str,
        assignee: Option<&str>,
    ) -> Result<Vec<IssueDto>, String> {
        match self {
            Self::Github(cli) => cli.list_open_issues(repository, assignee).await,
            Self::Gitlab(cli) => cli.list_open_issues(repository, assignee).await,
        }
    }

    pub async fn issue(&self, repository: &str, number: i64) -> Result<IssueDto, String> {
        match self {
            Self::Github(cli) => cli.issue(repository, number).await,
            Self::Gitlab(cli) => cli.issue(repository, number).await,
        }
    }
    /// The CLI of a kind of forge, as the config names it.
    pub fn new(cfg: &Config, kind: ForgeKind) -> ForgeClient {
        match kind {
            ForgeKind::Github => {
                ForgeClient::Github(github::Github::new(Cli::new(cfg.gh_bin.as_deref(), "gh")))
            }
            ForgeKind::Gitlab => ForgeClient::Gitlab(gitlab::Gitlab::new(Cli::new(
                cfg.glab_bin.as_deref(),
                "glab",
            ))),
        }
    }

    /// The CLI of the forge a repository's remote is on.
    pub fn for_repository(cfg: &Config, forge: &ForgeIntegration) -> ForgeClient {
        ForgeClient::new(cfg, forge.kind())
    }

    /// Whether the CLI is installed and signed in to `host`. The error is the
    /// CLI's own words, or what stopped it from answering.
    pub async fn auth_status(&self, host: &str) -> Result<(), String> {
        match self {
            ForgeClient::Github(cli) => cli.auth_status(host).await,
            ForgeClient::Gitlab(cli) => cli.auth_status(host).await,
        }
    }

    /// The account the CLI is signed in to `host` as.
    pub async fn whoami(&self, host: &str) -> Result<String, String> {
        match self {
            ForgeClient::Github(cli) => cli.whoami(host).await,
            ForgeClient::Gitlab(cli) => cli.whoami(host).await,
        }
    }
}

/// One forge CLI: the program the config names, found the way `python_bin`
/// is found — a path as it stands, a bare name on the daemon's PATH.
#[derive(Debug, Clone)]
pub struct Cli {
    configured: Option<String>,
    program: &'static str,
}

impl Cli {
    fn new(configured: Option<&str>, program: &'static str) -> Cli {
        Cli {
            configured: configured.map(str::to_string),
            program,
        }
    }

    /// Where the program is, or why it is not there.
    fn binary(&self) -> Result<PathBuf, String> {
        let name = self.configured.as_deref().unwrap_or(self.program);
        let found = match name.contains('/') {
            true => Some(PathBuf::from(name)).filter(|p| ariadne_core::is_executable(p)),
            false => {
                let path = std::env::var_os("PATH").unwrap_or_default();
                ariadne_core::which(&path, name)
            }
        };
        found.ok_or_else(|| {
            format!(
                "`{name}` is not installed on the daemon's PATH; install it, or set {}_bin \
                 in config.toml",
                self.program
            )
        })
    }

    /// Run the CLI to completion, bounded by [`CLI_TIMEOUT`].
    async fn run(&self, args: &[&str]) -> Result<Output, String> {
        let binary = self.binary()?;
        let child = tokio::process::Command::new(&binary)
            .args(args)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        match tokio::time::timeout(CLI_TIMEOUT, child).await {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(format!("cannot run `{}`: {e}", binary.display())),
            Err(_) => Err(format!(
                "`{} {}` did not answer in {} seconds",
                self.program,
                args.join(" "),
                CLI_TIMEOUT.as_secs()
            )),
        }
    }

    /// Run the CLI and answer its standard output, or its own words on why
    /// it failed.
    async fn answer(&self, args: &[&str]) -> Result<String, String> {
        let output = self.run(args).await?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
        let said = match output.stderr.is_empty() {
            true => String::from_utf8_lossy(&output.stdout),
            false => String::from_utf8_lossy(&output.stderr),
        };
        let said = said.trim();
        Err(match said.is_empty() {
            true => format!(
                "`{} {}` failed with {}",
                self.program,
                args.join(" "),
                output.status
            ),
            false => format!("`{} {}`: {said}", self.program, args.join(" ")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(host: &str, owner: &str, name: &str) -> Option<Remote> {
        Some(Remote {
            host: host.into(),
            owner: owner.into(),
            name: name.into(),
        })
    }

    /// Every spelling a clone URL takes reads as the same repository,
    /// lower-cased; a local path names none.
    #[test]
    fn a_remote_url_is_read_in_its_ssh_and_https_forms() {
        let ariadne = remote("github.com", "acme", "ariadne");
        for url in [
            "git@github.com:acme/ariadne.git",
            "git@github.com:acme/ariadne",
            "git@github.com:Acme/Ariadne.git/",
            "ssh://git@github.com/acme/ariadne.git",
            "ssh://git@github.com:22/acme/ariadne",
            "https://github.com/acme/ariadne.git",
            "https://github.com/acme/ariadne",
            "https://github.com/acme/ariadne/",
            "https://user:token@GitHub.com/acme/ariadne.git",
        ] {
            assert_eq!(Remote::parse(url), ariadne, "{url}");
        }
        assert_eq!(
            Remote::parse("git@gitlab.example.com:group/sub/proj.git"),
            remote("gitlab.example.com", "group/sub", "proj")
        );
        for url in [
            "/srv/git/ariadne.git",
            "../ariadne",
            "file:///srv/git/ariadne.git",
            "https://github.com/ariadne",
            "",
        ] {
            assert_eq!(Remote::parse(url), None, "{url}");
        }
    }
}
