# Historical token-use baseline and recommendations

## Decision checkpoint

This is a historical audit, not authorization to alter production prompts,
skills, model assignments, or workflow gates. The user should select stable
recommendation identifiers below before implementation work is created.

## Question and answer

**Question:** Where did this Ariadne installation spend avoidable tokens and
elapsed time, and which bounded improvements should be tested first?

The records demonstrate high cumulative usage and review cycles, but not
avoidable model work. `TU-01` through `TU-04` are diagnostic or controlled-test
candidates, not measured savings. Add attribution before suppressing a
wake-up, check, handoff entry, or tool result.
Required checks and review columns remain part of every comparison.

## Snapshot and coverage

- Cutoff: `2026-10-14T20:31:49.158Z`; analyzer version `2`; source SHA-256
  `b63f022f7c51a3b17d620196e55f504f4582744a4e8dab6e4cc3a57c47041f47`. The raw database is deliberately not in Git.
- Primary source: `pinned snapshot /tmp/ariadne-token-usage-snapshot.XXXXXX.db`. The inventory also inspected
  6 dated backups. Their retained IDs, event IDs, and
  shared usage counters are compared in `snapshot.json`; they are not summed.
- The primary source holds 54 surviving goals, 180
  surviving tasks, 615 surviving sessions, 171122
  events, and 1072 cumulative transcript-source rows. It has
  3 running sessions and 1
  in-progress tasks, reported as partial.
- The task listing supplied with this study said 48 goals and 179 tasks from
  October 5–14. The database has a wider surviving inventory; neither source
  proves that deleted rows before the retained ledger are complete.
- Local transcript metadata scanned 2,901 files and mapped
  {'codex': 227, 'claude': 370} retained internal sessions.
  It found 349 final vendor counter snapshots;
  171 are within 1% of the corresponding
  Ariadne session input and output. The rest are retained as discrepancies,
  not adjusted into the canonical total. The retained, cutoff-filtered
  transcript manifest SHA-256 is `eeb5a52f535138ca551715c202c5b9ebe41e0667e3a51f94be7d0e302864f471`.

## Counting rules and reconciliation

`input_tokens` includes cached input; `cached_input_tokens` is a subset, so
uncached input is `input - cached`, never another additive total. Canonical
usage is the sum of `session_usage` once per `(session_id, source)`, because
those are cumulative transcript totals that replace earlier reports. It is
10,837,777,871 input (156,455,029 uncached +
10,681,322,842 cached) and 30,300,608 output tokens.

The session-end ledger is not added to that total: an end fact stores the
session's cumulative usage and a restarted session can produce more than one
such fact. Summing it would duplicate tokens. For deleted sessions, the
latest surviving end fact is a fallback only when there is no live usage row;
3 such fallback(s) were needed here.
26 surviving session(s) have no usage row:
that is **missing usage**, not demonstrated zero. Sessions with an actual
usage row of zero are counted separately (0).

`reconciliation.json` records direct usage, non-additive end facts, fact
counts, and launch-source matching. ACP writes the launch id as `source`.
Retired launch rows can be absent after a relaunch, so unmatched sources keep
only the evidence retained in facts.
Transcript metadata maps to sessions, not launch IDs. Therefore it validates
representative cumulative session counters but cannot distribute a vendor
counter across a session's launch sources. `transcript-reconciliation.csv`
records each such comparison and its retained launch count.

## Inventory and aggregates

The files below cover every surviving goal and task, including cancelled and
failed/retried work where its rows or facts remain. `goals.csv` and
`tasks.csv` use canonical session usage; `dimensions.csv` groups the same
canonical total by role, workflow column, model, effort, and current outcome.
`outcomes.csv` also retains task-end facts, whose count can exceed surviving
tasks because retries write another ending.

- [snapshot.json](snapshot.json) — cutoff, revisions, coverage and counts.
- [goals.csv](goals.csv) and [tasks.csv](tasks.csv) — one aggregate per
  surviving entity, with distinct token categories and partial flags.
- [dimensions.csv](dimensions.csv) — comparable dimensional totals.
- [entity-attribution.csv](entity-attribution.csv) — retained per-session
  goal/task role, column, model, effort, outcome, and usage-evidence rows.
- [outcomes.csv](outcomes.csv), [timing.csv](timing.csv), and [reconciliation.json](reconciliation.json)
  — retained facts, gaps, and non-additive counter checks.
- [completion-timing.csv](completion-timing.csv) and [completion-summary.json](completion-summary.json)
  — every ending fact and the latest ending selected per retained task.
- [goal-analysis.csv](goal-analysis.csv) — every goal's dominant contributor,
  outcome evidence, and coverage limitation.
- [transcript-reconciliation.csv](transcript-reconciliation.csv) — local
  transcript metadata matches and vendor-counter comparisons without paths or text.
- [trace-selection.json](trace-selection.json) — selected trace identifiers
  and derived indicators only; raw payloads stay outside Git.
- [audit-overhead.csv](audit-overhead.csv) — this audit's partial goal usage,
  kept separate from completed historical comparisons.

## Timing evidence

The database records session and task wall-clock lifetimes, not CPU time or
model active time. It records console-permission waits in ledger facts where
available, but it does not separate queue delay, human wait outside a
permission prompt, tool execution, model inference, or active execution.

- Ended-session lifetime observed: 3,910,876 s
  across 612 sessions. This is inclusive
  wall time, so it must not be called execution time.
- Goal and task CSV row lifetimes use their stored `updated_at`; they are not
  completion measurements and can include later metadata changes or retries.
  Future comparisons use the latest `task_ended` fact per retained task;
  `completion-summary.json` gives its distribution and prior facts remain
  retry evidence in `completion-timing.csv`.
