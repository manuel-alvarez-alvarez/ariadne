---
name: performance-review
description: Review time, memory and round-trip costs with measured evidence. Use when a hot path, regression or scaling risk needs performance review.
---

# Performance review

Measure first. A guess about speed is usually wrong. Write the verdict
first, beside its measurement. No preamble, no narration, no closing
summary.

## Steps

1. Pin the scope to the diff. Find the hot path it changes, and the
   realistic input size and workload.
   Done when the workload includes the reported input size and
   operation.
2. Measure the current cost. Record the method, environment and result.
   Done when another reviewer can repeat the measurement.
3. Read the hot path for every cost class below. Read code outside the
   diff only to judge a risk you can name.
   Done when every class has a measurement or a recorded absence.
4. Measure each proposed change with the same method and workload.
   Done when each proposal has comparable before and after results.
5. Write the verdict. Give every blocking finding its workload and its
   measured cost. Cap open, unmeasured questions at five, the ones you
   have most conviction in.
6. Call `fail_step` with the verdict on a blocking finding, else
   `complete_step`.

## What to look for

- Complexity: nested work over the same growing input.
- Round trips: a query or a request per item where one call fits.
- Allocation: a copy per item or a buffer grown one element at a time.
- Repeated work: a value computed again inside a loop.
- Blocking: slow work on a thread that other work needs.
- Volume: rows or bytes read beyond what the answer needs.

## Rules

- A blocking finding names its workload and its measured cost.
- Mark a finding with no measurement optional, not blocking.
- Compare results from the same method and environment.
- Weigh implementation cost against measured time or memory savings.
- Keep readable code when the gain is too small to measure.

## Do not tell yourself

- "This loop looks slow." -> Appearance is not a measurement.
- "One item is enough." -> Growth costs appear at realistic sizes.
- "The benchmark improved." -> Different workloads do not support one
  comparison.

## Done

The report names the hot path and workload. Each blocking finding has a
repeatable measurement. Each proposal has comparable before and after
results.
