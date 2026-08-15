//! Minimal git helpers used at the API layer.
//!
//! The full GitManager (worktrees, merges, diffs) arrives with milestone 5;
//! goal creation only needs repo validation and base-branch resolution.

use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

async fn git(repo: &Path, args: &[&str]) -> Result<String> {
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

/// Ensure `path` is an existing git work tree.
pub async fn validate_repo(path: &Path) -> Result<()> {
    if !path.is_dir() {
        bail!(
            "repo path does not exist or is not a directory: {}",
            path.display()
        );
    }
    let inside = git(path, &["rev-parse", "--is-inside-work-tree"]).await?;
    if inside != "true" {
        bail!("not a git work tree: {}", path.display());
    }
    Ok(())
}

/// Current branch of the repo (used as default base branch).
pub async fn current_branch(path: &Path) -> Result<String> {
    git(path, &["symbolic-ref", "--short", "HEAD"])
        .await
        .with_context(|| {
            format!(
                "cannot resolve current branch of {} (detached HEAD?)",
                path.display()
            )
        })
}

/// Ensure a branch points at a real commit. Catches freshly `git init`ed
/// repos where HEAD names an unborn branch: worktrees cannot be created
/// from it, so fail goal creation with a clear message instead.
pub async fn ensure_branch_has_commits(path: &Path, branch: &str) -> Result<()> {
    git(
        path,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
    )
    .await
    .map(|_| ())
    .map_err(|_| {
        anyhow::anyhow!(
            "branch {branch} of {} has no commits yet — make an initial commit first",
            path.display()
        )
    })
}

/// Verify a branch exists in the repo.
pub async fn branch_exists(path: &Path, branch: &str) -> Result<bool> {
    Ok(git(
        path,
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
