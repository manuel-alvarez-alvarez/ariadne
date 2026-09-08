---
name: spec-review
description: Review a specification for completeness, testability and scope. Use when a proposal or requirement set needs a verdict.
---

# Spec review

A spec is ready when two people cannot read it two ways. Turn each open meaning
into a question.

## Steps

1. Read the goal and the specification. Map each rule to the goal.
   Done when every rule is inside the goal or marked as added scope.
2. Test each rule in thought. Name the test that proves it.
   Done when every rule has an observable pass condition.
3. Find each missing decision. Check error paths, empty cases, boundaries,
   limits and permissions.
   Done when each gap has one direct question.
4. Find ambiguous words and conflicting rules. Write the competing readings.
   Done when each ambiguity has two concrete interpretations.
5. Write one verdict. Separate blocking questions from later improvements.
   Done when every finding has a weight and a location.

## What to look for

- Ambiguity: Find a rule that two readers can interpret differently.
- Tests: Find a criterion with no observable proof.
- Reasons: Find a number with no stated basis.
- Duplication: Find behavior stated twice, where one copy can become stale.
- Time: Find a future promise in place of current behavior.
- Scope: Find a requirement that the goal did not request.

## Rules

- Judge the specification without inventing its implementation.
- Give every finding the question a reader must answer.
- Request changes when a rule cannot be tested as written.
- Keep each requirement in one authoritative place.

## Do not tell yourself

- "The implementer will know what this means." -> Different readings create
  different products.
- "We can choose the edge cases later." -> An unanswered boundary question
  moves risk into implementation.
- "More detail is always safer." -> Added scope can hide the goal.

## Done

Every rule is testable, stated once and inside the goal. Every open question
has a location and a weight. The verdict names all remaining decisions.
