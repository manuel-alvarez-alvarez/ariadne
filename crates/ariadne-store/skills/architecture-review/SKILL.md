---
name: architecture-review
description: Judge a change against system boundaries, coupling and dependency direction. Use when a design, component, or integration needs review.
---

# Architecture review

Judge the change against the system that exists. Treat each boundary as an
architectural decision.

## Steps

1. Map the current system. Name its modules, owners and dependency directions.
   Done when every changed component has a place on the map.
2. Place the change on that map. Mark each boundary it crosses.
   Done when every new dependency has a source and target.
3. Trace ownership of behavior and state. Find each idea held in two places.
   Done when every changed responsibility has one named owner.
4. Price each architectural concern. Name the concrete future change it makes
   harder.
   Done when every concern names one affected change and cost.
5. Write the verdict. Separate blocking concerns from accepted costs and later
   work.
   Done when each concern has one disposition.

## What to look for

- Boundaries: Find a crossing that no existing path uses.
- Direction: Find a dependency that points against the established flow.
- Ownership: Find logic in a layer that does not own its decision.
- State: Find one value stored in two places that can disagree.
- Abstraction: Find one caller behind a general layer.
- Coupling: Find unrelated callers forced through one abstraction.
- Reversal: Find a decision that costs much to change later.

## Rules

- Name the cost of every objection.
- Prefer the boundary arrangement the repository already uses.
- Accept a small inconsistency when a broad abstraction has no current use.
- Mark each concern as blocking, accepted or later work.

## Do not tell yourself

- "I would build it differently." -> Judge the change against the system that
  exists.
- "We might need this abstraction." -> Current callers define its boundary.
- "The dependency is small." -> Direction matters before dependency size.

## Done

The report maps every changed component and dependency. Each boundary concern
names its cost and disposition. Every changed responsibility has one owner.
