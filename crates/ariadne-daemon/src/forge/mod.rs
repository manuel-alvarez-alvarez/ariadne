//! The forge a repository's remote is on, and the CLI that speaks to it (025).
//!
//! Ariadne talks to a forge only through its own CLI, `gh` for GitHub and
//! `glab` for GitLab, run as a child process: the user signs in once, with the
//! tool they already have, and Ariadne holds no token of its own. Each forge
//! is a directory of its own, so what a later change teaches one forge goes
//! beside the rest of it.

pub mod github;
pub mod gitlab;
pub mod hooks;
pub mod live;
pub mod news;
pub mod poll;
pub mod pulls;
pub mod tunnel;

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

/// The only identity of a pull request: its registered repository and number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestRef {
    pub repository_id: String,
    pub number: i64,
}

impl PullRequestRef {
    pub fn parse(url: &str, integration: &ForgeIntegration) -> Option<Self> {
        let url = reqwest::Url::parse(url).ok()?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.host_str()?.eq_ignore_ascii_case(&integration.host)
        {
            return None;
        }
        let path = url.path().trim_end_matches('/');
        let (repository, number) = match integration.kind() {
            ForgeKind::Github => path.rsplit_once("/pull/")?,
            ForgeKind::Gitlab => {
                let (repository, number) = path.rsplit_once("/merge_requests/")?;
                (repository.strip_suffix("/-").unwrap_or(repository), number)
            }
        };
        if !repository.eq_ignore_ascii_case(&format!("/{}/{}", integration.owner, integration.name))
            || number.is_empty()
            || !number.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let number = number.parse::<i64>().ok().filter(|n| *n > 0)?;
        Some(Self {
            repository_id: integration.repository_id.clone(),
            number,
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
    let previous = repository.forge.clone();
    let repository = written(store, repository, row).await?;
    hooks::remove_replaced(cfg, previous.as_ref(), repository.forge.as_ref()).await;
    Ok(repository)
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
    /// Every open request of the repository, and which of them ask for
    /// `login`'s review.
    pub async fn list_open_pull_requests(
        &self,
        repository: &str,
        login: &str,
    ) -> Result<pulls::Listed, String> {
        match self {
            Self::Github(cli) => cli.list_open_pull_requests(repository, login).await,
            Self::Gitlab(cli) => cli.list_open_pull_requests(repository, login).await,
        }
    }
    pub async fn pull_request(
        &self,
        repository: &str,
        number: i64,
    ) -> Result<pulls::ForgePullRequest, String> {
        match self {
            Self::Github(cli) => cli.pull_request(repository, number).await,
            Self::Gitlab(cli) => cli.pull_request(repository, number).await,
        }
    }
    pub async fn search_pull_requests(
        &self,
        repository: &str,
        query: &str,
    ) -> Result<Vec<pulls::ForgePullRequest>, String> {
        match self {
            Self::Github(cli) => cli.search_pull_requests(repository, query).await,
            Self::Gitlab(cli) => cli.search_pull_requests(repository, query).await,
        }
    }

    /// Everything a request holds beyond the list fetch's fields (026),
    /// bounded by `within` ([`crate::timeouts::Timeouts::forge_details`]).
    pub async fn details(
        &self,
        repository: &str,
        number: i64,
        within: Duration,
    ) -> Result<pulls::ForgeDetails, String> {
        let read = async {
            match self {
                Self::Github(cli) => cli.details(repository, number).await,
                Self::Gitlab(cli) => cli.details(repository, number).await,
            }
        };
        tokio::time::timeout(within, read)
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "the details of request {number} did not arrive in {} seconds",
                    within.as_secs()
                ))
            })
    }

    /// Reply to one stored comment of request `number` with `body`, and
    /// answer the forge id of the reply.
    pub async fn reply(
        &self,
        repository: &str,
        number: i64,
        comment: &ariadne_store::PullRequestComment,
        body: &str,
    ) -> Result<String, String> {
        match self {
            Self::Github(cli) => cli.reply(repository, number, comment, body).await,
            Self::Gitlab(cli) => cli.reply(repository, number, comment, body).await,
        }
    }

    /// Resolve the thread one stored comment of request `number` is in: a
    /// thread a reviewer session opened, once a push fixed it (029).
    pub async fn resolve(
        &self,
        repository: &str,
        number: i64,
        comment: &ariadne_store::PullRequestComment,
    ) -> Result<(), String> {
        match self {
            Self::Github(cli) => cli.resolve(repository, number, comment).await,
            Self::Gitlab(cli) => cli.resolve(repository, number, comment).await,
        }
    }

    /// Post one review of request `number` in the name of `login`, and
    /// answer what it posted, as comments of that login (029). The review
    /// asks for changes or comments; no call here approves.
    pub async fn submit_review(
        &self,
        repository: &str,
        number: i64,
        review: &pulls::ReviewDraft,
        login: &str,
    ) -> Result<Vec<ariadne_store::NewPullRequestComment>, String> {
        match self {
            Self::Github(cli) => cli.submit_review(repository, number, review, login).await,
            Self::Gitlab(cli) => cli.submit_review(repository, number, review, login).await,
        }
    }

    /// Write the one summary comment a review keeps on request `number`
    /// (029): `existing` edited in place, else a new one. Answers its forge
    /// id and when it was written.
    pub async fn write_summary(
        &self,
        repository: &str,
        number: i64,
        existing: Option<&str>,
        body: &str,
    ) -> Result<(String, String), String> {
        match self {
            Self::Github(cli) => cli.write_summary(repository, number, existing, body).await,
            Self::Gitlab(cli) => cli.write_summary(repository, number, existing, body).await,
        }
    }

    /// Whether request `number`, which no list holds now, still asks for
    /// the review of `login` (029). GitHub reads the request's timeline,
    /// since it stops listing a request once the reviewer reviewed it.
    /// GitLab keeps a reviewer listed after a review, so its list is the
    /// whole answer, and a request it does not list asks for nothing.
    pub async fn review_still_requested(
        &self,
        repository: &str,
        number: i64,
        login: &str,
    ) -> Result<bool, String> {
        match self {
            Self::Github(cli) => cli.review_still_requested(repository, number, login).await,
            Self::Gitlab(_) => Ok(false),
        }
    }

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

    /// Open a pull or merge request from `head` onto `base` in `owner/name`,
    /// titled and bodied as asked, and answer its URL.
    pub async fn open(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<String, String> {
        match self {
            ForgeClient::Github(cli) => cli.open(repo, head, base, title, body, draft).await,
            ForgeClient::Gitlab(cli) => cli.open(repo, head, base, title, body, draft).await,
        }
    }
}

