---
name: merge
description: Land a reviewed task on the base branch. Rebase, squash, push and fast-forward.
---

# Merge

Land the reviewed task on the base branch, in your worktree. `<remote>`
is what `git remote -v` names, if it names one.

## Steps

1. Fetch the base: `git fetch <remote> <base>` where a remote exists.
   Fast-forward `<base>` with `--ff-only <remote>/<base>` where the primary
   checkout sits on `<base>`. Else fetch it straight onto `<base>`.
2. Rebase the task branch onto `<base>`. Fix every conflict yourself.
3. Squash the task branch onto its merge base with `<base>`:
   `git reset --soft "$(git merge-base <base> HEAD)" && git commit`,
   with a Conventional Commits subject.
4. Check `git diff --stat <base> HEAD`. It names only files of the
   task. A wider list means the base moved; start again from step 1.
5. Where a remote exists, push `HEAD:<base>` to `<remote>` from the task
   worktree. The repository's hooks run its checks on the push.
6. Fast-forward `<base>` in the primary checkout with
   `git merge --ff-only <remote>/<base>`.
7. With no remote, run the checks the repository instructions name for a
   landing. Then fast-forward `<base>` in the primary checkout onto the task
   branch.
8. Call `complete_step`. Give it the base branch's sha as `merge_commit`.

## Rules

- A refused push is a fix on the task branch. Make it, then start again
  from step 1.
- A rejected non-fast-forward push means the base moved. Start again from
  step 1.
- Never force a push to `<base>`.
- A moved base is not a conflict. Fetch it again and start over.

## Done

The push passed the repository hooks. `<base>` holds the squashed commit and
nothing else of the task. `complete_step` carries its sha.
