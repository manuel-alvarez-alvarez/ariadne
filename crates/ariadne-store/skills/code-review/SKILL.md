---
name: code-review
description: Review a change for defects first and simplification second, with every finding tied to a failure it causes.
---

# Code review

Find what breaks. Then find what is more complex than it needs to be.

## Steps

1. Read the task, its acceptance criteria and the author's summary.
2. Read the whole diff before you judge any part of it.
3. Verify the change yourself. Build it, test it and lint it.
4. Check each acceptance criterion against the code and its tests.
5. Write the verdict once.

## What to look for

- Correctness: wrong results, unhandled errors, races, boundary and empty
  cases.
- Tests: a criterion with no test, and a test that cannot fail.
- Reuse: code that repeats something the repository already has.
- Simplification: a branch, a layer or a state that carries no weight.
- Conventions: the repository's own style, naming and structure.

## Rules

- Give every finding the input and the failure it causes. A finding you cannot
  make fail is a question, not a defect.
- Mark each finding must-fix or optional.
- Judge the change against the task, not against the design you would have
  picked.
- Approve only what you would merge yourself.

## Done

The verdict names each finding, its file, its failure and its weight. It is one
verdict for the round.
