---
name: migration
description: Move a schema or data shape forward without loss. Use when schema shapes change, data moves, or old callers retire.
---

# Migration

Data outlives code. Use the expand-contract sequence to protect each data
shape.

## Steps

1. Write the old shape, new shape and every caller.
   Done when each caller names the shape it reads and writes.
2. Expand first. Add the new shape beside the old shape.
   Done when old and new callers both run against the expanded shape.
3. Write both shapes while callers overlap.
   Done when each new write fills both shapes.
4. Migrate existing data in batches.
   Done when every existing row holds the new shape.
5. Switch callers to read the new shape.
   Done when every active caller reads the new shape.
6. Contract last. Retire the old shape in a later deploy.
   Done when search and usage data show no caller remains.
7. Test the expand-contract sequence on a copy of real data.
   Done when the test preserves every row and its required values.
8. Measure the migration at real size.
   Done when you record the duration and resource use.
9. Write the recovery path before destructive migration work.
   Done when a tested reverse path or a backup is ready.

## Rules

- Keep old and new shapes readable while callers overlap.
- Run large expand-contract backfills in batches off the hot path.
- Put destructive contract work in a separate later deploy.
- Back up before destructive shape work.
- Never drop or rename a live shape.

## Do not tell yourself

- "One rename is safe." -> Expand first, migrate in batches, then contract.
- "One update will finish fast." -> Run batches to protect table access.
- "Old callers are gone." -> Prove no caller remains with search and usage.

## Done

The expand-contract sequence preserves every row. The duration is known. No
caller uses the old shape. A recovery path is ready.
