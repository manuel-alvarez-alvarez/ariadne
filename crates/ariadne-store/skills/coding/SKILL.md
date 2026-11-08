---
name: coding
description: Use when implementation, fixes, or behavior-preserving code changes are needed.
---

# Coding

Build the task and nothing else, with proof at the public seam. Done when `## Done` holds.

## When not to use

Use `debugging` when the cause of a failure is unknown. Use `migration` when
a stored shape changes.

## Steps

1. Read the ticket's Objective, Criteria, Interfaces and Out of scope
   sections by name. Read the named spec and the surrounding code. Match its
   style, naming and structure.

   Done when you can name each definition you change and its callers.
2. Read the repository rules that govern each file you change. Read the
   nearest `AGENTS.md`, `CLAUDE.md` or `CONTRIBUTING.md`, and the spec of its
   area. Read the test rules before you write a test.

   Done when each file you change has its rules named.
3. Name an assumption for each gap. Send a contradiction to the orchestrator
   with both readings and your pick.

   Done when every gap has an assumption.
4. For a code behavior change, write a failing test first at the public seam.
   Name its claim, not its function. Take the expected value from the spec,
   a worked example or another independent source.

   Done when the test reaches its claim through a public interface.
5. Run that test alone, never the suite around it. Watch it fail for the
   predicted reason. Build the code that turns it green. Repeat for every
   behavior change.

   Done when every behavior change has meaningful proof.
6. For a refactor, follow the refactoring path below in place of steps 4
   and 5. For prose, inspect the diff and run the relevant existing checks.

   Done when the verification detects accidental change.
7. In the same commit, change each document that a repository rule binds to
   your code. Such documents are the spec, the manual and the index.

   Done when no document those rules name disagrees with the code.
8. Commit the work in the form below. Run the checks of what you changed at
   the committed revision.

   Done when the required checks are green.
9. Call `complete_step` with the column report. End it with one line per
   Criteria number and the proof for that criterion.

   Done when the report has been submitted.

### Refactoring path

Behavior stays identical, and a green test is the proof.

1. Read the callers, the tests and the history of the code before you remove
   or reshape it.

   Done when you can state why it exists, or show the reason is gone.
2. Run the tests that cover it and record the green. Where the cover is
   thin, write characterization tests that record what the code does today.

   Done when the behavior you will move is pinned.
3. Make one kind of change at a time: rename, then extract, then move. Run
   the tests after each move.

   Done when each move holds one change and ends green.
4. Report a defect you find. Fix none inside the refactor.

   Done when the diff is structure only. One commit holds every move.

## Rules

### The commit

Write `type(scope): subject`: a lowercase type and imperative subject under
72 characters. Take types and scopes from the repository. Read its recent
history.

Keep the body to three short sentences. Say what changed and why it matters.
On a return from review, add a new commit on top. Never amend. The merge
column squashes.

### Tests

- Write the simplest code that meets the criteria.
- Give each behavior test one reason to fail and no test-order dependence.
- Prefer real values to mocks where the real thing is fast.
- The tautological test recomputes the answer in its assertion.
- The coupled test reads internals and breaks while behavior holds.

## Never

Never edit a file for behavior the Out of scope section names or the task
does not ask for.

## Do not tell yourself

- "This cleanup is small enough to include." -> A mixed diff hides both
  changes from review.
- "The test passes, so it proves the claim." -> A behavior test you never
  watched fail proves only that it runs.
- "Mocking internals makes this easier." -> A mock behind the seam couples
  the test to code free to change.
- "The reviewer will catch a breach of a repository rule." -> A breach of a
  written rule returns the task. Read the rule first.
- "This code is clearly useless." -> The fence stands for a reason you have
  not found. Find it first.

## Done

Every behavior change has a seam test that failed before implementation. A
refactor leaves its tests green and its diff structure only. The documents
the repository rules bind to the code changed with it. Required checks are
green at the committed revision. The task is committed under a conventional
subject.
