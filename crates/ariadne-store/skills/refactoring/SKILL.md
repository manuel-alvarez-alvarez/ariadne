---
name: refactoring
description: Change the structure of code, not what it does, and prove it. Use when a task asks for a rename, an extraction, a move, or a cleanup.
---

# Refactoring

Behavior stays identical. That is the whole contract, and green tests are
the proof.

## Steps

1. Understand a thing before you remove or reshape it: Chesterton's Fence.
   Read its callers, its tests and its history.
   Done when you can state the reason it exists, or show that the reason is
   gone.
2. Find the tests that cover the code. Run them and record the green.
   Done when you know what the suite says before you touch anything.
3. Where the cover is thin, write characterization tests first. They record
   what the code does today.
   Done when the behavior you are about to move is pinned.
4. Make one kind of change at a time. Rename, then extract, then move.
5. Run the tests after each move.
   Done when each move ends green.
6. Keep each commit reversible on its own.

## Rules

- Keep behavior identical. A defect you find stays for its own task;
  report it.
- Keep every public interface, unless the task says to change one.
- Keep a refactor apart from a feature and from a fix. One commit does one
  of them.
- Stop and revert where the tests go red for a reason you cannot explain.

## Do not tell yourself

- "This code is clearly useless." -> The fence stands for a reason you have
  not found yet. Find it first.
- "The simpler version is obviously the same." -> Obvious is not proof. The
  green run after the move is.
- "I will fix this little bug while I am here." -> A fix inside a refactor
  hides both from review.

## Done

The tests pass unchanged. The diff is structure only. Every removal names
the reason the fence could go. A reviewer can read it as a sequence of
small, safe moves.
