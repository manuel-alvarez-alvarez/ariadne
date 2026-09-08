---
name: testing
description: Write tests that prove each criterion at a seam, named for the claim they hold. Use when a task calls for tests or a change needs a guard.
---

# Testing

A test states one claim at a seam and fails when that claim breaks.

## Steps

1. List the acceptance criteria. Write one test per criterion.
   Done when every criterion names its test.
2. Pick the seam: the public boundary where the behavior shows. Tests live
   at seams, not against internals.
   Done when each test reaches its claim through a public interface.
3. Name each test after the claim it makes, not after the function it
   calls.
4. Arrange, act, assert. Keep the three steps visible.
5. Take every expected value from an independent source: a known-good
   literal, a worked example, the spec.
6. Run each new test against the unfixed code, and watch it fail.
   Done when each test has failed once, for the right reason.

## Rules

- Assert on behavior at the seam. A refactor behind the seam leaves the
  test green.
- Give each test one reason to fail.
- Prefer real values to mocks wherever the real thing is fast.
- Make tests independent of order and of each other.
- Spend tests on your own claims, not on what the language or the library
  already guarantees.

## Anti-patterns

Two tests that lie. Hunt both in your own suite:

- The tautological test: the assertion recomputes the code's own answer, so
  it passes by construction and can never disagree with the code.
- The coupled test: it breaks on a refactor while behavior held. That tell
  says the test reads internals; move it to the seam.

## Do not tell yourself

- "The test passes, so it proves the claim." -> A test that never failed
  proves nothing. Watch it fail first.
- "Mocking the internals makes this easier." -> A mock behind the seam
  couples the test to code that is free to change.
- "The snapshot covers it." -> A snapshot derived the way the code derives
  it is tautological.

## Done

Every acceptance criterion names a test at a seam. Each test failed without
its change and passes with it. The suite is green and repeatable.
