---
name: architecture-review
description: Use when a design, component or integration needs review.
---

# Architecture review

Judge the change against the system that exists. Done when the conditions in `## Done` hold.

## When not to use

Use `code-review` when the question is correctness, tests or conventions.

## Steps

1. Pin the scope to the diff. Take the reviewed revision from
   `git rev-parse HEAD`. Map the system the diff touches: modules, owners
   and dependency directions. Reuse the column report's evidence only
   where it covers the reviewed revision.

   Done when every changed component has a place on the map.
2. Mark each boundary the change crosses on that map. Read code
   outside the diff only to judge a risk you can name.

   Done when every new dependency has a source and a target.
3. Trace ownership of behavior and state. Find each idea held in two
   places.

   Done when every changed responsibility has one named owner.
4. Give each finding its trigger: the input or state that causes it. Name
   the future change it blocks, its cost and the rule it
   breaks. A finding with no trigger you can name is optional. Mark it
   unverified. Cap optional findings at five, the ones you have most
   conviction in.

   Done when every must-fix finding names its trigger, rule and cost.
5. Add the findings to the one verdict of the review skill loaded beside
   this one. Where this is the only review skill loaded, write the
   verdict. On a must-fix finding, call `fail_step` with it. Else call
   `complete_step`.

   Done when the findings are in one verdict.

## Rules

- Write the verdict first. Add no preamble and no closing summary.
- A must-fix finding breaks a criterion, a written repository rule or a
  boundary the repository already holds.
- Prefer the boundary arrangement the repository already uses.

### What to look for

- Boundaries: a crossing that no existing path uses.
- Direction: a dependency that points against the established flow.
- Ownership: logic in a layer that does not own its decision.
- State: one value stored in two places that can disagree.
- Abstraction: one caller behind a general layer.
- Coupling: unrelated callers forced through one abstraction.
- Reversal: a decision that costs much to change later.

## Never

- Never edit a file.
- Run no command that moves HEAD or a ref: no switch, reset, rebase
  or commit.

## Do not tell yourself

- "I would build it differently." -> Judge the change against the
  system that exists.
- "We might need this abstraction." -> Current callers define its
  boundary.
- "The review asked for findings, so I name one." -> A sound boundary
  gets no invented finding.

## Done

The verdict maps every changed component and dependency. Optional
findings stay at five or fewer.
