---
name: coding
description: Implement a task from its specification, with the tests that prove it. Use when a task asks for new code, a feature, or a fix.
---

# Coding

Implement the task you were given, and nothing else. Build it in thin
vertical slices.

## Steps

1. Read the task, its acceptance criteria and the spec it names.
2. Read the code around the change. Match its style, its naming and its
   structure.
3. Read the repository's conventions: `AGENTS.md`, `CLAUDE.md`,
   `CONTRIBUTING.md`.
4. State your assumptions before work that is not trivial. Write them where
   you report the work.
   Done when each gap in the task has a named assumption.
5. On confusion or a contradiction, stop and ask the orchestrator over
   `send_message`. Name the two readings and the one you would pick.
   Done when the answer settles the reading.
6. Cut the work into thin vertical slices. Each slice is the smallest
   complete piece that runs end to end.
7. Build one slice at a time, with the tests its acceptance criteria call
   for. Prove each slice before the next: run the tests, the build and the
   linters. Commit the slice with an imperative subject.
   Done when the slice is proven and committed.

## Rules

- Change only what the task asks for. Leave unrelated code alone.
- Refactor nothing on the way. A separate task does that.
- Write the simplest code that meets the criteria. Three plain lines beat a
  premature abstraction.
- Commit no generated file and no secret.
- Write no authorship trailer and no tool trailer.
- Keep the tests green at every commit.
- Where the task cannot be done as written, stop and report the reason.

## Do not tell yourself

- "I will test it all at the end." -> A defect in slice one makes every
  later slice wrong. Prove each slice.
- "It is obvious what they meant." -> A silent assumption is the commonest
  failure. State it, or ask.
- "This cleanup is small enough to include." -> A mixed diff hides both
  changes from review.

## Done

The acceptance criteria pass. The tests and the linters are green. Each
slice was proven on its own. The diff holds nothing the task did not ask
for.
