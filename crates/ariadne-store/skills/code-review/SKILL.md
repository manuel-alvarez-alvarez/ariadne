---
name: code-review
description: Review repository conventions and task acceptance as separate axes. Use when a branch, pull request, or patch needs a failure-based verdict.
---

# Code review

Judge two axes apart: repository conventions, and task acceptance.

## Steps

1. For a branch review, run `git checkout --detach <branch>` in the task
   worktree. Start the checks of what the diff changed, once for this
   verdict.
   Done when every check printed its failures.
2. Pin the review scope. For a second review, read the SHA you judged last
   from your own conversation.
   Run `git merge-base --is-ancestor <sha> HEAD`.
   If HEAD is not after that SHA, use `get_diff`.
   Otherwise, run `git log <sha>..HEAD` and `git diff <sha>..HEAD`.
   Read only those new commits. With no SHA, use `get_diff`.
   Done when the revisions resolve and the diff is not empty.
3. Read the task and the repository instructions. List the sources for each
   axis.
   Done when each axis has a source or a recorded absence.
4. Read the scoped diff and the code around it. Map each changed hunk to
   its purpose.
   Done when every hunk has a stated purpose.
5. Record each build, test and lint result from its log. Do not run a
   check again before this verdict.
   Done when every check has a result.
6. Judge the repository axis. Check the change against documented style,
   naming, structure and code patterns.
   Done when every breach cites its rule and location.
7. Judge the acceptance axis. Check every criterion, its tests and added
   behavior.
   Done when every criterion has a status and scope creep is listed.
8. Write the verdict:
   - both axes, each under its own heading
   - each finding with its file, function, input and failure
   - the SHA you judged, from `git rev-parse HEAD`
   On any must-fix finding, call `fail_step` with the verdict. Else call
   `complete_step` with the verdict, and list the optional findings in it.
   An optional finding never fails the step.

## Repository conventions axis

- Reuse: Find code that repeats a repository solution.
- Simplification: Find a branch, layer or state that carries no weight.
- Conventions: Cite the repository rule that each change breaks.

## Acceptance criteria axis

- Correctness: Find wrong results, unhandled errors, races and boundary cases.
- Missing: Find each criterion with no implementation.
- Wrong: Find behavior that looks complete but fails its criterion.
- Tests: Find each missing proof.
- Test quality: Judge each test by reading its setup, action and assertions.
  Never change code to see whether a test fails.
- Scope creep: Find behavior the task did not ask for.

## Rules

- Give every finding the input and the failure it causes.
- Treat a claim without a failure as a question, not a defect.
- Mark each finding as must-fix or optional.
- A must-fix finding breaks a criterion or a written repository rule.
- Judge the change against the task, not your preferred design.
- Edit no file: the worktree is the develop column's to change.

## Do not tell yourself

- "The checks pass, so the change is correct." -> Checks prove only their
  covered behavior.
- "The code is clean, so it meets the task." -> Clean code can implement the
  wrong behavior.
- "The task passes, so conventions do not matter." -> A correct feature can
  still damage the repository.
- "I will save a note on this review." -> Save to memory only a breach that
  will come back. The verdict is the record.

## Done

Each axis has its own verdict. Every finding names its file, failure and
weight. Every criterion has a status.
