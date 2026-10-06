# Kev relevance scoring for session handoff selection (repaired rerun)

The first run of this benchmark is invalid; see `pilot-invalidated/NOTE.md`. This
report and `results.json` describe one rerun, after the repair below.

## Question

Can entry relevance scoring retain mandatory session facts more often than current
recency selection, or a deployable rules baseline, at the same rendered handoff
character budget?

This pilot selects existing folded entries. It does not summarize, rewrite, or measure
downstream task completion.

## What was wrong with the first run, and with the first repair

The first run:

- Its `rules(item)` read `item["labels"]` - an answer annotation, not evidence a
  production selector would ever see. Its "rules" result was an oracle wearing a
  deployable baseline's name.
- Its 60 histories were built from six fixed fact texts, repeated with only the id
  and family name changed; the development and evaluation splits were not
  independent histories.
- Its cold-start timer started before loading the model and stopped after scoring
  every evaluation case, so its `cold_start_ms` included warm inference time.
- Its `report.md` and `results.json` were written from different runs.

The first repair fixed all four, but review caught two more:

- Every one of its ten scenario templates had 6 development and 3 evaluation
  instances. The same template, phrased differently, sat on both sides of the
  split - evaluation was still measuring a pattern development had already shown.
  Each template now sits wholly in one split (`dataset.DEV_TEMPLATES`,
  `dataset.EVAL_TEMPLATES`), checked directly off a `template` field, not inferred
  from a family string.
- Kev scored an entry's full raw `text`, while every other policy budgets the
  rendered, folded version `render()` actually produces. A long tool entry's
  folded-away lines were evidence Kev had and no deployable policy's budget ever
  saw. `evidence()` is now the one place either kind of policy reads an entry's
  body from, so both see exactly what the rendered handoff would have shown.

## Evidence and method

The production reference is `crates/ariadne-daemon/src/agents/handoff.rs`. It folds
tool output to ten lines, fences each entry, removes whole older entries, and counts
the fixed introduction and omission note in its character budget. The benchmark
copies those rules locally and does not read `crates/` at runtime.

`cases.json` holds 90 synthetic histories, each rendered text genuinely distinct: 36
development, 54 reserved evaluation, across ten scenario templates (a live constraint,
a correction, an unresolved blocker, a decision, a changed-file note, a decisive tool
result, an obsolete constraint superseded by a correction, two entries both needed at
once, a quoted directive planted in tool output, and a run of distracting recent
output), nine instances each with different phrasing and fillers. Every template sits
wholly in development or wholly in evaluation, never both: `constraint`, `decision`,
`changed_file`, and `tool_result` are development-only; `correction`, `blocker`,
`obsolete_then_correction`, `multi_mandatory`, `quoted_directive_trap`, and
`distracting_recent_output` are evaluation-only, which is where the required coverage
(obsolete constraints, corrections, multiple relevant entries, quoted directives,
distracting recent output) lives. A case and its entries carry a neutral id, a `kind`,
and rendered `text` - no label, no mandatory marker. `annotations.json` holds the
mandatory ids, the label on each labelled entry, and why, frozen before any policy ran
and read only to grade a selection afterward, or to build the `oracle_label` upper
bound. `dataset.split_violations` checks every committed case for a family, a
template, or an exact duplicate history crossing the two splits, and finds none.

Four variants are compared at 260, 430, and 2,000 characters, including the
introduction and omission note. `recency` is the production renderer's own policy.
`rules` is deployable: a score from an entry's `kind` and the same folded evidence
every policy reads (`run.py:rules`), matching on patterns like `Correction:`,
`Decision:`, `Changed files:`, "unavailable/blocked/cannot", and a passed test-runner
result in a tool entry - never a label. `oracle_label` is not deployable: the
ground-truth mandatory set, reported only as an upper bound. `kev` scores that same
folded evidence through `evidence()`, the one function every policy's text comes from,
so a long tool entry's hidden, folded-away lines are invisible to Kev exactly as they
are to recency's and rules' budgets. All four keep exact rendered entry text and
restore source order; `predictions.json` records every selection, score, budget,
selected text, and omitted mandatory fact.

