---
name: coding
description: Implement a task from its specification, in the repository's own conventions, with the tests that prove it.
---

# Coding

Implement the task you were given, and nothing else.

## Steps

1. Read the task, its acceptance criteria and the spec it names.
2. Read the code around the change. Match its style, its naming and its
   structure.
3. Read the repository's conventions: `AGENTS.md`, `CLAUDE.md`,
   `CONTRIBUTING.md`.
4. Write the change in small commits. Write each commit subject in the
   imperative.
5. Add the tests the acceptance criteria call for. Run the tests and the
   linters.
6. Where the task cannot be done as written, stop and report the reason.

## Rules

- Change only what the task asks for. Leave unrelated code alone.
- Refactor nothing on the way. A separate task does that.
- Commit no generated file and no secret.
- Write no authorship trailer and no tool trailer.
- Keep the tests green at every commit.

## Done

The acceptance criteria pass. The tests and the linters are green. The diff
holds nothing the task did not ask for.
