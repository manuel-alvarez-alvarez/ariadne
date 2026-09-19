---
name: coding
description: Implement a task from its specification, with the tests that prove it. Use when a task asks for new code, a feature, or a fix.
---

# Coding

Implement the task you were given, and nothing else. The task is one commit.

## Steps

1. Read the task, its acceptance criteria and the spec it names.
2. Find the code before you read a file. Call `search_code` for a name,
   `outline` for the shape of a file, `symbol` for one definition. Read a
   file by the line range `symbol` gives. Call `impact` on each definition
   you change, before you change it. Match the style, the naming and the
   structure around the change.
   Done when a tool named every file you opened.
3. Read the repository's conventions: `AGENTS.md`, `CLAUDE.md`,
   `CONTRIBUTING.md`.
4. State your assumptions before work that is not trivial. Write them where
   you report the work.
   Done when each gap in the task has a named assumption.
5. On confusion or a contradiction, stop and ask the orchestrator over
   `send_message`. Name the two readings and the one you would pick.
   Done when the answer settles the reading.
6. Build the whole task, with the tests its acceptance criteria call for.
   Done when every criterion has the code and the test that prove it.
7. Prove the work: run the tests and the lint of what you changed, as
   one command in the foreground, with a timeout up to ten minutes.
   Split a run too long by crate or package. Never poll a background
   run with a no-op command.
   Send the whole output to a log file outside the worktree, such as
   `/tmp/<task>-check.log`. Print only the summary and the failures.
   For nextest: `cargo nextest run --status-level fail --final-status-level fail 2>&1 | tail -n 40`.
   For another runner: `| tail -n 40`.
   Read the log file only for the detail of a failure.
   Done when both are green.
8. Commit the work once, with an imperative subject.
   Done when the task is one commit on your branch.

## Rules

- Change only what the task asks for. Leave unrelated code alone.
- Refactor nothing on the way. A separate task does that.
- Write the simplest code that meets the criteria. Three plain lines beat a
  premature abstraction.
- Commit no generated file and no secret.
- Write no authorship trailer and no tool trailer.
- Where the task cannot be done as written, stop and report the reason.

## Do not tell yourself

- "I will commit this part now and finish it later." -> A partial commit
  leaves work on the branch that no test proves. Commit the task whole.
- "It is obvious what they meant." -> A silent assumption is the commonest
  failure. State it, or ask.
- "This cleanup is small enough to include." -> A mixed diff hides both
  changes from review.

## Done

The acceptance criteria pass. The tests and the lint of what you changed are
green. The task is one commit on your branch. The diff holds nothing the task
did not ask for.
