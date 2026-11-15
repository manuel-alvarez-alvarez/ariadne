---
name: migration
description: Moves a stored shape forward with no row lost and a recovery path. Use when a schema shape changes, data moves, or a caller retires.
---

# Migration

Move a stored shape forward and lose no row.

## When not to use

Use `coding` when no stored shape changes. Use `conflict-resolution` when
two branches ship one migration number.

## Steps

1. Write the old shape, the new shape and every caller.

   Done when each caller names the shape it reads and writes.
2. Read the migration rules. Add a new migration file after the
   latest one.

   Done when no released migration is edited and the new file sorts last.
3. Where old and new callers overlap, expand first. Add the new shape beside
   the old one, write both, then switch the callers. Where no caller
   overlaps, change the shape in one step.

   Done when each overlap is named, or none is shown.
4. Write the recovery path: a reverse migration. Where callers overlap,
   keep the old shape until the contract.

   Done when the path restores the copy of step 5.
5. Run the whole sequence on a copy of real data. Count the rows and compare
   the required values before and after.

   Done when the copy keeps every row and its required values.
6. Where callers overlap, leave the contract to a later task. Call
   `complete_step` with the column report. With no overlap, name that in it.

   Done when the report names the deferred work, or states no overlap.

## Rules

- Keep old and new shapes readable while callers overlap.

## Never

Never edit a released migration. Never drop or rename a shape that a caller
still reads. Never migrate the only copy of real data.

## Do not tell yourself

- "I will fix the released migration in place." -> A database that ran it
  keeps its record. Add a new migration.
- "One rename is safe." -> A caller you did not find still reads the old
  name. Search for callers first.
- "Old callers are gone." -> Prove no caller remains with search and usage.
