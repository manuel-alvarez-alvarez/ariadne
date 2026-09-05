---
name: performance-review
description: Review a change for its cost in time, memory and round trips, with a measurement behind every claim.
---

# Performance review

Measure first. A guess about speed is usually wrong.

## Steps

1. Find the hot path. Name the input size that matters.
2. Measure the current cost. Record the numbers and how you got them.
3. Read the code for the costs below.
4. Measure again after any change you propose.
5. Report the numbers, not adjectives.

## What to look for

- Complexity: a loop inside a loop over the same growing input.
- Round trips: a query or a request per item, where one call would do.
- Allocation: a copy per item, or a buffer grown one element at a time.
- Repeated work: a value computed again inside a loop.
- Blocking: slow work on a thread that others wait on.
- Volume: reading more rows or more bytes than the answer needs.

## Rules

- Give every claim a measurement.
- Report the cost at a realistic input size, never at one.
- Weigh the cost of the change against the time it saves.
- Leave readable code alone where the gain is too small to measure.

## Done

The report names the hot path, the measured cost before and after, and the
input size both were taken at.
