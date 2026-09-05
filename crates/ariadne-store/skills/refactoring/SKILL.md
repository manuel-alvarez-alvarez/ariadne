---
name: refactoring
description: Change the structure of code without changing what it does, and prove the behavior held.
---

# Refactoring

Behavior stays identical. That is the whole contract.

## Steps

1. Find the tests that cover the code. Run them and record the result.
2. Where the cover is thin, write characterization tests first. They record
   what the code does today.
3. Make one kind of change at a time. Rename, then extract, then move.
4. Run the tests after each step.
5. Keep each commit reversible on its own.

## Rules

- Change no behavior. A defect you find stays for its own task; report it.
- Change no public interface unless the task says to.
- Never mix a refactor with a feature or a fix.
- Stop and revert where the tests go red for a reason you cannot explain.

## Done

The tests pass unchanged. The diff is structure only. A reviewer can read it as
a sequence of small, safe moves.
