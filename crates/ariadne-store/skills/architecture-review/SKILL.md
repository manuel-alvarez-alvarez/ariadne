---
name: architecture-review
description: Judge whether a change fits the system as it stands - its boundaries, its coupling and its direction.
---

# Architecture review

Judge the change against the system that exists, not the one you would build.

## Steps

1. Read how the system is arranged today. Find the boundaries it already keeps.
2. Place the change on that map.
3. Work out what now depends on what.
4. Look for the same idea implemented twice.
5. Report what the change makes harder later.

## What to look for

- A boundary the change crosses that nothing crossed before.
- A dependency that points the wrong way.
- Logic in a layer that has no business holding it.
- State kept in two places, which will disagree.
- An abstraction with one caller, or one with ten unrelated callers.
- A decision that will be expensive to reverse.

## Rules

- Name the cost of every objection. Structure you cannot justify is taste.
- Prefer the arrangement the repository already uses.
- Accept a small inconsistency over a large new abstraction.
- Say which objections block the change and which are for later.

## Done

The report says where the change sits, what now depends on what, and which of
its costs are worth paying.
