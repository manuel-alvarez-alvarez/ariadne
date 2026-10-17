---
name: coding
description: Build a task whole and prove changed behavior at a seam. Use when a task asks for code, a feature, a fix, or tests.
---

# Coding

Build the task you were given, and nothing else. Prove behavior where it
changes, and use verification that fits work that preserves behavior.

## Steps

1. Read the task, acceptance criteria, named spec, repository instructions
   and surrounding code. Match its style, naming and structure.
   Done when you can name each definition you change and its callers.
2. Name an assumption for each gap. Send a contradiction to the orchestrator
   with both readings and your pick.
   Done when every gap has an assumption.
3. For a code behavior change, write a failing test first at the public seam.
   Name its claim, not its function. Take the expected value from the spec,
   a worked example or another independent source.
   Done when the test reaches its claim through a public interface.
4. Run that test alone, never the suite around it. Watch it fail for the
   predicted reason. Build the code that turns it green. Repeat for every
   behavior change.
   Done when every behavior change has meaningful proof.
5. For prose or a behavior-preserving refactor, choose verification that
   fits the risk. Inspect the diff and run relevant existing checks.
   Done when the verification detects accidental change.
6. Commit the work in the form below. Run the checks of what you changed at
   the committed revision.
   Done when the required checks are green.
7. Call `complete_step` with the column report, covering every criterion.

## The commit

Write `type(scope): subject`: a lowercase type and imperative subject under
72 characters. Take types and scopes from the repository. Read its recent
history and follow `AGENTS.md`, `CLAUDE.md` or `CONTRIBUTING.md`.

Keep the body to three short sentences. Say what changed and why it matters.
On a return from review, add a new commit on top. Never amend. The merge
column squashes.

## Rules

- Write the simplest code that meets the criteria.
- Give each behavior test one reason to fail and no test-order dependence.
- Assert at the seam. A preserving refactor leaves the test green.
- Prefer real values to mocks where the real thing is fast.
- Test your claims, not a library's guarantees.

## Two tests that lie

- The tautological test recomputes the answer in its assertion.
- The coupled test reads internals and breaks while behavior holds.

## Do not tell yourself

- "This cleanup is small enough to include." -> A mixed diff hides both
  changes from review.
- "The test passes, so it proves the claim." -> A behavior test you never
  watched fail proves only that it runs.
- "This edit needs no proof." -> Prose and preserving refactors need
  verification, even when they need no new test.
- "Mocking internals makes this easier." -> A mock behind the seam couples
  the test to code free to change.
- "I will save a note on this task." -> Save to memory only a trap that
  will come back. The commit is the record of the task.

## Done

Every behavior change has a seam test that failed before implementation.
Every other change has verification that fits its risk. Required checks are
green at the committed revision. The task is committed under a conventional
subject.