- Running work is partial at the cutoff and excluded from completed-work
  elapsed comparisons. This review's own goal is separately labelled in the
  aggregates when present; its running session has no final usage at cutoff.

## Trace method and findings

The analyzer selects the twelve largest input-plus-output sessions plus
median-usage completed agent and reviewer sessions. The selected traces cover
34.2% of input plus output; separate category
coverage is in `trace-selection.json`. `trace-findings.csv` gives event IDs
and separates observed signals from inference and untested benefits. Derived
event counts flag repeated daemon prompts, repeated completed tool calls, and
large ACP tool-result payloads; none alone proves avoidable work.

Within the selected traces, the largest observed daemon-prompt count is
34 and the largest observed daemon-prompt text
volume is 61,872 characters. The selected
trace with the most repeated exact commands has
0. These indicators establish where to
sample; they do not establish that every repetition is avoidable.

The trace review separates full prompts with distinct reviewed SHAs from short
continuations. The full review returns are distinct correction cycles, not
duplicate copies. Short continuations lack delivery outcomes, so the audit
does not classify them as waste. Large tool payloads remain diagnostic signals:
the ledger does not say which later prompt included a payload.

The repository already limits several suspected sources: skills are indexed as
one line and opened only when needed (`agents/prompts.rs`), handoff rendering
folds entries and applies a character budget (`agents/handoff.rs`), and
transcript counters are cumulative per source (`store/usage.rs`). These are
implemented safeguards, not new recommendations. Their presence also means a
larger handoff or prompt metric is a reason to inspect a selected trace before
changing policy.

## Ranked candidates

### TU-01 — Attribute scheduler deliveries before considering coalescing

**Evidence.** Selected traces include review returns with distinct reviewed SHAs and short continuation prompts. They prove delivery volume, but not redundant work or per-delivery tokens.

**Bounded scope.** Add diagnostic delivery IDs, causes, and completion outcomes at the scheduler/ACP boundary. Do not suppress a delivery, change a workflow column, or change prompt text.

**Expected benefit.** No claimed saving until matched evidence identifies duplicate deliveries. It enables a bounded later coalescing experiment. Confidence: high for measurement; no savings estimate.

**Quality guard and measurement.** Compare delivery causes, completed responses, prompt characters, canonical uncached input, selected completion lead time, and finish/review outcomes under unchanged checks.

**Counter-evidence.** The largest reviewed cycle contains distinct review revisions. Treating it as duplicate wake-up work would weaken correction quality.

### TU-02 — Measure and tighten handoff selection at the existing character budget

**Evidence.** The code already folds and ranks handoff history, but cumulative session usage cannot show which selected entries caused input. Long/restarted sessions are identifiable in the trace sample, making this a bounded observability-first candidate.

**Bounded scope.** Add offline/diagnostic handoff selection metrics and a fixture corpus before changing the ranking policy. Do not shrink the configured budget or discard corrections, blockers, failed commands, edits, or decisive checks without an outcome comparison.

**Expected benefit.** Estimated lower repeated input and faster resume turns where a handoff is much smaller than raw history. Confidence: low.

**Quality guard and measurement.** For matched resumes, record candidate/selected character counts, continuation turns, canonical tokens, time to next useful state, and task outcome. Preserve existing handoff behavior as the control.

**Counter-evidence.** The existing rules intentionally protect high-value evidence. A smaller handoff that causes re-reading, wrong changes, or review churn is a regression.

### TU-03 — Keep verification policy unchanged; add no suppression candidate

**Evidence.** The selected traces contain zero repeated completed exact commands after opener-to-completion matching. Earlier prefix/title matching conflated different commands and edits.

**Bounded scope.** Do not change checks or add command suppression. Revisit only if future attribution records an identical command, unchanged revision, and completed prior result.

**Expected benefit.** No expected saving from this baseline. Confidence: high.

**Quality guard and measurement.** Keep required checks unchanged. If a later duplicate is found, compare tool duration, retry rate, review findings, and task outcomes before any change.

**Counter-evidence.** The selected traces have substantial review-cycle work, but that is different revisions and corrections, not duplicate verification.

### TU-04 — Bound broad tool-result retention with task-aware summaries

**Evidence.** The trace indicators count large stored tool-result payloads. Tool outputs may be echoed into agent context or handoffs, but this database does not record prompt assembly per turn, so causal token savings are untested.

**Bounded scope.** First add a report of large result payloads by tool, role, and outcome. Any later truncation must keep exit status, failing lines, paths, diffs, and an explicit way to retrieve full output.

**Expected benefit.** Potentially lower input context and handoff size, plus faster model turns on noisy command output. Confidence: low.

**Quality guard and measurement.** Compare prompt/handoff size, canonical token categories, diagnosis success, retry rate, and reviewer findings on matched command-heavy tasks.

**Counter-evidence.** Full output can contain the only failure clue. Do not truncate before a representative trace review demonstrates redundant material.


## Limitations and what would change the ranking

The audit cannot reconstruct tokens per prompt or tool call, because the
database stores cumulative launch counters rather than per-turn usage. It
cannot infer active execution time from wall-clock fields. A model
or effort comparison is observational and confounded by task size, role, and
outcome; compare like role/column/outcome workloads before acting. The ranking
would change if a controlled before/after run preserves the same required
checks and shows no reduction in tokens or lead time, or if trace review shows
the flagged content is necessary to correct outcomes.

## Reproduction

See [README.md](README.md). The command reads a database in read-only mode;
copy a live SQLite database with its WAL using SQLite's backup API first, and
keep that copy outside the repository.
