---
name: performance-review
description: Review time, memory and round-trip costs with measured evidence. Use when a hot path, regression, or scaling risk needs performance review.
---

# Performance review

Measure first. A guess about speed is usually wrong. Keep every conclusion
beside its measurement.

## Steps

1. Find the hot path. Name the realistic input size and workload.
   Done when the workload includes the reported input size and operation.
2. Measure the current cost. Record the method, environment and results.
   Done when another reviewer can repeat the measurement.
3. Read the hot path for every cost class below. Mark each candidate location.
   Done when every class has evidence or a recorded absence.
4. Measure each proposed change with the same method and workload.
   Done when each proposal has comparable before and after results.
5. Report the numbers and trade-offs. Separate measured findings from open
   questions.
   Done when every performance claim cites its measurement.

## What to look for

- Complexity: Find nested work over the same growing input.
- Round trips: Find a query or request per item where one call fits.
- Allocation: Find a copy per item or a buffer grown one element at a time.
- Repeated work: Find a value computed again inside a loop.
- Blocking: Find slow work on a thread that other work needs.
- Volume: Find rows or bytes read beyond what the answer needs.

## Rules

- Give every claim a measurement.
- Measure the cost at a realistic input size.
- Compare results from the same method and environment.
- Weigh implementation cost against measured time or memory savings.
- Keep readable code when the gain is too small to measure.

## Do not tell yourself

- "This loop looks slow." -> Appearance is not a measurement.
- "One item is enough." -> Growth costs appear at realistic sizes.
- "The benchmark improved." -> Different workloads do not support one
  comparison.

## Done

The report names the hot path and workload. Each claim has a repeatable
measurement. Each proposal has comparable before and after results.
