---
name: merge
description: Use when a reviewed task needs landing on its base branch.
---

# Merge

Land the reviewed task on its base branch as one squashed commit. Done when the conditions in `## Done` hold.

## When not to use

Use `pr-babysit` when the task lands through a request.

## Steps

The primary checkout is the `Repo:` path of your briefing. `<upstream>` is
`<remote>/<base>` where `git remote -v` names a remote, else `<base>`.

Read the repository policy first. Ariadne permits direct landing of an
unpublished task branch.

1. Where a remote exists, run `git fetch <remote> <base>`.

   Done when the upstream ref is current.
2. For an unpublished branch, rebase the task branch onto the current `<upstream>`.
   For a published branch, merge `<upstream>` into it instead. Where the
   rebase or the merge stops on a conflict, follow `conflict-resolution`.

   Done when the branch holds the current `<upstream>`.
3. Only after that rebase, squash the branch into one commit. Give it a
   Conventional Commits subject:
   `git reset --soft "$(git merge-base <upstream> HEAD)" && git commit`.

   Done when one commit holds the task.
4. Run `git diff --stat <upstream> HEAD`. It names only files of the task.

   Done when the list holds no other file.
5. With a remote, run:

   ```sh
   git push <remote> HEAD:<base>
   ```

   The hooks run the checks. Then fast-forward `<base>` in the primary
   checkout:
   - `git -C <repo> merge --ff-only <remote>/<base>` where `<base>` is checked out there
   - else `git -C <repo> fetch <remote> <base>:<base>`.

   Done when the primary checkout is current.
6. With no remote: run the checks the repository names for a landing. Then
   fast-forward `<base>` in the primary checkout to the task branch.

   Done when the base holds the task.
7. Call `complete_step` with the column report and the sha of `<base>`
   as `merge_commit`.

   Done when the report has been submitted.

A refused push or a failed check is a fix on the task branch. Start again
from step 1. A failed fast-forward means the base moved. Start again from
step 1.

## Never

Never force-push any branch or remote, including with `--force-with-lease`.
Never push the task branch by its name.

## Do not tell yourself

- "The push is enough." -> The base must hold the result.
- "The base name is a safe squash point." -> The name moves. Squash onto the merge base.

## Done

The push passed the repository hooks. `<base>` holds the squashed commit and
nothing else of the task. `complete_step` carries its sha.
