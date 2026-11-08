---
name: migration
description: Use when a schema shape changes, data moves, or a caller retires.
---

# Migration

Complete the migration work. Done when the conditions in `## Done` hold.

Data outlives code. Use the expand-contract sequence to protect each data
shape.

## When not to use

Use `coding` when its triggers apply.

## Steps

1. Write the old shape, new shape and every caller.

   Done when each caller names the shape it reads and writes.
2. Write the recovery path before you write any migration.

   Done when a tested reverse path or a backup is ready.
3. Rehearse the whole sequence on a copy of real data.

   Done when the copy keeps every row and its required values.
4. Expand first. Add the new shape beside the old shape.

   Done when old and new callers both run against the expanded shape.
5. Write both shapes while callers overlap.

   Done when each new write fills both shapes.
6. Migrate existing data in batches, off the hot path. Record the duration
   and the resource use at real size.

   Done when every existing row holds the new shape.
7. Switch callers to read the new shape.

   Done when each caller reads the new shape.
8. Leave the contract to a later task. Call `complete_step` with the
   column report, naming the deferred work.

   Done when the report names the deferred work.

## Rules

### Rules

- Keep old and new shapes readable while callers overlap.
- Put destructive contract work in a separate later deploy.
- Back up before destructive shape work.
- Never drop or rename a live shape.

## Never

Never change work outside the task.

## Do not tell yourself

- "One rename is safe." -> Expand first, migrate in batches, then contract.
- "One update will finish fast." -> Run batches to protect table access.
- "Old callers are gone." -> Prove no caller remains with search and usage.

## Done

The expand-contract sequence preserves every row. The duration is known. No
caller uses the old shape. A recovery path is ready.
