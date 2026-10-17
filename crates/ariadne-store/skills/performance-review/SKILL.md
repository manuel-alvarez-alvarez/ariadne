---
name: performance-review
description: Use when a hot path, regression or scaling risk needs review. Reviews time, memory and round-trip costs with measured evidence.
---

# Performance review

Measure first. A guess about speed is usually wrong. Write the verdict
first, beside its measurement. No preamble, no narration, no closing
summary.

## Steps

1. Pin the scope to the diff. Find the hot path it changes, and a
   realistic workload.
   Done when the workload names a real input size and operation.
2. Reuse the column report's measurement if it covers this workload.
   Else measure: method, environment, result.
   Done when another reviewer can repeat it.
3. Read the hot path for every cost class below. Read code outside the
   diff only to judge a risk you can name.
   Done when every class has a measurement or a recorded absence.
4. Measure each proposed change the same way.
   Done when each proposal has comparable before and after results.
5. Give each finding its trigger, the workload that causes its measured
   cost, and the regression or requirement it breaks. A finding with no
   trigger or no broken rule is optional. Mark it unverified.
   Done when every blocking finding names its trigger, cost and rule.
6. Write the verdict. Cap optional findings at five, the ones you have
   most conviction in.
7. Call `fail_step` with the verdict on a blocking finding, else
   `complete_step`.

## What to look for

- Complexity: nested work over the same growing input.
- Round trips: a query or a request per item where one call fits.
- Allocation: a copy per item or a buffer grown one element at a time.
- Repeated work: a value computed again inside a loop.
- Blocking: slow work on a thread that other work needs.
- Volume: rows or bytes read beyond what the answer needs.

## Rules

- Keep readable code when the gain is too small to measure.

## Do not tell yourself

- "This loop looks slow." -> Appearance is not a measurement.
- "One item is enough." -> Growth costs appear at realistic sizes.
- "The benchmark improved." -> Different workloads do not support one
  comparison.

## Done

The report names the hot path and workload. Each blocking finding has a
measurement and the rule it breaks. Optional findings stay at five or
fewer.
