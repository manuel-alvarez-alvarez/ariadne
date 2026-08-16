//! GitManager: worktrees, branches, merge verification, diffs.
//!
//! Shells out to `git` — worktree support in libgit2/gitoxide is weak and the
//! CLI is the canonical implementation.

use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

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

    /// Create an engineer worktree on `branch` (created at `base` when new).
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
        } else {
            self.git(repo, &["worktree", "add", "-b", branch, &wt, base])
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
        Ok(self
            .git(
                repo,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{branch}"),
                ],
            )
            .await
            .is_ok())
    }

    pub async fn delete_branch(&self, repo: &Path, branch: &str) -> Result<()> {
        self.git(repo, &["branch", "-D", branch]).await?;
        Ok(())
    }

    /// True when `ancestor` is reachable from `descendant` — the merge
    /// verification used before accepting `mark_merged`.
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

    pub async fn rev_parse(&self, repo: &Path, reference: &str) -> Result<String> {
        self.git(repo, &["rev-parse", reference]).await
    }

    /// Diff of the task branch against its merge base with `base`
    /// (`git diff base...branch`).
    pub async fn diff(&self, repo: &Path, base: &str, branch: &str) -> Result<String> {
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
