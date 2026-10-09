---
name: merge
description: Land a reviewed task on the base branch. Rebase, run the whole suite once, squash, fast-forward and push.
---

# Merge

Land the reviewed task on the base branch, in your worktree. `<remote>`
is what `git remote -v` names, if it names one.

## Steps

1. Fetch the base: `git fetch <remote> <base>`. Merge it with
   `--ff-only <remote>/<base>` where the primary checkout sits on
   `<base>`. Else fetch it straight onto `<base>`.
2. Rebase the task branch onto `<base>`. Fix every conflict yourself.
3. Run the whole suite, the build and the linters once.
4. Fix a failure on the task branch, then rebase again.
5. Squash the task branch onto its merge base with `<base>`:
   `git reset --soft "$(git merge-base <base> HEAD)" && git commit`,
   with a Conventional Commits subject.
6. Check `git diff --stat <base> HEAD`. It names only files of the
   task. A wider list means the base moved; start again from step 1.
7. Fast-forward `<base>` in the primary checkout onto the task branch.
   A failed fast-forward means the base moved; start again from step 1.
8. Push `<base>` where a remote exists.
9. Call `complete_step`. Give it the base branch's sha as `merge_commit`.

## Rules

- Run the suite once, after the rebase and before the fast-forward. A
  failure there is a fix on the task branch; a failure after it is a
  revert.
- Push `<base>` before you call `complete_step`. The call ends the
  worktree you ran it from.
- Never force a push to `<base>`.
- A moved base is not a conflict. Fetch it again and start over.

## Done

The suite passed on the rebased tree. `<base>` holds the squashed
commit and nothing else of the task. `complete_step` carries its sha.
