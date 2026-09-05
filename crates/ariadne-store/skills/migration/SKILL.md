---
name: migration
description: Change a schema or move data forward, in one direction, without losing anything.
---

# Migration

Data outlives code. A migration that loses data is not undone by a revert.

## Steps

1. Write down the shape before and the shape after.
2. Write the migration forward. Make it safe to run again.
3. Say what happens to rows that do not fit the new shape.
4. Test it on a copy of real data, never on an empty database.
5. Measure how long it takes at real size.
6. Write down the way back: a reverse migration, or a backup taken first.

## Rules

- Never drop a column and add its replacement in one step. Add, backfill,
  switch the reads, then drop.
- Keep the old shape and the new shape readable while both versions run.
- Batch a large backfill. One long transaction locks the table.
- Never write a migration that depends on the application code of one version.
- Back up before anything destructive.

## Done

The migration ran on a copy of real data, its duration is known, no row was
lost, and the way back is written down.
