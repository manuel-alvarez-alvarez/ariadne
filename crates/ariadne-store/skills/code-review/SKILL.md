---
name: code-review
description: Use when a diff, specification, hot path, regression or scaling risk needs review.
---

# Code review

Complete the code-review work. Done when the conditions in `## Done` hold.

Judge repository convention and task acceptance apart. Write the verdict
first. No preamble, no narration, no closing summary.

## When not to use

Use `architecture-review` when its triggers apply.

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

   Done when each hunk has a purpose.
5. Judge the repository axis against documented style, naming, structure
   and patterns. Judge the acceptance axis against every criterion and
   added behavior, and the diff against the Out of scope section. Judge
   each test by reading its setup, action and assertions. Never change
   code to see whether a test fails.

   Done when both axes have been judged.
6. Give each finding its trigger, the input or state that breaks it, and
   its failure. A finding with no trigger you can name is optional, not
   must-fix, and marked unverified. Cap optional findings at five, the
   ones you have most conviction in.

   Done when every finding has a weight.
7. Write the verdict under each axis's own heading, with one pass or fail
   line per Criteria number. On a must-fix finding, call `fail_step` with
   the return report. Else call `complete_step` with the verdict and the
   optional findings.

   Done when the verdict has been submitted.

## Rules

### Repository conventions axis

- Find a repository solution the change repeats.
- Find a branch, layer or state that carries no weight.
- Cite the written rule and location for each breach.

### Acceptance criteria axis

- Find wrong results, unhandled errors, races and boundary cases.
- Find a criterion with no implementation or proof.
- Find behavior outside the task.

### Rules

- A must-fix finding breaks a criterion or a written repository rule.
- An optional finding never fails the step.
- Judge the task, not your preferred design. Edit no file.
- Evidence proves a check ran. Inspect code and enforce both axes
  yourself.
- Never rerun a check only to confirm the column report.

### Specification lens

A spec is ready when two people cannot read it two ways. Write the verdict
first. No preamble, no narration, no closing summary.

#### Steps

1. Pin the scope to the diff. Read the goal and the changed rules. Reuse
   the column report's evidence where it covers this revision.

   Done when every changed rule is inside the goal or marked as added
   scope.
2. Name the test that proves each changed rule. Read code only to judge a
   risk you can name.

   Done when every changed rule has an observable pass condition.
3. Find each missing decision: an error path, an empty case, a boundary, a
   limit or a permission.

   Done when each gap has one direct question.
4. Give each finding its trigger: an ambiguous reading or a conflicting
   rule. Name the requirement or written rule it breaks. A finding with
   no nameable trigger or no broken requirement is optional. Mark it
   unverified.

   Done when every blocking finding names its trigger and the rule it
   breaks.
5. Write the verdict. Cap optional findings at five, the ones you have
   most conviction in.

   Done when the verdict has been written.
6. Call `fail_step` with the verdict on a blocking finding, else
   `complete_step`.

   Done when the report has been submitted.

#### What to look for

- Ambiguity: a rule two readers read two ways, to different behavior.
- Tests: a criterion with no observable proof.
- Reasons: a number with no stated basis.
- Duplication: behavior stated twice, where one copy can go stale.
- Time: a future promise in place of current behavior.
- Scope: a requirement the goal did not request.

#### Rules

- Judge the specification without inventing its implementation.
- Keep each requirement in one authoritative place.

#### Do not tell yourself

- "The implementer will know what this means." -> Different readings
  create different products.
- "We can choose the edge cases later." -> An unanswered boundary
  question moves risk into implementation.
- "The spec asked for a review, so I name an ambiguity." -> A clear rule
  gets no invented reading.

#### Done

Every changed rule is testable, stated once and inside the goal. Optional
findings stay at five or fewer.

### Performance lens

Measure first. A guess about speed is usually wrong. Write the verdict
first, beside its measurement. No preamble, no narration, no closing
summary.

#### Steps

1. Pin the scope to the diff. Find the hot path it changes, and a
   realistic workload.

   Done when the workload names a real input size and operation.
2. Reuse the column report's measurement only if it covers this
   revision and workload. Else measure: method, environment, result.

   Done when another reviewer can repeat it.
3. Read the hot path for every cost class below. Read code outside the
   diff only to judge a risk you can name.

   Done when every class has a measurement or a recorded absence.
4. Measure each proposed change the same way.

   Done when each proposal has comparable before and after results.
5. Give each finding its trigger, the workload that causes its measured
   cost, and the regression or requirement it breaks. A finding with no
   trigger or no broken rule is optional. Mark it unverified.

   Done when every blocking finding names its trigger, cost and rule.
6. Write the verdict. Cap optional findings at five, the ones you have
   most conviction in.

   Done when the verdict has been written.
7. Call `fail_step` with the verdict on a blocking finding, else
   `complete_step`.

   Done when the report has been submitted.

#### What to look for

- Complexity: nested work over the same growing input.
- Round trips: a query or a request per item where one call fits.
- Allocation: a copy per item or a buffer grown one element at a time.
- Repeated work: a value computed again inside a loop.
- Blocking: slow work on a thread that other work needs.
- Volume: rows or bytes read beyond what the answer needs.

#### Rules

- Keep readable code when the gain is too small to measure.

#### Do not tell yourself

- "This loop looks slow." -> Appearance is not a measurement.
- "One item is enough." -> Growth costs appear at realistic sizes.
- "The benchmark improved." -> Different workloads do not support one
  comparison.

#### Done

The report names the hot path and workload. Each blocking finding has a
measurement and the rule it breaks. Optional findings stay at five or
fewer.

## Never

Edit no file.

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
