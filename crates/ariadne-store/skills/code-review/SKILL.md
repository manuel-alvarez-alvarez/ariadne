---
name: code-review
description: Use when a diff, specification, hot path, regression or scaling risk needs review.
---

# Code review

Judge the branch revision and end the column with one verdict. Done when the conditions in `## Done` hold.

## When not to use

Use `architecture-review` when the question is a boundary or a dependency.
Use `pr-reviewer` when the diff is an open forge request.

## Steps

1. Run `git rev-parse HEAD` and `git branch --show-current`. Review that
   revision in place. Name the sha as the reviewed revision. Where no
   branch prints, the work is off the branch. That is a must-fix finding.

   Done when the reviewed revision and the task branch are named.
2. Read the column report as unverified claims, not proof.

   Done when you can name what each claim covers.
3. Pin the scope to the diff. With a prior SHA from the conversation, run
   `git merge-base --is-ancestor <sha> HEAD`. Where HEAD is after it, run
   `git diff <sha>..HEAD`. Otherwise, use `get_diff`.

   Done when the diff is not empty.
4. List the checks the diff requires. Reuse evidence only when it covers
   the reviewed revision and a required check. Run each required check
   with no reusable result, once for this verdict.

   Done when every required check has a reused or fresh result.
5. Map every hunk of the diff to its purpose. Read code outside the diff
   only to judge a risk you can name.

   Done when each hunk has a purpose.
6. Judge the repository axis and the acceptance axis below. Judge the diff
   against the Out of scope section. Judge each test by reading its setup,
   action and assertions. Apply each lens below that the diff touches.

   Done when both axes and every lens that applies have been judged.
7. Give each finding its trigger: the input or state that causes it. Name
   its failure and the rule it breaks. Search once for the same pattern
   at sibling call sites. A finding with no trigger you can name is
   optional. Mark it unverified. Cap optional findings at five, the ones
   you have most conviction in.

   Done when every finding has a weight.
8. Write the verdict under each axis's own heading, with one pass or fail
   line per Criteria number. Open it with pass or fail and the reviewed
   revision. End it with each behavior you did not judge.

   Done when every Criteria number has one line.
9. On a must-fix finding, call `fail_step` with the return report. Else call
   `complete_step` with the verdict and the optional findings.

   Done when `git rev-parse HEAD` prints the reviewed revision and the
   column has ended.

## Rules

- Write the verdict first. Add no preamble and no closing summary.
- A must-fix finding breaks a criterion or a written repository rule.
- A criterion you did not judge fails. A category that does not apply
  never counts as passed.
- Judge the task, not your preferred design.
- Evidence proves a check ran. Inspect code and enforce both axes
  yourself.

### Repository conventions axis

- Find a repository solution the change repeats.
- Find a branch, layer or state that carries no weight.
- Cite the written rule and location for each breach.

### Acceptance criteria axis

- Find wrong results, unhandled errors, races and boundary cases.
- Find a criterion with no implementation or proof.
- Find behavior outside the task.

### Specification lens

A spec is ready when two people cannot read it two ways. Judge a changed
rule inside the goal, and name the test that proves it.

- Ambiguity: a rule two readers read two ways, to different behavior.
- Testability: a rule with no observable pass condition, or an error path,
  empty case, boundary or limit left undecided.
- Duplication: behavior stated twice, where one copy can go stale.
- Scope: a requirement the goal did not request.

### Performance lens

Judge a hot-path change by a measurement with its workload, never by
appearance. The workload names a real input size and operation.
Reuse the column report's measurement only where it covers the reviewed
revision and workload. Else measure: method, environment, result.
Measure each proposed change the same way. A cost you did not measure is
optional.

- Complexity: nested work over the same growing input.
- Round trips: a query or a request per item where one call fits.
- Allocation: a copy per item or a buffer grown one element at a time.
- Repeated work: a value computed again inside a loop.
- Blocking: slow work on a thread that other work needs.
- Volume: rows or bytes read beyond what the answer needs.

## Never

- Edit no file.
- Run no command that moves HEAD or a ref: no switch, reset, rebase
  or commit.
- Never change code to see whether a test fails.
- Never rerun a check only to confirm the column report.
- Never fail the step on an optional finding.

## Do not tell yourself

- "The checks pass, so the change is correct." -> Checks prove only
  covered behavior.
- "The task passes, so conventions do not matter." -> A correct feature
  can still damage the repository.
- "The report says it is fixed." -> The report is a claim. Read the code.
- "The diff is small, so it is safe." -> Classify by risk, not by size.
- "The task asked for a review, so I name a gap." -> A sound change gets
  no invented gap.
- "The implementer will know what this means." -> Different readings
  create different products.
- "This loop looks slow." -> Appearance is not a measurement.
- "The benchmark improved." -> Different workloads do not support one
  comparison.

## Done

Every Criteria number has a pass or fail line. Every finding names its
trigger, failure and weight. HEAD stays on the task branch at the
reviewed revision. The column ended on one step call.
