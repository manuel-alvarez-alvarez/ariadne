---
name: code-review
description: Review repository conventions and task acceptance as separate axes. Use when a branch, pull request, or patch needs a failure-based verdict.
---

# Code review

Judge repository conventions and task acceptance apart.

## Steps

1. For a branch review, run `git checkout --detach <branch>` in the task
   worktree. Search the handoff reasons, accessible logs and CI evidence.
   Read each result's revision, command and result.
   Done when you can judge what every result covers.
2. Reuse a result only when its revision matches `git rev-parse HEAD`.
   Its command must be a check this diff requires. Run each required check
   with no reusable result, once for this verdict.
   Done when every required check has a reused or fresh result.
3. Pin the scope. For a second review, read the prior SHA from your
   conversation. Run `git merge-base --is-ancestor <sha> HEAD`.
   If HEAD is not after that SHA, use `get_diff`. Otherwise, run
   `git log <sha>..HEAD` and `git diff <sha>..HEAD`. With no SHA, use
   `get_diff`.
   Done when the revisions resolve and the diff is not empty.
4. Read the task and repository instructions. List the source for each axis.
   Read the scoped diff and surrounding code. Map every hunk to its purpose.
5. Judge the repository axis against documented style, naming, structure and
   patterns. Judge the acceptance axis against every criterion, its tests and
   added behavior. A reused result proves only its covered behavior.
6. Write both axes under their own headings. Give each finding its file,
   function, input, failure and weight. Include the SHA from
   `git rev-parse HEAD`.
   On any must-fix finding, call `fail_step` with the verdict. Else call
   `complete_step` with the verdict, and list the optional findings in it.
   An optional finding never fails the step.

## Repository conventions axis

- Find a repository solution the change repeats.
- Find a branch, layer or state that carries no weight.
- Cite the written rule and location for each breach.

## Acceptance criteria axis

- Find wrong results, unhandled errors, races and boundary cases.
- Find each criterion with no implementation or proof.
- Judge each test by reading its setup, action and assertions.
  Never change code to see whether a test fails.
- Find behavior outside the task.

## Rules

- Treat a claim without a failure as a question, not a defect.
- Mark each finding as must-fix or optional.
- A must-fix finding breaks a criterion or a written repository rule.
- Judge the task, not your preferred design. Edit no file.
- Evidence proves a check ran. Inspect code and enforce both axes yourself.

## Do not tell yourself

- "The checks pass, so the change is correct." -> Checks prove only covered
  behavior.
- "The reused result is close enough." -> A missing revision or command
  proves nothing here. Run the check.
- "The task passes, so conventions do not matter." -> A correct feature can
  still damage the repository.
- "I will save a note on this review." -> Save to memory only a breach that
  will come back. The verdict is the record.

## Done

Each axis has its own verdict. Every finding names its file, failure and
weight. Every criterion has a status.
