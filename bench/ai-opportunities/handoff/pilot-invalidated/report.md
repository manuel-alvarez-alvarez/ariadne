# Kev relevance scoring for session handoff selection

## Question

Can entry relevance scoring retain mandatory session facts more often than current
recency selection at the same rendered handoff character budget?

This pilot selects existing folded entries. It does not summarize, rewrite, or measure
downstream task completion.

## Evidence and method

The production reference is `crates/ariadne-daemon/src/agents/handoff.rs`.
It folds tool output to ten lines, fences each entry, removes whole older entries,
and counts the fixed introduction and omission note in its character budget.
The benchmark copies those rules locally and does not read `crates/` at runtime.

`cases.json` contains 60 sanitized synthetic histories. Forty are development cases and
twenty are reserved evaluation cases. A source family and paraphrase remain in one split.
Labels were frozen before scoring: user constraint, correction, unresolved blocker,
decision, changed file, and decisive tool result. Every history has stale instructions,
conflicting statements, distracting recent output, and quoted instructions in tool output.

The compared policies are current recency selection, fixed label rules, and Kev scoring.
All receive 260, 430, or 2,000 characters, including the introduction and omission note.
The 2,000-character budget fits every fixture. Selections retain original rendered text
and restore source order. `predictions.json` records every selection, score, budget,
selected text, and omitted mandatory fact.

The one frozen Kev question scores each original entry. This was the only prompt design
measured on reserved cases. It uses Kev package revision `f1535963cea021439370c23127bc970b6788e730`
and `jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101`.

## Reserved-case results

| Policy | Budget | Mandatory recall | Critical omissions | Useful retained | Budget compliant |
| --- | ---: | ---: | ---: | ---: | --- |
| Recency | 260 | 0.00 | 20 | 0.00 | yes |
| Recency | 430 | 0.00 | 20 | 0.00 | yes |
| Rules | 260 | 1.00 | 0 | 0.50 | yes |
| Rules | 430 | 1.00 | 0 | 0.25 | yes |
| Kev | 260 | 1.00 | 0 | 0.50 | yes |
| Kev | 430 | 1.00 | 0 | 0.25 | yes |
| All policies | 2,000 | 1.00 | 0 | 0.20 | yes |

Kev and the rules baseline selected the labelled fact in all 20 cases at that budget.
Recency omitted every labelled fact because each distractor was newer. The representative recency failure is
`handoff-40`: it retains recent formatting output and drops the earlier user constraint.

## Cost and environment

The model ran once under the shared operating-system lock on MPS with bfloat16 and
the automatic backend. Cold startup was 37,666 ms. The 100 entry scores had warm
p50 102.5 ms and p95 135.4 ms. There were 100 model calls, zero timeouts, and zero
malformed answers. Peak process memory was 8,526,020,608 bytes from `getrusage ru_maxrss`.

The experiment used `/Users/malvarez/.ariadne/ai-opportunities/hf` instead of the production
cache and loaded in process rather than through the daemon server. Those differences limit
latency and memory extrapolation to production.

## Recommendation and limits

Investigate further. Kev matches a fixed rules baseline on this synthetic pilot, but the
sample is small and labels are intentionally structured. The strongest counter-evidence
is the rules baseline, which matches Kev recall without model cost. Adopt later only after real sanitized session histories
show the same result and an integration budget validates startup and memory costs.

The fallback is recency selection when model loading fails, an entry times out, or any
score is missing, non-finite, or malformed. These are infrastructure failures, not model
quality evidence. A later integration should select after `render_entry` and before
`apply_budget`, retain event ids with entries, and mark user prompts as protected evidence.
