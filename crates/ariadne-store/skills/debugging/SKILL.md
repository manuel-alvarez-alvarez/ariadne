---
name: debugging
description: Find the cause of a defect, fix the cause, and add the regression test that fails without the fix.
---

# Debugging

Prove the cause before you change anything.

## Steps

1. Reproduce the defect. Write down the exact steps and the exact output.
2. Write a test that fails because of the defect. That test is your proof that
   you understand it.
3. Narrow the cause. Bisect the input, the code path or the history.
4. Name the cause in one sentence before you fix it.
5. Fix the cause, not the symptom.
6. Run the new test, then the whole suite.

## Rules

- Never fix what you cannot reproduce.
- A fix with no failing test behind it is a guess.
- Keep the change narrow. A fix that also refactors hides its own risk.
- Where the cause sits in another component, report it and stop.

## Done

The regression test fails without the fix and passes with it. The suite is
green. The report names the cause.
