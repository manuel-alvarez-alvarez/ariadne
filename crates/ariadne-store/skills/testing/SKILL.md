---
name: testing
description: Write tests that prove each acceptance criterion, at the right level, named for the claim they hold.
---

# Testing

A test states one claim and fails when that claim breaks.

## Steps

1. List the acceptance criteria. Write one test per criterion.
2. Pick the level. Test a unit where the logic is local. Test through the
   interface where the value is in the wiring.
3. Name each test after the claim it makes, not after the function it calls.
4. Arrange, act, assert. Keep the three steps visible.
5. Run each new test against the unfixed code. A test that cannot fail proves
   nothing.

## Rules

- Assert on behavior, not on internals.
- Give each test one reason to fail.
- Prefer real values to mocks wherever the real thing is fast.
- Make tests independent of order and of each other.
- Do not test what the language or the library already guarantees.

## Done

Every acceptance criterion names a test. Each test fails without its change.
The suite is green and repeatable.
