---
name: merge
description: Use when a reviewed task needs landing.
---

# Merge

Complete the merge work. Done when the conditions in `## Done` hold.

The primary checkout is the `Repo:` path of your briefing. `<upstream>` is
`<remote>/<base>` where `git remote -v` names a remote, else `<base>`.

Read the repository policy first. Distinguish base history from task history.
For Ariadne, direct landing permits the sequence below on an unpublished task branch.
For a published Ariadne branch, merge the base instead of rebasing published commits.
Squash the final result before pushing to main. Never rewrite main.
Keep generic direct landing available where the repository permits it.

## When not to use

Use `pr-babysit` when its triggers apply.

## Steps

1. Where a remote exists, run `git fetch <remote> <base>`.

   Done when the upstream ref is current.
2. For an unpublished branch, rebase the task branch onto the current `<upstream>`. Resolve every
   conflict yourself.

   Done when every conflict is resolved.
3. Only after that rebase, squash the branch into one commit.
   For a published Ariadne branch, merge `<upstream>` first, then squash for direct landing. Give it a Conventional Commits subject:
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

## Rules

### Rules

- A refused push or a failed check is a fix on the task branch. Start
  again from step 1.
- A failed fast-forward means the base moved. Start again from step 1.
- Never push the task branch by its name.

## Never

Never force-push any branch or remote, including with `--force-with-lease`.

## Do not tell yourself

- "The push is enough." -> The base must hold the result.

## Done

The push passed the repository hooks. `<base>` holds the squashed commit and
nothing else of the task. `complete_step` carries its sha.
