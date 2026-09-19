---
name: testing
description: Write tests that prove each criterion at a seam, named for the claim they hold. Use when a task calls for tests or a change needs a guard.
---

# Testing

A test states one claim at a seam and fails when that claim breaks.

## Steps

1. List the acceptance criteria. Write one test per criterion.
   Done when every criterion names its test.
2. Pick the seam: the public boundary where the behavior shows.
   Done when each test reaches its claim through a public interface.
3. Name each test after the claim it makes, not after the function it
   calls.
4. Arrange, act, assert. Keep the three steps visible.
5. Take every expected value from an independent source: a known-good
   literal, a worked example, the spec.
6. Run each new test on its own against the unfixed code, and watch it fail.
   Run the one test, never the suite around it.
   Done when each test has failed once, for the right reason.
7. Call `save_memory` for the seam or the flake you had to learn. Save a
   trap, a working command or a convention no file states. Save only a fact
   that cost you time. Never save a task report, a change summary, a plan,
   or what the code, a spec or `AGENTS.md` states. A task saves 2 memories
   at most, and the daemon refuses the third.

## Rules

- Assert on behavior at the seam. A refactor behind the seam leaves the
  test green.
- Give each test one reason to fail.
- Prefer real values to mocks wherever the real thing is fast.
- Make tests independent of order and of each other.
- Spend tests on your own claims, not on what the library guarantees.
- Run a check in the foreground, with a timeout up to ten minutes. Split a
  run too long by crate or package. Never poll a background run with a
  no-op command.
- Send the whole output to a log file outside the worktree, such as
  `/tmp/<task>-check.log`. Print only the summary and the failures.
  For nextest: `cargo nextest run --status-level fail --final-status-level fail 2>&1 | tail -n 40`.
  For another runner: `| tail -n 40`.
- Read the log file only for the detail of a failure.

## Anti-patterns

Two tests that lie. Hunt both in your suite:

- The tautological test: the assertion recomputes the code's own answer, so
  it can never disagree with the code.
- The coupled test: it breaks on a refactor while behavior held. It reads
  internals; move it to the seam.

## Do not tell yourself

- "The test passes, so it proves the claim." -> A test that never failed
  proves nothing. Watch it fail first.
- "Mocking the internals makes this easier." -> A mock behind the seam
  couples the test to code free to change.
- "The snapshot covers it." -> A snapshot derived the way the code derives
  it is tautological.
- "This might help somebody later." -> Might is not did. Save the fact that
  cost you time.

## Done

Every acceptance criterion names a test at a seam. Each test failed without
its change and passes with it. Each test is green and repeatable.
