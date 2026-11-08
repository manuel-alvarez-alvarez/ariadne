---
name: coding
description: Use when implementation, fixes, or behavior-preserving code changes are needed.
---

# Coding

Complete the coding work. Done when the conditions in `## Done` hold.

Build the task you were given, and nothing else. Prove behavior where it
changes, and use verification that fits work that preserves behavior.

## When not to use

Use `debugging` when its triggers apply.

## Steps

1. Read the ticket's Objective, Criteria, Interfaces and Out of scope
   sections by name. Read the named spec, repository instructions and
   surrounding code. Match its style, naming and structure.

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
7. Call `complete_step` with the column report. End it with one line per
   Criteria number and the proof for that criterion.

   Done when the report has been submitted.

## Rules

### The commit

Write `type(scope): subject`: a lowercase type and imperative subject under
72 characters. Take types and scopes from the repository. Read its recent
history and follow `AGENTS.md`, `CLAUDE.md` or `CONTRIBUTING.md`.

Keep the body to three short sentences. Say what changed and why it matters.
On a return from review, add a new commit on top. Never amend. The merge
column squashes.

### Rules

- Write the simplest code that meets the criteria.
- Give each behavior test one reason to fail and no test-order dependence.
- Assert at the seam. A preserving refactor leaves the test green.
- Prefer real values to mocks where the real thing is fast.
- Test your claims, not a library's guarantees.

### Two tests that lie

- The tautological test recomputes the answer in its assertion.
- The coupled test reads internals and breaks while behavior holds.

### Refactoring

Behavior stays identical, and green tests are the proof.

#### Steps

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

   Done when each move has one change.
5. Run the tests after each move.

   Done when each move ends green.
6. Commit the whole refactor once, after every move is green.

   Done when one commit holds every move.
7. Call `complete_step` with the column report.

   Done when the report has been submitted.

#### Rules

- Keep behavior identical. A defect you find stays for its own task;
  report it.
- Keep every public interface, unless the task says to change one.
- Keep a refactor apart from a feature and from a fix. One commit does one
  of them.
- Stop and revert where the tests go red for a reason you cannot explain.

#### Do not tell yourself

- "This code is clearly useless." -> The fence stands for a reason you have
  not found yet. Find it first.
- "The simpler version is obviously the same." -> Obvious is not proof. The
  green run after the move is.
- "I will fix this little bug while I am here." -> A fix inside a refactor
  hides both from review.

#### Done

The tests pass unchanged. The diff is structure only. Every removal names
the reason the fence could go. One commit holds every move.

## Never

Never include behavior outside the task.

## Do not tell yourself

- "This cleanup is small enough to include." -> A mixed diff hides both
  changes from review.
- "The test passes, so it proves the claim." -> A behavior test you never
  watched fail proves only that it runs.
- "Mocking internals makes this easier." -> A mock behind the seam couples
  the test to code free to change.

## Done

Every behavior change has a seam test that failed before implementation.
Every other change has verification that fits its risk. Required checks are
green at the committed revision. The task is committed under a conventional
subject.
