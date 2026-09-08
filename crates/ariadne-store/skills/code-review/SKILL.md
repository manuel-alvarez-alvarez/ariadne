---
name: code-review
description: Review repository conventions and task acceptance as separate axes. Use when a branch, pull request, or patch needs a failure-based verdict.
---

# Code review

Judge two axes apart. One axis covers repository conventions. The other axis covers the task acceptance criteria.

## Steps

1. Pin the review scope. Name the base revision and the changed files.
   Done when the base resolves and the diff is not empty.
2. Read the task and the repository instructions. List the sources for each
   axis.
   Done when each axis has an authoritative source or a recorded absence.
3. Read the whole diff and the surrounding code. Map each changed hunk to its
   purpose.
   Done when every hunk has a stated purpose.
4. Run the repository checks. Record each build, test and lint result.
   Done when every required check has a result.
5. Judge the repository axis. Check the change against documented style,
   naming, structure and established code patterns.
   Done when every breach cites its rule and location.
6. Judge the acceptance axis. Check every criterion, its tests and all added
   behavior.
   Done when every criterion has a status and all scope creep is listed.
7. Report both axes under separate headings. Give each axis its own verdict.
   Done when the report has separate headings and verdicts for both axes.

## Repository conventions axis

- Reuse: Find code that repeats an established repository solution.
- Simplification: Find a branch, layer or state that carries no weight.
- Conventions: Cite the repository rule that each change breaks.

## Acceptance criteria axis

- Correctness: Find wrong results, unhandled errors, races and boundary cases.
- Missing: Find each criterion with no complete implementation.
- Wrong: Find behavior that appears complete but fails the stated criterion.
- Tests: Find each missing proof and each test that cannot fail.
- Scope creep: Find behavior the task did not ask for.

## Rules

- Give every finding the input and the failure it causes.
- Treat a claim without a failure as a question, not a defect.
- Mark each finding as must-fix or optional.
- Judge the change against the task, not against your preferred design.
- Keep each axis verdict independent.
- Approve the change only when both axes pass.

## Do not tell yourself

- "The checks pass, so the change is correct." -> Checks prove only their
  covered behavior.
- "The code is clean, so it meets the task." -> Clean code can implement the
  wrong behavior.
- "The task passes, so conventions do not matter." -> A correct feature can
  still damage the repository.

## Done

Each axis has its own verdict. Every finding names its file, failure and
weight. Every criterion has a status. Every scope change is visible.