The one frozen Kev question scores each entry, unchanged from the first run. It uses
Kev package revision `f1535963cea021439370c23127bc970b6788e730` and
`jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101`. A per-entry call that
times out after 30s, or returns a non-finite score, is counted and dropped; a case
missing any score falls back to recency.

## Reserved-evaluation results

| Policy | Budget | Mandatory recall | Critical omissions | Useful retained | Budget compliant |
| --- | ---: | ---: | ---: | ---: | --- |
| Recency | 260 | 0.00 | 63 | 0.00 | yes |
| Recency | 430 | 0.29 | 45 | 0.16 | yes |
| Recency | 2,000 | 1.00 | 0 | 0.22 | yes |
| Rules | 260 | 0.95 | 3 | 0.56 | yes |
| Rules | 430 | 1.00 | 0 | 0.30 | yes |
| Rules | 2,000 | 1.00 | 0 | 0.22 | yes |
| Oracle (upper bound, not deployable) | 260 | 0.95 | 3 | 0.56 | yes |
| Oracle (upper bound, not deployable) | 430 | 1.00 | 0 | 0.30 | yes |
| Kev | 260 | 0.79 | 13 | 0.47 | yes |
| Kev | 430 | 0.94 | 4 | 0.29 | yes |
| Kev | 2,000 | 1.00 | 0 | 0.22 | yes |

Rules reaches the oracle_label upper bound exactly at 430 and 2,000 characters, and
comes within 3 of the 63 mandatory entries at 260. Kev recalls fewer mandatory entries
than rules at every budget where the budget binds (0.79 vs 0.95 at 260, 0.94 vs 1.00
at 430); the two agree only once the budget is wide enough that selection barely
matters (2,000). Recency, which does not read any entry's content, recovers no
mandatory fact at the tightest budget and recovers well under a third even at 430.

Kev's shortfall concentrates in two templates: `multi_mandatory` (two entries are each
needed; a tight budget holds two whole entries, and Kev sometimes keeps the wrong one
of the two) and `distracting_recent_output` (the mandatory entry is older than three
trailing distractors). `predictions.json`, filtered to `policy=kev`, has every case.

## Cost and environment

The model ran once under the shared operating-system lock on MPS with bfloat16 and
the automatic backend, temperature not applicable (a probability is read from the
model's own output head, not sampled). Model load alone - measured before any case
was scored, never across the loop that follows it - was 72,674 ms. The 288 entry
scores had warm p50 104.9 ms and p95 136.5 ms. There were 288 model calls, zero
timeouts, zero malformed answers, and zero cases that fell back to recency. Peak
process memory was 11,258,970,112 bytes from `getrusage ru_maxrss`.

The experiment used `~/.ariadne/ai-opportunities/hf` instead of the production cache
and loaded in process rather than through the daemon server. Those differences limit
latency and memory extrapolation to production.

## Recommendation and limits

On this corrected evaluation, the deployable rules baseline reaches the oracle upper
bound at 430 and 2,000 characters and comes close at 260, while Kev recalls fewer
mandatory entries than rules at every budget that actually binds. This favors the
free, deployable rules baseline over Kev for this selection task, on this dataset.
It does not rule out Kev for a harder or more varied real-session dataset than this
one: the rules baseline was written from the same labelled vocabulary the development
templates use, and a real session's phrasing may not match it as cleanly as these
fixtures do.

The fallback is recency selection when model loading fails, an entry times out, or any
score is missing, non-finite, or malformed; this run hit none of those. A later
integration should select after `render_entry` and before `apply_budget`, retain event
ids with entries, and mark user prompts as protected evidence.

This corrected evidence is ready for the user's implementation decision at the
investigation checkpoint; it does not itself make that decision.
