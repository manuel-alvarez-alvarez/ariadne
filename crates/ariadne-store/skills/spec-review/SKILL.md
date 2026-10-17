---
name: spec-review
description: Review a specification for ambiguity, testability and scope. Use when a proposal or requirement set needs a pass or fail verdict.
---

# Spec review

A spec is ready when two people cannot read it two ways. Write the verdict
first. No preamble, no narration, no closing summary.

## Steps

1. Read the goal and the specification. Map each rule to the goal.
   Done when every rule is inside the goal or marked as added scope.
2. Name the test that proves each rule. Read code only to judge a risk you
   can name.
   Done when every rule has an observable pass condition.
3. Find each missing decision: an error path, an empty case, a boundary, a
   limit or a permission.
   Done when each gap has one direct question.
4. Find a rule two readers can read two ways, each reading to a different
   behavior. Write both readings and the behavior each one causes.
   Done when each ambiguity names two readings and their two behaviors.
5. Write the verdict. Give each finding a weight and a location. Cap
   later-improvement findings at five, the ones you have most conviction
   in.
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

- A blocking finding names two readings and the different behavior each
  causes.
- Mark a finding with one reading alone optional, not blocking.
- Judge the specification without inventing its implementation.
- Request changes only where a rule cannot be tested as written.
- Keep each requirement in one authoritative place.

## Do not tell yourself

- "The implementer will know what this means." -> Different readings
  create different products.
- "We can choose the edge cases later." -> An unanswered boundary
  question moves risk into implementation.
- "The spec asked for a review, so I name an ambiguity." -> A clear rule
  gets no invented reading.

## Done

Every rule is testable, stated once and inside the goal. Every finding
names its two readings or its gap, a weight and a location.
