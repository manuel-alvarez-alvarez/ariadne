---
name: spec-review
description: Use when a proposal or requirement set needs a pass or fail verdict. Reviews a specification for ambiguity, testability and scope.
---

# Spec review

A spec is ready when two people cannot read it two ways. Write the verdict
first. No preamble, no narration, no closing summary.

## Steps

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
6. Call `fail_step` with the verdict on a blocking finding, else
   `complete_step`.

## What to look for

- Ambiguity: a rule two readers read two ways, to different behavior.
- Tests: a criterion with no observable proof.
- Reasons: a number with no stated basis.
- Duplication: behavior stated twice, where one copy can go stale.
- Time: a future promise in place of current behavior.
- Scope: a requirement the goal did not request.

## Rules

- Judge the specification without inventing its implementation.
- Keep each requirement in one authoritative place.

## Do not tell yourself

- "The implementer will know what this means." -> Different readings
  create different products.
- "We can choose the edge cases later." -> An unanswered boundary
  question moves risk into implementation.
- "The spec asked for a review, so I name an ambiguity." -> A clear rule
  gets no invented reading.

## Done

Every changed rule is testable, stated once and inside the goal. Optional
findings stay at five or fewer.
