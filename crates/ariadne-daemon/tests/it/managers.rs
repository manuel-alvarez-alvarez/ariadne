//! Integration tests for GitManager.
//!
//! These need `git` on PATH and touch real repositories, so they run with the
//! rest of the suite rather than behind `#[ignore]`.

use crate::common;

use std::path::PathBuf;

use ariadne_daemon::gitwt::GitManager;

use common::sh;

/// A toy repo with an initial commit on `main`.
fn toy_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    sh(
        &repo,
        "git init -q -b main && git config user.email t@t && git config user.name t && \
               echo v1 > file.txt && git add . && git commit -qm init",
    );
    (dir, repo)
}

#[tokio::test]
async fn git_worktree_lifecycle_and_merge_verification() {
    let (dir, repo) = toy_repo();
    let git = GitManager;

    // The task's worktree on a new branch.
    let wt = dir.path().join("wt-eng");
    git.add_worktree(&repo, &wt, "fix-the-widget-aaa111", "main")
        .await
        .unwrap();
    assert!(wt.join("file.txt").exists());

    // Commit on the task branch.
    sh(
        &wt,
        "echo v2 > file.txt && git add . && git commit -qm change",
    );

    // A pull request review's worktree, detached at the branch tip.
    let wt_rev = dir.path().join("wt-rev");
    git.add_detached_worktree(&repo, &wt_rev, "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt_rev.join("file.txt"))
            .unwrap()
            .trim(),
        "v2"
    );

    // Diff base...branch shows the change.
    let diff = git
        .diff(&repo, "main", "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert!(
        diff.contains("-v1") && diff.contains("+v2"),
        "unexpected diff: {diff}"
    );

    // Not merged yet.
    assert!(
        !git.is_ancestor(&repo, "fix-the-widget-aaa111", "main")
            .await
            .unwrap()
    );

    // Merge in the primary checkout (what the merge column's agent does).
    sh(&repo, "git merge -q --no-ff fix-the-widget-aaa111 -m merge");
    assert!(
        git.is_ancestor(&repo, "fix-the-widget-aaa111", "main")
            .await
            .unwrap()
    );

    // Cleanup: remove worktrees, delete branch.
    git.remove_worktree(&repo, &wt).await.unwrap();
    git.remove_worktree(&repo, &wt_rev).await.unwrap();
    git.delete_branch(&repo, "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert!(
        !git.branch_exists(&repo, "fix-the-widget-aaa111")
            .await
            .unwrap()
    );

    // A new commit lands after crash: re-adding a worktree for an existing
    // branch reuses it.
    git.add_worktree(&repo, &wt, "fix-the-widget-bbb222", "main")
        .await
        .unwrap();
    git.remove_worktree(&repo, &wt).await.unwrap();
    git.add_worktree(&repo, &wt, "fix-the-widget-bbb222", "main")
        .await
        .unwrap();
    git.remove_worktree(&repo, &wt).await.unwrap();
}

/// A detached worktree — what a pull request review runs in (029) — is
/// moved to the branch's new tip, and whatever was left in it goes.
#[tokio::test]
async fn a_detached_worktree_is_refreshed_to_the_new_tip() {
    let (dir, repo) = toy_repo();
    let git = GitManager;

    let wt = dir.path().join("wt-eng");
    git.add_worktree(&repo, &wt, "fix-the-widget-aaa111", "main")
        .await
        .unwrap();
    sh(&wt, "echo r1 > file.txt && git add . && git commit -qm r1");

    let wt_rev = dir.path().join("wt-rev");
    git.add_detached_worktree(&repo, &wt_rev, "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt_rev.join("file.txt"))
            .unwrap()
            .trim(),
        "r1"
    );

    // More commits on the branch; the detached worktree is refreshed.
    sh(&wt, "echo r2 > file.txt && git add . && git commit -qm r2");
    git.checkout_detached(&wt_rev, "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt_rev.join("file.txt"))
            .unwrap()
            .trim(),
        "r2"
    );

    // The review broke the code to prove a test, and left the edit and a
    // scratch file behind. The refresh still moves the tree, and what the
    // review left is gone.
    sh(&wt, "echo r3 > file.txt && git add . && git commit -qm r3");
    sh(&wt_rev, "echo broken > file.txt && echo scratch > red.txt");
    git.checkout_detached(&wt_rev, "fix-the-widget-aaa111")
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt_rev.join("file.txt"))
            .unwrap()
            .trim(),
        "r3"
    );
    assert!(!wt_rev.join("red.txt").exists());
}

/// A repository nobody has committed to yet: the base branch is unborn, so the
/// task's worktree is cut orphan and its first commit is the repository's.
/// The diff has no merge base to be read against, and is the whole branch —
/// nor, once it has landed, a first parent, since it is the commit the
/// repository starts from.
#[tokio::test]
async fn a_worktree_is_cut_from_a_base_branch_with_no_commits() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    sh(
        &repo,
        "git init -q -b main && git config user.email t@t && git config user.name t",
    );
    let git = GitManager;
    assert!(!git.branch_exists(&repo, "main").await.unwrap());

    let wt = dir.path().join("wt-eng");
    git.add_worktree(&repo, &wt, "first-task-aaa111", "main")
        .await
        .unwrap();
    // Nothing to check out, and no branch until the first column commits.
    assert!(!wt.join("file.txt").exists());
    assert!(!git.branch_exists(&repo, "first-task-aaa111").await.unwrap());

    sh(
        &wt,
        "echo v1 > file.txt && git add . && git commit -qm first",
    );
    assert!(git.branch_exists(&repo, "first-task-aaa111").await.unwrap());

    // The whole branch is the change.
    let diff = git.diff(&repo, "main", "first-task-aaa111").await.unwrap();
    assert!(diff.contains("+v1"), "{diff}");

    // And the base fast-forwards onto it, which is what the merge column does.
    sh(&repo, "git merge --ff-only first-task-aaa111");
    assert!(
        git.is_ancestor(&repo, "first-task-aaa111", "main")
            .await
            .unwrap()
    );

    // What the landed task reads as afterwards, which is the only diff of it
    // left once the branch and the worktree are cleaned up. Its commit has no
    // parent to be read against, so it is read against the empty tree.
    let landed = sh(&repo, "git rev-parse HEAD");
    let diff = git
        .diff_against_first_parent(&repo, landed.trim())
        .await
        .unwrap();
    assert!(diff.contains("+v1"), "{diff}");

    git.remove_worktree(&repo, &wt).await.unwrap();
}
