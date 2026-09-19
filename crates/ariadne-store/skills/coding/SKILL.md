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
   you change, before you change it. Call `path` between related definitions
   when the change crosses several edges.
   Done when a tool named every file you opened.
3. State your assumptions where you report the work.
   Done when each gap in the task has a named assumption.
4. On a contradiction, ask the orchestrator over `send_message`. Name the
   two readings and the one you would pick.
5. Build the whole task, with the tests its acceptance criteria call for.
   Done when every criterion has the code and the test that prove it.
6. Prove the work: run the tests and the lint of what you changed, as
   one command in the foreground, with a timeout up to ten minutes.
   Split a run too long by crate or package. Never poll a background
   run with a no-op command.
   Send the whole output to a log file outside the worktree, such as
   `/tmp/<task>-check.log`. Print only the summary and the failures.
   For nextest: `cargo nextest run --status-level fail --final-status-level fail 2>&1 | tail -n 40`.
   For another runner: `| tail -n 40`.
   Read the log file only for the detail of a failure.
   Done when both are green.
7. Call `save_memory` for the trap you hit, or the command that proved the
   change. Save a trap, a working command or a convention no file states.
   Save only a fact that cost you time. Never save a task report, a change
   summary, a plan, or what the code, a spec or `AGENTS.md` states. A task
   saves 2 memories at most, and the daemon refuses the third.
8. Commit the work once, with an imperative subject.
   Done when the task is one commit on your branch.

## Rules

- Change only what the task asks for.
- Write the simplest code that meets the criteria.
- Where the task cannot be done as written, stop and report the reason.

## Do not tell yourself

- "I will commit this part now and finish it later." -> A partial commit
  leaves work no test proves. Commit the task whole.
- "It is obvious what they meant." -> A silent assumption is the commonest
  failure. State it, or ask.
- "This cleanup is small enough to include." -> A mixed diff hides both
  changes from review.
- "I will note what I did." -> A note on your work is a report. Save the
  trap, not the task.

## Done

The acceptance criteria pass. The tests and the lint of what you changed are
green. The task is one commit on your branch. The diff holds nothing the task
did not ask for.
