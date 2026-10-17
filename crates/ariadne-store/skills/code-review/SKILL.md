---
name: code-review
description: Review a branch, pull request or patch for a correctness or convention failure. Use when a diff needs a pass or fail verdict.
---

# Code review

Judge repository convention and task acceptance apart. Write the verdict
first. No preamble, no narration, no closing summary.

## Steps

1. For a branch review, run `git checkout --detach <branch>` in the task
   worktree. Read the column report as unverified claims, not proof.
   Done when you can name what each claim covers.
2. Pin the scope to the diff. With a prior SHA from the conversation, run
   `git merge-base --is-ancestor <sha> HEAD`. Where HEAD is after it, run
   `git diff <sha>..HEAD`. Otherwise, use `get_diff`.
   Done when the diff is not empty.
3. List the checks the diff requires. Reuse a result only when its
   revision matches `git rev-parse HEAD` and its command is a required
   check. Run each required check with no reusable result, once for this
   verdict.
   Done when every required check has a reused or fresh result.
4. Map every hunk of the diff to its purpose. Read code outside the diff
   only to judge a risk you can name.
5. Judge the repository axis against documented style, naming, structure
   and patterns. Judge the acceptance axis against every criterion and
   added behavior. Judge each test by reading its setup, action and
   assertions. Never change code to see whether a test fails.
6. Give each finding its trigger, the input or state that breaks it, and
   its failure. Mark a finding with no trigger optional, not must-fix. Cap
   optional findings at five, the ones you have most conviction in.
7. Write the verdict under each axis's own heading. On a must-fix finding,
   call `fail_step` with the return report. Else call `complete_step` with
   the verdict and the optional findings.

## Repository conventions axis

- Find a repository solution the change repeats.
- Find a branch, layer or state that carries no weight.
- Cite the written rule and location for each breach.

## Acceptance criteria axis

- Find wrong results, unhandled errors, races and boundary cases.
- Find a criterion with no implementation or proof.
- Find behavior outside the task.

## Rules

- A must-fix finding breaks a criterion or a written repository rule.
- Treat a claim with no trigger you can name as optional, not must-fix.
- An optional finding never fails the step.
- Judge the task, not your preferred design. Edit no file.
- Evidence proves a check ran. Inspect code and enforce both axes
  yourself.
- Never rerun a check only to confirm the column report.

## Do not tell yourself

- "The checks pass, so the change is correct." -> Checks prove only
  covered behavior.
- "The task passes, so conventions do not matter." -> A correct feature
  can still damage the repository.
- "The task asked for a review, so I name a gap." -> A sound change gets
  no invented gap.

## Done

Each axis has its own verdict. Every finding names its trigger, failure
and weight. Every criterion has a status.
