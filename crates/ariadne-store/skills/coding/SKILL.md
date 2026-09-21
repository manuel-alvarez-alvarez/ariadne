---
name: coding
description: Build a task whole and prove it with tests at a seam. Use when a task asks for code, a feature, a fix, or the tests that guard one.
---

# Coding

Build the task you were given, and nothing else. A criterion is done when a
test proves it. The task is one commit.

## Steps

1. Read the task, its acceptance criteria and the spec it names.
<!-- knowledge on -->
2. Find the code before you read a file. Call `search_code` for a name,
   `outline` for the shape of a file, `symbol` for one definition. Read a
   file by the line range `symbol` gives. Call `impact` on a definition
   whose signature you change. Call `path` only where the change crosses
   several edges. Do not search with the shell for what `search_code` found.
   Done when you can name each definition you change and its callers.
<!-- knowledge off -->
2. Read the code around the change. Match its style, naming and structure.
   Done when you can name each definition you change and its callers.
<!-- knowledge end -->
3. State your assumptions where you report the work.
   Done when each gap in the task has a named assumption.
4. On a contradiction, ask the orchestrator over `send_message`. Name the
   two readings and the one you would pick.
5. Take the criteria one at a time. Write the test first, at the seam: the
   public boundary where the behavior shows. Name it after the claim it
   makes, not after the function it calls. Arrange, act, assert. Take the
   expected value from an independent source: a known-good literal, a
   worked example, the spec.
   Done when the test reaches its claim through a public interface.
6. Run that one test and watch it fail. Run the one test, never the suite
   around it.
   Done when it failed for the reason you predicted.
7. Build the code that turns it green. Go back to step 5 for the next
   criterion.
   Done when every criterion has the code and the test that prove it.
8. Prove the work: run the tests and the lint of what you changed, as
   one command in the foreground, with a timeout up to ten minutes.
   Split a run too long by crate or package. Never poll a background
   run with a no-op command.
   Send the whole output to a log file outside the worktree, such as
   `/tmp/<task>-check.log`. Print only the summary and the failures.
   For nextest: `cargo nextest run --status-level fail --final-status-level fail 2>&1 | tail -n 40`.
   For another runner: `| tail -n 40`.
   Read the log file only for the detail of a failure.
   Done when both are green.
9. Commit the work once, in the form below.
    Done when the task is one commit on your branch.

## The commit

Write the subject as a Conventional Commit: `type(scope): subject`. Keep the
type lowercase and the subject imperative, under 72 characters. Take the
types and the scopes from the repository: read its history with
`git log --oneline -20`, and follow `AGENTS.md`, `CLAUDE.md` or
`CONTRIBUTING.md` where one of them states the format.

Keep the body to three short sentences at most. Say what changed and why a
reader cares. The diff already says how, so repeat none of it. Write no
trailer under the body: no `Co-Authored-By`, no tool name, no generated-by
line. A trailer is noise in every log that reads the commit later.

```
feat(store): seed a skill the catalog gained

Seeding runs by name on every open. An old database reaches a new skill
without a migration.
```

```
fix(daemon): retry a git spawn that failed under load
```

## Rules

- Change only what the task asks for.
- Write the simplest code that meets the criteria.
- Give each test one reason to fail, and no dependence on another test.
- Assert on behavior at the seam. A refactor behind the seam leaves the
  test green.
- Prefer real values to mocks wherever the real thing is fast.
- Spend tests on your own claims, not on what the library guarantees.
- Where the task cannot be done as written, stop and report the reason.

## Two tests that lie

Hunt both in what you wrote:

- The tautological test: the assertion recomputes the code's own answer, so
  it can never disagree with the code.
- The coupled test: it breaks on a refactor while behavior held. It reads
  internals; move it to the seam.

## Do not tell yourself

- "I will commit this part now and finish it later." -> A partial commit
  leaves work no test proves. Commit the task whole.
- "It is obvious what they meant." -> A silent assumption is the commonest
  failure. State it, or ask.
- "This cleanup is small enough to include." -> A mixed diff hides both
  changes from review.
- "The test passes, so it proves the claim." -> A test you never watched
  fail proves that it runs, not that it holds.
- "Mocking the internals makes this easier." -> A mock behind the seam
  couples the test to code free to change.
- "I will note what I did." -> A note on your work is a report. Save the
  trap, not the task.

## Done

Every criterion names a test at a seam that failed without its change and
passes with it. The tests and the lint of what you changed are green. The
task is one commit on your branch, under a conventional subject.