/// One forge CLI: the program the config names, found the way `python_bin`
/// is found — a path as it stands, a bare name on the daemon's PATH.
#[derive(Debug, Clone)]
pub struct Cli {
    configured: Option<String>,
    program: &'static str,
    /// How long one call may take, its input written and its output read:
    /// [`CLI_TIMEOUT`].
    timeout: Duration,
}

impl Cli {
    fn new(configured: Option<&str>, program: &'static str) -> Cli {
        Cli {
            configured: configured.map(str::to_string),
            program,
            timeout: CLI_TIMEOUT,
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

    /// Run the CLI to completion, bounded by [`CLI_TIMEOUT`], with `input`
    /// on its standard input where there is one. The input is written while
    /// the output is read, under the same deadline: a CLI that stalls before
    /// it reads an input larger than the pipe holds would otherwise block
    /// the write past any deadline, and one that writes much before it reads
    /// would deadlock against it. A call that runs out kills the CLI.
    async fn run(&self, args: &[&str], input: Option<&str>) -> Result<Output, String> {
        use tokio::io::AsyncWriteExt;
        let binary = self.binary()?;
        let stdin = match input {
            Some(_) => std::process::Stdio::piped(),
            None => std::process::Stdio::null(),
        };
        let spawned = tokio::process::Command::new(&binary)
            .args(args)
            .stdin(stdin)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let mut child = spawned.map_err(|e| format!("cannot run `{}`: {e}", binary.display()))?;
        let pipe = child.stdin.take();
        let write = async move {
            if let (Some(input), Some(mut pipe)) = (input, pipe) {
                pipe.write_all(input.as_bytes()).await?;
                // Dropped here: the CLI reads the end of its input.
            }
            Ok::<(), std::io::Error>(())
        };
        let work = async { tokio::join!(write, child.wait_with_output()) };
        match tokio::time::timeout(self.timeout, work).await {
            Ok((Err(e), _)) => Err(format!("cannot write to `{}`: {e}", binary.display())),
            Ok((Ok(()), Ok(output))) => Ok(output),
            Ok((Ok(()), Err(e))) => Err(format!("cannot run `{}`: {e}", binary.display())),
            Err(_) => Err(format!(
                "`{} {}` did not answer in {} seconds",
                self.program,
                args.join(" "),
                self.timeout.as_secs_f64()
            )),
        }
    }

    /// Run the CLI and answer its standard output, or its own words on why
    /// it failed.
    async fn answer(&self, args: &[&str]) -> Result<String, String> {
        self.call(args).await.map_err(|refusal| refusal.to_string())
    }

    /// Run the CLI and answer its standard output, or the refusal: what the
    /// forge said, kept apart from the command, whose arguments carry a
    /// comment's whole text.
    pub(crate) async fn call(&self, args: &[&str]) -> Result<String, Refusal> {
        self.call_with_input(args, None).await
    }

    /// [`Cli::call`], with `input` on the CLI's standard input: a JSON body
    /// `gh api --input -` sends as it is.
    pub(crate) async fn call_with_input(
        &self,
        args: &[&str],
        input: Option<&str>,
    ) -> Result<String, Refusal> {
        let command = format!("{} {}", self.program, args.join(" "));
        let output = self.run(args, input).await.map_err(|message| Refusal {
            said: None,
            answer: None,
            message,
        })?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
        // `gh api` writes the forge's own answer — `{"message": ..., "errors":
        // [...]}`, which says why it refused — to stdout, and only a line
        // such as "gh: Unprocessable Entity (HTTP 422)" to stderr: both are
        // kept, or the reason is lost.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let said = [stderr.trim(), stdout.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" — ");
        let message = match said.is_empty() {
            true => format!("`{command}` failed with {}", output.status),
            false => format!("`{command}`: {said}"),
        };
        Err(Refusal {
            answer: Some(stdout.trim().to_string()).filter(|answer| !answer.is_empty()),
            said: Some(said).filter(|said| !said.is_empty()),
            message,
        })
    }
}

/// A forge CLI call that failed: what the CLI and the forge said, apart
/// from the command it ran. Only what was said is read for why: the
/// command's arguments hold a comment's whole text, which can say anything.
#[derive(Debug, Clone)]
pub(crate) struct Refusal {
    /// The CLI's and the forge's words; None where nothing answered — the
    /// CLI did not start, or did not answer in time.
    said: Option<String>,
    /// The forge's own answer alone, as the CLI printed it to standard
    /// output: `gh api`'s JSON, `{"message": ..., "errors": [...]}`.
    answer: Option<String>,
    message: String,
}

impl Refusal {
    /// Whether the forge answered that what the call named is not there:
    /// `gh` and `glab` both say "404" for it. A server error, a timeout or a
    /// lost answer is no proof the thing is gone.
    pub(crate) fn is_missing(&self) -> bool {
        self.said
            .as_deref()
            .is_some_and(|said| said.contains("HTTP 404") || said.contains("404 Not Found"))
    }

    /// The forge's own answer as JSON, where it gave one.
    pub(crate) fn answer(&self) -> Option<serde_json::Value> {
        self.answer
            .as_deref()
            .and_then(|answer| serde_json::from_str(answer).ok())
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refusal is read for what the forge said, never for the command:
    /// a summary whose text says "HTTP 404" is no 404 from the forge.
    /// A CLI that stalls before it reads an input larger than the pipe holds
    /// is stopped by the deadline, the write with it, and its call answers
    /// that it did not answer.
    #[tokio::test]
    async fn a_cli_that_reads_no_input_is_stopped_by_the_deadline() {
        let cli = Cli {
            configured: Some("/bin/sleep".into()),
            program: "gh",
            timeout: Duration::from_millis(500),
        };
        let input = "x".repeat(1024 * 1024);
        let started = std::time::Instant::now();
        let refused = cli
            .call_with_input(&["30"], Some(&input))
            .await
            .expect_err("a stalled CLI answers nothing");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        assert!(refused.to_string().contains("did not answer"), "{refused}");
    }

    #[test]
    fn a_refusal_is_missing_only_where_the_forge_said_404() {
        // What a refused edit of a summary titled "P1: Handle HTTP 404
        // responses" carries: the title is in the message, never in `said`.
        let refusal = |said: Option<&str>| Refusal {
            said: said.map(str::to_string),
            answer: None,
            message: "`gh api -f body=P1: Handle HTTP 404 responses`".into(),
        };
        assert!(!refusal(Some("gh: Server Error (HTTP 502)")).is_missing());
        assert!(!refusal(None).is_missing(), "a timeout is no 404");
        assert!(refusal(Some("gh: Not Found (HTTP 404)")).is_missing());
        assert!(refusal(Some("404 Not Found")).is_missing());
    }

    #[test]
    fn pull_request_urls_share_the_repository_and_number_only() {
        let mut integration = ForgeIntegration {
            webhook_id: None,
            webhook_secret: None,
            webhook_url: None,
            webhook_state: "polling".into(),
            webhook_error: None,
            webhook_last_delivery_at: None,
            fetch_error: None,
            repository_id: "repository".into(),
            kind: "github".into(),
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widgets".into(),
            remote: "origin".into(),
            enabled: true,
            login: Some("me".into()),
            review_model: None,
            review_effort: None,
            detected_at: String::new(),
            updated_at: String::new(),
        };
        let expected = Some(PullRequestRef {
            repository_id: "repository".into(),
            number: 42,
        });
        for url in [
            "https://github.com/acme/widgets/pull/42",
            "http://GITHUB.com/ACME/Widgets/pull/42/",
            "https://github.com/acme/widgets/pull/42?diff=split#comment-1",
        ] {
            assert_eq!(PullRequestRef::parse(url, &integration), expected, "{url}");
        }
        for url in [
            "https://github.com/other/widgets/pull/42",
            "https://evil.example/acme/widgets/pull/42",
            "https://github.com/acme/widgets/issues/42",
            "file://github.com/acme/widgets/pull/42",
            "https://github.com/acme/widgets/pull/0",
            "https://github.com/acme/widgets/pull/42/files",
        ] {
            assert_eq!(PullRequestRef::parse(url, &integration), None, "{url}");
        }
        integration.kind = "gitlab".into();
        integration.host = "gitlab.example".into();
        integration.owner = "group/sub".into();
        for url in [
            "https://gitlab.example/group/sub/widgets/merge_requests/42",
            "http://GITLAB.example/GROUP/Sub/Widgets/-/merge_requests/42/?a=b#note",
        ] {
            assert_eq!(PullRequestRef::parse(url, &integration), expected, "{url}");
        }
    }

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
