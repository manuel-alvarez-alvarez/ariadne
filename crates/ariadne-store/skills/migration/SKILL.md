---
name: migration
description: Use when a schema shape changes, data moves, or a caller retires.
---

# Migration

Move a stored shape forward and lose no row. Done when `## Done` holds.

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
4. Write the recovery path: a reverse migration, or the old shape kept
   until the contract.

   Done when the path restores the copy of step 5.
5. Run the whole sequence on a copy of real data. Count the rows and compare
   the required values before and after.

   Done when the copy keeps every row and its required values.
6. Leave the contract to a later task. Call `complete_step` with the column
   report, naming the deferred work.

   Done when the report names the deferred work.

## Rules

- Keep old and new shapes readable while callers overlap.

## Never

Never edit a released migration. Never drop or rename a shape that a caller
still reads. Never run a migration on the only copy of real data.

## Do not tell yourself

- "I will fix the released migration in place." -> A database that ran it
  keeps its record. Add a new migration.
- "One rename is safe." -> A caller you did not find still reads the old
  name. Expand first.
- "The copy ran clean, so every row survived." -> Count the rows.
- "Old callers are gone." -> Prove no caller remains with search and usage.

## Done

A new migration holds the change, and no released migration changed. Every
row of the copy of real data survived with its values. A recovery path
restored the copy. The contract is deferred and named.
