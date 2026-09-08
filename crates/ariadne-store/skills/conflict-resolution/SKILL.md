---
name: conflict-resolution
description: Resolve a merge or rebase conflict by the intent of each side. Use when a rebase stops, a merge conflicts, or markers block a landing.
---

# Conflict resolution

A conflict is two intents that meet in one file. Resolve it by intent.

## Steps

1. Read the conflict state. Run `git status` and open every conflicting file.
   Done when you can name each file and the operation in progress.
2. Find the intent of each side. Read both commit messages, both diffs and the
   ticket each one names.
   Done when you can state each intent in one sentence.
3. Resolve each hunk. Keep both intents where they fit together.
   Done when no conflict marker is left.
4. Where two intents clash, keep the one the landing goal wants. Write the
   trade-off in the commit body.
   Done when the commit body names each intent you dropped.
5. Run the repository's own checks: the build, the tests and the linters.
   Done when each check passes.
6. Finish the operation. Stage the files, then continue the rebase or commit
   the merge.
   Done when git reports no operation in progress.

## Rules

- Carry over only behavior one side already wrote.
- Work every hunk to a resolution. Never abort the operation.
- Read the code around a hunk. The intent is often outside the markers.
- Fix what the resolution broke, in the same commit.

## Do not tell yourself

- "Mine is newer, so mine wins." -> Age ranks no intent. Read both sides.
- "The tests pass, so the merge is right." -> No test covers an intent you
  dropped.
- "I will put it back later." -> A dropped intent is invisible once the markers
  are gone.

## Done

No conflict marker is left. Both intents are in the result, or the dropped one
is written in the commit body. The checks pass. Git reports no merge and no
rebase in progress.
