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
    async fn git(&self, repo: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .await
            .context("running git")?;
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

    /// Create an author worktree on `branch` (created at `base` when new).
    ///
    /// A repository nobody has committed to yet has an unborn base branch,
    /// which no commit can be cut from: the task branch starts unborn as well,
    /// and the author's first commit is the repository's first commit.
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

    /// Create a reviewer worktree, detached at `reference` (two worktrees
    /// cannot share a branch).
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

    /// Move a (reviewer) worktree to a new detached position, e.g. the task
    /// branch tip at the next review round.
    pub async fn checkout_detached(&self, worktree: &Path, reference: &str) -> Result<()> {
        self.git(worktree, &["checkout", "--detach", reference])
            .await?;
        Ok(())
    }

    pub async fn remove_worktree(&self, repo: &Path, worktree: &Path) -> Result<()> {
        let wt = worktree.display().to_string();
        self.git(repo, &["worktree", "remove", "--force", &wt])
            .await?;
        Ok(())
    }

    pub async fn prune_worktrees(&self, repo: &Path) -> Result<()> {
        self.git(repo, &["worktree", "prune"]).await?;
        Ok(())
    }

    pub async fn branch_exists(&self, repo: &Path, branch: &str) -> Result<bool> {
        Ok(self.branch_tip(repo, branch).await.is_ok())
    }

    /// The commit `branch` points at, as a full sha.
    pub async fn branch_tip(&self, repo: &Path, branch: &str) -> Result<String> {
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
    pub async fn common_dir(&self, repo: &Path) -> Result<PathBuf> {
        let args = ["rev-parse", "--path-format=absolute", "--git-common-dir"];
        Ok(PathBuf::from(self.git(repo, &args).await?))
    }

    /// Ensure `path` is an existing git work tree.
    pub async fn validate_repo(&self, path: &Path) -> Result<()> {
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
    pub async fn current_branch(&self, repo: &Path) -> Result<String> {
        self.git(repo, &["symbolic-ref", "--short", "HEAD"])
            .await
            .with_context(|| {
                format!(
                    "cannot resolve current branch of {} (detached HEAD?)",
                    repo.display()
                )
            })
    }

    /// Ensure a branch points at a real commit, saying which one does not when
    /// it is a branch nothing has been committed to yet.
    pub async fn ensure_branch_has_commits(&self, repo: &Path, branch: &str) -> Result<()> {
        if self.branch_exists(repo, branch).await? {
            return Ok(());
        }
        bail!("branch {branch} of {} has no commits yet", repo.display())
    }

    pub async fn delete_branch(&self, repo: &Path, branch: &str) -> Result<()> {
        self.git(repo, &["branch", "-D", branch]).await?;
        Ok(())
    }

    /// True when `ancestor` is reachable from `descendant` — the merge
    /// verification used before accepting `finish_task`.
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
    pub async fn diff_against_first_parent(&self, repo: &Path, commit: &str) -> Result<String> {
        self.git(repo, &["diff", &format!("{commit}^1"), commit])
            .await
    }
}
