//! GitManager: repo validation, worktrees, branches, merge verification,
//! diffs.
//!
//! Shells out to `git` — worktree support in libgit2/gitoxide is weak and the
//! CLI is the canonical implementation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

/// Git's own hash of the empty tree: the "before" of a branch that starts from
/// no commit at all.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, Clone, Default)]
pub struct GitManager;

impl GitManager {
    pub(crate) async fn committed_clean(
        &self,
        repo: &Path,
        worktree: &Path,
        base: &str,
        branch: &str,
    ) -> Result<bool> {
        let ahead = self
            .git(repo, &["rev-list", "--count", &format!("{base}..{branch}")])
            .await?;
        let status = self.git(worktree, &["status", "--porcelain"]).await?;
        Ok(ahead.trim().parse::<u64>()? > 0 && status.trim().is_empty())
    }

    async fn git(&self, repo: &Path, args: &[&str]) -> Result<String> {
        let mut command = Command::new("git");
        if args.first() == Some(&"push") {
            command.env("GIT_TERMINAL_PROMPT", "0");
        }
        let output = command
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .await
            .context("git could not start")?;
        if !output.status.success() {
            bail!(
                "git {} failed in {}: {}",
                args.join(" "),
                repo.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Create a task worktree on `branch` (created at `base` when new).
    ///
    /// A repository nobody has committed to yet has an unborn base branch,
    /// which no commit can be cut from: the task branch starts unborn as well,
    /// and the first column's first commit is the repository's first commit.
    pub async fn add_worktree(
        &self,
        repo: &Path,
        worktree: &Path,
        branch: &str,
        base: &str,
    ) -> Result<()> {
        let wt = worktree.display().to_string();
        if self.branch_exists(repo, branch).await? {
            // Respawn after a crash: reuse the existing task branch.
            self.git(repo, &["worktree", "add", &wt, branch]).await?;
        } else if self.branch_exists(repo, base).await? {
            self.git(repo, &["worktree", "add", "-b", branch, &wt, base])
                .await?;
        } else {
            self.git(repo, &["worktree", "add", "--orphan", "-b", branch, &wt])
                .await?;
        }
        Ok(())
    }

    /// Create a detached worktree at `reference`, for a session that reviews
    /// a request (029): two worktrees cannot share a branch.
    pub async fn add_detached_worktree(
        &self,
        repo: &Path,
        worktree: &Path,
        reference: &str,
    ) -> Result<()> {
        let wt = worktree.display().to_string();
        self.git(repo, &["worktree", "add", "--detach", &wt, reference])
            .await?;
        Ok(())
    }

    /// Move a detached worktree to a new detached position, e.g. the head of
    /// a request on its next push (029).
    ///
    /// Whatever the session left in the tree is thrown away first. The tree
    /// is read-only by contract, but a session that proves a test fails by
    /// breaking the code leaves edits behind, and a plain checkout refuses to
    /// overwrite them: the tree then never moves again, and the session is
    /// never started for the next round. Ignored files — build caches — stay.
    pub async fn checkout_detached(&self, worktree: &Path, reference: &str) -> Result<()> {
        self.git(worktree, &["checkout", "--force", "--detach", reference])
            .await?;
        self.git(worktree, &["clean", "-fd"]).await?;
        Ok(())
    }

    pub async fn remove_worktree(&self, repo: &Path, worktree: &Path) -> Result<()> {
        let wt = worktree.display().to_string();
        self.git(repo, &["worktree", "remove", "--force", &wt])
            .await?;
        Ok(())
    }

    pub(crate) async fn prune_worktrees(&self, repo: &Path) -> Result<()> {
        self.git(repo, &["worktree", "prune"]).await?;
        Ok(())
    }

    pub async fn branch_exists(&self, repo: &Path, branch: &str) -> Result<bool> {
        let args = [
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ];
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .await
            .context("git could not start")?;
        if output.status.success() {
            return Ok(true);
        }
        if output.status.code() == Some(1) {
            return Ok(false);
        }
        bail!(
            "git {} failed in {}: {}",
            args.join(" "),
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }

    /// The commit `branch` points at, as a full sha.
    pub(crate) async fn branch_tip(&self, repo: &Path, branch: &str) -> Result<String> {
        self.git(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )
        .await
    }

    /// Where the repository keeps the refs every one of its worktrees shares.
    ///
    /// `<repo>/.git` for an ordinary checkout, but a repository registered at
    /// a linked worktree or a bare clone keeps them elsewhere, and only git
    /// knows where — which matters to whoever watches a branch, since a task
    /// branch is written there whichever tree commits to it.
    pub(crate) async fn common_dir(&self, repo: &Path) -> Result<PathBuf> {
        let args = ["rev-parse", "--path-format=absolute", "--git-common-dir"];
        Ok(PathBuf::from(self.git(repo, &args).await?))
    }

    /// Ensure `path` is an existing git work tree.
    pub(crate) async fn validate_repo(&self, path: &Path) -> Result<()> {
        if !path.is_dir() {
            bail!(
                "repo path does not exist or is not a directory: {}",
                path.display()
            );
        }
        let inside = self
            .git(path, &["rev-parse", "--is-inside-work-tree"])
            .await?;
        if inside != "true" {
            bail!("not a git work tree: {}", path.display());
        }
        Ok(())
    }

    /// Current branch of the repo (used as default base branch).
    pub(crate) async fn current_branch(&self, repo: &Path) -> Result<String> {
        self.git(repo, &["symbolic-ref", "--short", "HEAD"])
            .await
            .with_context(|| {
                format!(
                    "cannot resolve current branch of {} (detached HEAD?)",
                    repo.display()
                )
            })
    }

    /// Whether `reference` names a commit the repository holds: the head of
    /// a request under review, before it is fetched (029). A reference git
    /// cannot read as one is `false`.
    pub(crate) async fn has_commit(&self, repo: &Path, reference: &str) -> Result<bool> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "--verify", "--quiet", "--end-of-options"])
            .arg(format!("{reference}^{{commit}}"))
            .output()
            .await
            .context("git could not start")?;
        Ok(output.status.success())
    }

    /// Fetch `branch` from `from` — a remote name or a clone URL — into
    /// `FETCH_HEAD` alone: the head of a request the user reviews, which may
    /// share its name with a branch of the checkout (029).
    pub(crate) async fn fetch_head_of(&self, repo: &Path, from: &str, branch: &str) -> Result<()> {
        self.git(repo, &["fetch", from, &format!("refs/heads/{branch}")])
            .await?;
        Ok(())
    }

    /// The tip `branch` has on `from` — a remote name or a clone URL — as a
    /// full sha, fetched into `FETCH_HEAD` alone: the base a request merged
    /// into, which the checkout's own branch may not have caught up with
    /// (005).
    pub(crate) async fn fetched_tip(
        &self,
        repo: &Path,
        from: &str,
        branch: &str,
    ) -> Result<String> {
        self.fetch_head_of(repo, from, branch).await?;
        self.git(repo, &["rev-parse", "--verify", "FETCH_HEAD^{commit}"])
            .await
    }

    /// Move the local `branch` forward to `to`, a commit already in the
    /// repository, and only forward (005): a branch that already holds `to`
    /// is left as it is, one with commits `to` lacks is refused, and a
    /// missing one is created at `to`. Where a
    /// worktree has the branch checked out, `git merge --ff-only` runs in
    /// it, which refuses to overwrite a local change; elsewhere the ref is
    /// moved with `update-ref`, guarded by its old value.
    pub(crate) async fn fast_forward(&self, repo: &Path, branch: &str, to: &str) -> Result<()> {
        if !self.branch_exists(repo, branch).await? {
            // No local branch yet: it starts where the remote's is.
            self.git(repo, &["update-ref", &format!("refs/heads/{branch}"), to])
                .await?;
            return Ok(());
        }
        let tip = self.branch_tip(repo, branch).await?;
        if self.is_ancestor(repo, to, &tip).await? {
            return Ok(());
        }
        if !self.is_ancestor(repo, &tip, to).await? {
            bail!("{branch} has commits that {to} lacks: it cannot be fast-forwarded");
        }
        let listed = self.git(repo, &["worktree", "list", "--porcelain"]).await?;
        let wanted = format!("branch refs/heads/{branch}");
        let mut path = None;
        let mut checked_out = None;
        for line in listed.lines() {
            if let Some(at) = line.strip_prefix("worktree ") {
                path = Some(at.to_string());
            } else if line == wanted {
                checked_out = path.clone();
            }
        }
        match checked_out {
            Some(worktree) => {
                self.git(Path::new(&worktree), &["merge", "--ff-only", "--quiet", to])
                    .await?;
            }
            None => {
                self.git(
                    repo,
                    &["update-ref", &format!("refs/heads/{branch}"), to, &tip],
                )
                .await?;
            }
        }
        Ok(())
    }

    /// The diff of `HEAD` in `worktree` from the commit `since`:
    /// `git diff <since>..HEAD` (029).
    pub(crate) async fn diff_since(&self, worktree: &Path, since: &str) -> Result<String> {
        self.git(worktree, &["diff", &format!("{since}..HEAD")])
            .await
    }

    pub async fn delete_branch(&self, repo: &Path, branch: &str) -> Result<()> {
        self.git(repo, &["branch", "-D", branch]).await?;
        Ok(())
    }

    /// Whether `branch`'s local tip is also what `remote` has for it, read
    /// with `git ls-remote` rather than a fetch: the push check before the
    /// daemon opens a request, and the merge verification before it accepts
    /// `complete_step` for one.
    pub async fn remote_has_branch_tip(
        &self,
        repo: &Path,
        remote: &str,
        branch: &str,
    ) -> Result<bool> {
        let local = self.git(repo, &["rev-parse", branch]).await?;
        let refs = self
            .git(repo, &["ls-remote", "--heads", remote, branch])
            .await?;
        Ok(refs
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next())
            .is_some_and(|sha| sha == local))
    }

