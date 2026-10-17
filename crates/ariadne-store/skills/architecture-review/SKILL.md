---
name: architecture-review
description: Use when a design, component or integration needs review. Judges a change against system boundaries, coupling and dependency direction.
---

# Architecture review

Judge the change against the system that exists. Write the verdict first.
No preamble, no narration, no closing summary.

## Steps

1. Pin the scope to the diff. Map the system it touches: modules, owners
   and dependency directions. Reuse the column report's evidence where it
   covers this revision.
   Done when every changed component has a place on the map.
2. Place the change on that map. Mark each boundary it crosses. Read code
   outside the diff only to judge a risk you can name.
   Done when every new dependency has a source and a target.
3. Trace ownership of behavior and state. Find each idea held in two
   places.
   Done when every changed responsibility has one named owner.
4. Give each concern its trigger, the future change it blocks, and the
   rule or pattern it breaks. Price its cost. A concern with no nameable
   trigger or no rule it breaks is optional. Mark it unverified.
   Done when every blocking concern names its trigger, rule and cost.
5. Write the verdict. Cap optional concerns at five, the ones you have
   most conviction in.
6. Call `fail_step` with the verdict on a blocking finding, else
   `complete_step`.

## What to look for

- Boundaries: a crossing that no existing path uses.
- Direction: a dependency that points against the established flow.
- Ownership: logic in a layer that does not own its decision.
- State: one value stored in two places that can disagree.
- Abstraction: one caller behind a general layer.
- Coupling: unrelated callers forced through one abstraction.
- Reversal: a decision that costs much to change later.

## Rules

- Prefer the boundary arrangement the repository already uses.
- Accept a small inconsistency when a broad abstraction has no current
  use.

## Do not tell yourself

- "I would build it differently." -> Judge the change against the
  system that exists.
- "We might need this abstraction." -> Current callers define its
  boundary.
- "The review asked for concerns, so I name one." -> A sound boundary
  gets no invented concern.

## Done

The report maps every changed component and dependency. Optional
concerns stay at five or fewer.
