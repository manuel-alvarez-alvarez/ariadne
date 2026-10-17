---
name: merge
description: Land a reviewed task on the base branch. Rebase, squash, push and fast-forward.
---

# Merge

Work from your worktree. The primary checkout is the `Repo:` path of your
briefing. `<upstream>` is `<remote>/<base>` where `git remote -v` names a
remote, else `<base>`.

## Steps

1. Where a remote exists, run `git fetch <remote> <base>`.
2. Rebase the task branch onto `<upstream>`. Resolve every conflict
   yourself.
3. Squash the branch into one commit. Give it a Conventional Commits subject:
   `git reset --soft "$(git merge-base <upstream> HEAD)" && git commit`.
4. Run `git diff --stat <upstream> HEAD`. It names only files of the task.
   Done when the list holds no other file.
5. With a remote: push `HEAD:<base>` to `<remote>`. The hooks run the
   checks. Then fast-forward `<base>` in the primary checkout:
   - `git -C <repo> merge --ff-only <remote>/<base>` where `<base>` is checked out there
   - else `git -C <repo> fetch <remote> <base>:<base>`.
6. With no remote: run the checks the repository names for a landing. Then
   fast-forward `<base>` in the primary checkout to the task branch.
7. Call `complete_step` with the sha of `<base>` as `merge_commit`.

## Rules

- A refused push or a failed check is a fix on the task branch. Start
  again from step 1.
- A failed fast-forward means the base moved. Start again from step 1.
- Never force a push to `<base>`.

## Done

The push passed the repository hooks. `<base>` holds the squashed commit and
nothing else of the task. `complete_step` carries its sha.
