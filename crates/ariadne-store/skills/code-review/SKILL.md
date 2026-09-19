---
name: code-review
description: Review repository conventions and task acceptance as separate axes. Use when a branch, pull request, or patch needs a failure-based verdict.
---

# Code review

Judge two axes apart. One axis covers repository conventions. The other axis covers the task acceptance criteria.

## Steps

1. Install required tools. For a branch review, run
   `git checkout --detach <branch>` in the review worktree.
   Start the whole test suite, build and linters once for this verdict,
   as one command in the foreground with a timeout up to ten minutes.
   Split a run too long by crate or package. Never poll a background
   run with a no-op command.
   Send each check's output to a log file outside the worktree, such as
   `/tmp/<task>-check.log`. Print only the summary and the failures.
   For nextest: `cargo nextest run --status-level fail --final-status-level fail 2>&1 | tail -n 40`.
   For another runner: `| tail -n 40`.
   Done when every required check has ended and printed its failures.
2. Pin the review scope. For a second review, read the last verdict SHA with
   `read_messages` and `all: true`.
   Run `git merge-base --is-ancestor <sha> HEAD`.
   If HEAD is not after that SHA, use `get_diff`.
   Otherwise, run `git log <sha>..HEAD` and `git diff <sha>..HEAD`.
   Read only those new commits. With no SHA, use `get_diff` and name the base
   revision and changed files.
   Done when the revisions resolve and the diff is not empty.
3. Read the task and the repository instructions. List the sources for each
   axis.
   Done when each axis has an authoritative source or a recorded absence.
4. Read the scoped diff and the surrounding code. Map each changed hunk to
   its purpose. Call `impact` with no argument for the callers this task
   diff reaches. Call `symbol --detail context` on each changed definition,
   and read the tests it lists.
   Done when every hunk has a stated purpose, and you know the callers and
   the tests of every changed definition.
5. Record each build, test and lint result from its log. Read the log
   only for the detail of a failure. Do not run a check again before
   this verdict.
   Done when every required check has a result.
6. Judge the repository axis. Check the change against documented style,
   naming, structure and established code patterns.
   Done when every breach cites its rule and location.
7. Judge the acceptance axis. Check every criterion, its tests and all added
   behavior.
   Done when every criterion has a status and all scope creep is listed.
8. Report both axes under separate headings. Give each axis its own verdict.
   Done when the report has separate headings and verdicts for both axes.

## Repository conventions axis

- Reuse: Find code that repeats an established repository solution.
- Simplification: Find a branch, layer or state that carries no weight.
- Conventions: Cite the repository rule that each change breaks.

## Acceptance criteria axis

- Correctness: Find wrong results, unhandled errors, races and boundary cases.
- Missing: Find each criterion with no complete implementation.
- Wrong: Find behavior that appears complete but fails the stated criterion.
- Tests: Find each missing proof.
- Test quality: Judge each test by reading its setup, action and assertions.
  Never change code to see whether a test fails.
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
