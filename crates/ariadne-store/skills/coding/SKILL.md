---
name: coding
description: Build a task whole and prove it with tests at a seam. Use when a task asks for code, a feature, a fix, or the tests that guard one.
---

# Coding

Build the task you were given, and nothing else. A criterion is done when a
test proves it.

## Steps

1. Read the task, its acceptance criteria and the spec it names.
2. Read the code around the change. Match its style, naming and structure.
   Done when you can name each definition you change and its callers.
3. Name an assumption for each gap in the task. Ask the orchestrator about
   a contradiction with `send_message`. Give both readings and your pick.
   Done when each gap has a named assumption.
4. Take the criteria one at a time. Write the test first, at the seam: the
   public boundary where the behavior shows. Name it after the claim it
   makes, not after the function it calls. Arrange, act, assert. Take the
   expected value from an independent source: a known-good literal, a
   worked example, the spec.
   Done when the test reaches its claim through a public interface.
5. Run that one test and watch it fail. Run the one test, never the suite
   around it.
   Done when it failed for the reason you predicted.
6. Build the code that turns it green. Go back to step 4 for the next
   criterion.
   Done when every criterion has the code and the test that prove it.
7. Run the checks of what you changed.
   Done when they are green.
8. Commit the work in the form below.
9. Call `complete_step` with your assumptions and what proves each
   criterion.

## The commit

Write the subject as a Conventional Commit: `type(scope): subject`. Keep the
type lowercase and the subject imperative, under 72 characters. Take the
types and the scopes from the repository. Read its history with
`git log --oneline -20`. Follow `AGENTS.md`, `CLAUDE.md` or
`CONTRIBUTING.md` where one of them states the format.

Keep the body to three short sentences at most. Say what changed and why a
reader cares. The diff already says how, so repeat none of it.

```
feat(store): seed a skill the catalog gained

Seeding runs by name on every open. An old database reaches a new skill
without a migration.
```

```
fix(daemon): retry a git spawn that failed under load
```

On a return from review, add a new commit on top. Never amend. The merge
column squashes.

## Rules

- Write the simplest code that meets the criteria.
- Give each test one reason to fail, and no dependence on another test.
- Assert on behavior at the seam. A refactor behind the seam leaves the
  test green.
- Prefer real values to mocks wherever the real thing is fast.
- Spend tests on your own claims, not on what the library guarantees.

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
- "I will save a note on this task." -> Save to memory only a trap that
  will come back. The commit is the record of the task.

## Done

Every criterion names a test at a seam that failed without its change and
passes with it. The tests and the lint of what you changed are green. The
task is committed on your branch, under a conventional subject.