    /// True when `ancestor` is reachable from `descendant` — the merge
    /// verification used before accepting a merge column's `complete_step`.
    pub async fn is_ancestor(&self, repo: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .output()
            .await
            .context("running git merge-base")?;
        Ok(output.status.success())
    }

    /// Diff of the task branch against its merge base with `base`
    /// (`git diff base...branch`).
    ///
    /// There is no merge base when the repository had no commits as the task
    /// started: the base branch was unborn and the task branch was cut orphan
    /// from it. Then the whole branch is the change, and the empty tree is
    /// what it is read against.
    pub async fn diff(&self, repo: &Path, base: &str, branch: &str) -> Result<String> {
        if self.git(repo, &["merge-base", base, branch]).await.is_err() {
            return self.git(repo, &["diff", EMPTY_TREE, branch]).await;
        }
        self.git(repo, &["diff", &format!("{base}...{branch}")])
            .await
    }

    /// What `commit` brought into the branch it landed on: the diff against
    /// its first parent. For a task's merge commit that is the task's whole
    /// change as merged; works for fast-forward (single-parent) commits too.
    ///
    /// A root commit has no first parent, and `{commit}^1` names nothing at
    /// all there — git refuses the revision rather than reading it as empty.
    /// It is reachable: a repository with no commits is one Ariadne works in
    /// (002), and the first task to land in one lands the commit the whole
    /// repository starts from. So the empty tree stands in, the same way it
    /// does for the orphan branch that commit was written on ([`diff`]) —
    /// without it the diff of that task is a 409 for as long as the task
    /// exists, and after the cleanup there is nothing else left to read.
    pub async fn diff_against_first_parent(&self, repo: &Path, commit: &str) -> Result<String> {
        let parent = format!("{commit}^1");
        // `--verify --quiet` answers the question and prints nothing when the
        // answer is no: a root commit is not an error to report, it is the
        // other branch of this method.
        let before = match self
            .git(repo, &["rev-parse", "--verify", "--quiet", &parent])
            .await
        {
            Ok(_) => parent,
            Err(_) => EMPTY_TREE.to_string(),
        };
        self.git(repo, &["diff", &before, commit]).await
    }
}
