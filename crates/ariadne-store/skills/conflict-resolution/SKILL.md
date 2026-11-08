---
name: conflict-resolution
description: Use when a rebase or merge stops on a conflict.
---

# Conflict resolution

Resolve each conflict by intent, keeping both sides' meaning. Done when the conditions in `## Done` hold.

A conflict is two intents in one file.

## When not to use

Use `merge` when no conflict stops the landing. Use `debugging` when a check fails and no conflict is open.

## Steps

1. Read the conflict state. Run `git status` and open every conflicting file.

   Done when you can name each file and the operation in progress.
2. Read both sides before you keep either. Read both commit messages, both
   diffs, the task each one names and the code around each hunk.

   Done when you can state each intent in one sentence.
3. Resolve each hunk to the intent of both sides. Where the intents fit
   together, keep both. Carry over only behavior one side already wrote.

   Done when no conflict marker is left.
4. Where two intents clash, keep the one the goal of the task wants. Write
   each dropped intent and the reason in the commit body.

   Done when the commit body names each dropped intent.
5. Run the tests and the lint of the files you resolved. Fix what the
   resolution broke in the same commit.

   Done when each check passes.
6. Finish the operation. Stage the files, then continue the rebase or commit
   the merge.

   Done when git reports no operation in progress.
7. Call `complete_step` with the column report, unless another skill invoked
   this one. Then return to that skill's next step.

   Done when the column is reported or the caller has its next step.

## Never

Never take one side of a hunk whole. Never abort the operation. Never change
work outside the task.

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
