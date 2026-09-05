---
name: spec-review
description: Review a specification for completeness, testability and scope, before anybody writes code against it.
---

# Spec review

A spec is ready when two people cannot read it two ways.

## Steps

1. Read the goal, then read the spec against it.
2. Check that every rule is testable. Name the test that would prove each one.
3. Look for what is missing: error paths, empty and boundary cases, limits, and
   who is allowed to do what.
4. Check the scope. Find anything the spec adds that the goal did not ask for.
5. Write the verdict once.

## What to look for

- A rule two readers can read differently.
- A criterion with no test behind it.
- A number with no reason.
- A behavior stated twice, which will go stale in one place.
- A promise about the future in place of a description of now.

## Rules

- Judge the spec, not the implementation nobody has written.
- Give every finding the question a reader is left with.
- Request changes where a rule cannot be tested as written.

## Done

Every rule is testable, stated once, and inside the goal's scope. The verdict
names what is left open.
