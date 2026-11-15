---
name: conflict-resolution
description: Resolves each conflicting hunk to the intent of both sides. Use when a rebase or merge stops on a conflict.
---

# Conflict resolution

Resolve each conflict by intent, keeping both sides' meaning.

A conflict is two intents in one file.

## When not to use

Use `merge` when no conflict stops the landing. Use `debugging` when a check fails and no conflict is open.

## Steps

1. Read the conflict state. Run `git status` and open every conflicting file.
   Where the operation started in error, such as on the wrong base, abort it
   and start the correct one.

   Done when you can name each file and the operation in progress.
2. Read both sides before you keep either. Read both commit messages, both
   diffs, the task each one names and the code around each hunk.

   Done when you can state each intent in one sentence.
3. Resolve each hunk to the intent of both sides. Where the intents fit
   together, keep both. Carry over only behavior one side already wrote.
   Resolve the source of a generated file or a lock file by intent. Then
   make that file again with its tool. Run `git rebase --skip` only for a
   commit whose change the base already holds.

   Done when only hunks with clashing intents hold a conflict marker.
4. Where two intents clash, keep the one the goal of the task wants. Write
   each dropped intent and the reason in the commit body.

   Done when no conflict marker is left and the commit body names each
   dropped intent.
5. Run the checks of what you resolved. Fix what the resolution broke in the
   same commit.

   Done when each check passes.
6. Finish the operation. Stage the files, then continue the rebase or commit
   the merge.

   Done when git reports no operation in progress.
7. Where another skill invoked this one, give it each dropped intent and
   return to that skill's next step. Call nothing. Where no skill did, this
   step ends the column. Call `complete_step` with the column report. Put
   each dropped intent in it.

   Done when the caller has its next step or the column is reported.

## Never

Never take one side of a hunk whole. The one exception is a generated file
or a lock file that you make again with its tool. Never abort an operation
that started correctly. Never change work outside the task.

## Do not tell yourself

- "Mine is newer, so mine wins." -> Age ranks no intent. Read both sides.
- "The tests pass, so the merge is right." -> No test covers an intent you
  dropped.
- "I will put it back later." -> A dropped intent is invisible once the markers
  are gone.
