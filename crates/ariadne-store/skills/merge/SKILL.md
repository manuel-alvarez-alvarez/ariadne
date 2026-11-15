---
name: merge
description: Lands a reviewed task on its base branch as one squashed commit. Use when a reviewed task needs landing on its base branch.
---

# Merge

Land the reviewed task on its base branch as one squashed commit.

## When not to use

Use `pr-babysit` when the task lands through a request.

## Steps

1. Where a remote exists, run `git fetch <remote> <base>`.

   Done when the upstream ref is current.
2. Read the repository's history rules first. Then run
   `git ls-remote --heads <remote> <branch>`. Where it names the branch, the
   branch is published: merge `<upstream>` into it. Else, or with no remote,
   rebase the task branch onto the current `<upstream>`. Where the rebase or
   the merge stops on a conflict, follow `conflict-resolution`. Its document
   is `../conflict-resolution/SKILL.md`, relative to the directory of this one.

   Done when the branch holds the current `<upstream>`.
3. Only after that rebase or merge, squash the branch into one commit. Run
   `git reset --soft "$(git merge-base <upstream> HEAD)" && git commit`.
   Take the subject from the repository's commit convention. Write a body of
   up to three sentences on what changed and why. Write no other line.

   Done when one commit holds the task.
4. Run `git diff --stat <upstream> HEAD`. It names only files of the task.

   Done when the list holds no other file.
5. With a remote, run:

   ```sh
   git push <remote> HEAD:<base>
   ```

   The hooks run the checks. Start one push. Where the shell returns before
   the push ends, the hook still runs: wait for that process to end. Start no
   second push while one runs. Then fast-forward `<base>` in the primary
   checkout:
   - `git -C <repo> merge --ff-only <remote>/<base>` where `<base>` is checked out there
   - else `git -C <repo> fetch <remote> <base>:<base>`.

   Done when the primary checkout is current.
6. With no remote: run the checks the repository names for a landing. Then
   fast-forward `<base>` in the primary checkout to the task branch.

   Done when the base holds the task.
7. Read the full sha with `git -C <repo> rev-parse <base>`. Call
   `complete_step` with the column report and that sha as `merge_commit`.

   Done when the report has been submitted.

## Rules

### Names

The primary checkout is the `Repo:` path of your briefing. `<upstream>` is
`<remote>/<base>` where `git remote -v` names a remote, else `<base>`.

### Where a failure goes

A hook that fails is a failed check, not a moved base. Take the first rule
that fits.

- A failed check that names a file of the task. Call `fail_step` with the
  check's output.
- A failed check that names no file of the task. Send it to the
  orchestrator with `send_message`. Land nothing.
- Else the base moved: a non-fast-forward rejection, an extra file in
  `git diff --stat`, or a failed fast-forward. Start again from step 1.

## Never

Never force-push any branch or remote, including with `--force-with-lease`.
Never push the task branch by its name. Never change a file of the task.
Never commit anything but the merge of step 2 and the squash. Never run a
check that the push hook runs.

## Do not tell yourself

- "The push is enough." -> The base must hold the result.
- "The base name is a safe squash point." -> `<base>` and `<upstream>` move.
  Squash onto the merge base.
- "The shell returned, so the push failed." -> The hook still runs. Wait for
  it; a second push fights the first.
- "The check failed, so I fix it here." -> The previous column owns the fix.
  Return the task.
- "I run the suite first, to be safe." -> The hook runs it once. A second run
  doubles the machine time.
