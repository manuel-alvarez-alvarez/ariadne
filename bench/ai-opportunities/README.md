# Where Kev could help Ariadne

## Purpose

This report joins the reviewed inventory in [`survey.md`](survey.md) with three
experiments. It ranks the measured opportunities and gives concrete scopes for
future implementation tasks. The experiments are pilots. They do not establish
production reliability, and none authorizes automatic recovery or watchdog
changes.

Kev is a pinned Qwen model with a LoRA adapter. A request can contain multiple
typed questions, including scored questions. It does not generate text. All
three experiments used Kev-4B
(`jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101`) and package
revision `f1535963cea021439370c23127bc970b6788e730`. Device and memory costs
below are local MPS measurements, not production estimates.

## Ranked measured findings

The ranking weighs valid measured benefit, observed errors, implementation
effort, and operating cost. Higher-ranked work still needs stronger evidence
before implementation. The repaired handoff comparison is included with its
synthetic-data limits. The earlier invalid run remains archived separately.

| Rank | Opportunity | Measured result | Errors and limits | Effort and operating cost |
| --- | --- | --- | --- | --- |
| 1 | Failure diagnosis | On 45 synthetic held-out cases, Kev and rules-first Kev had 1.000 exhaustion precision and recall, with zero wrong switch recommendations. The current baseline had 0.667 precision, 1.000 recall, and five wrong switch recommendations. | All 125 cases were synthetic. On development cases, Kev made five wrong switch recommendations by calling insufficient evidence exhausted. The evaluation set is small; no recovery ran. | A classifier beside ACP exhaustion detection is plausible. Warm p50/p95 was 173/190 ms; model load took 445 seconds and peak memory was about 10.3 GB. |
| 2 | Progress detection | On 53 held-out windows, Kev caught 8/10 loop windows at 120 seconds, versus repetition's 6/10 at 165 seconds and the silence watchdog's 0/10. Kev's false escalation rate was 0.08, equal to the baseline and half the repetition rule's 0.16. | The split had 48 synthetic windows and five real-sanitized windows. Two real inferred cases were ambiguous and received false escalation labels; the exact truth is unknown. Combining Kev with the baseline raised false escalations to 0.16 because it retained baseline long-tool errors. | Requires a new event-window representation and careful policy integration. Eval warm p50/p95 was 585 ms/1.91 s, with 53 calls; cold start was 36.1 seconds and peak RSS about 12.5 GB. |
| 3 | Handoff history selection | On 54 independent synthetic evaluation histories, deployable rules recalled 0.95 of mandatory evidence at 260 characters and 1.00 at 430; Kev recalled 0.79 and 0.94. Recency recalled 0.00 and 0.29. At 2,000 characters, all three reached 1.00. | All 90 histories were synthetic; no real session history was scored. The rules use vocabulary from development templates, so real phrasing may differ. The benchmark selects entries; it does not test downstream task completion. | Rules need no model serving. Kev used 288 calls; warm p50/p95 was 105/136 ms, model load was 72.7 seconds, and peak memory was about 11.3 GB. Production costs may differ. |

### Evidence and reproduction

Each link opens the saved prediction records used by the reported metrics. Run
commands from the repository root unless a report says otherwise.

| Area | Saved predictions | Reproduction |
| --- | --- | --- |
| Failure diagnosis | [`failures/predictions.jsonl`](failures/predictions.jsonl), [`failures/results.json`](failures/results.json) | `python3 bench/ai-opportunities/failures/make_cases.py`; `HF_HOME=/Users/malvarez/.ariadne/ai-opportunities/hf python3 bench/ai-opportunities/failures/lock_run.py /Users/malvarez/.ariadne/ai-permissions/venv/bin/python3 bench/ai-opportunities/failures/experiment.py measure`; recompute with `/Users/malvarez/.ariadne/ai-permissions/venv/bin/python3 bench/ai-opportunities/failures/experiment.py recompute` |
| Progress detection | [`progress/predictions/eval.jsonl`](progress/predictions/eval.jsonl), [`progress/predictions/kev_raw_eval.jsonl`](progress/predictions/kev_raw_eval.jsonl), [`progress/results.json`](progress/results.json) | `python3 -m bench_progress.generate` from `bench/ai-opportunities/progress`; `python3 bench_progress/kev_runner.py --prompt prompts/v2.json --input cases/eval.jsonl --output predictions/kev_raw_eval.jsonl --stats predictions/kev_stats_eval.json --policy-name kev`; then `python3 -m bench_progress.replay --split eval --kev-raw predictions/kev_raw_eval.jsonl --kev-policy-name kev` and `python3 -m bench_progress.build_results > results.json` |
| Handoff history | [`handoff/predictions.json`](handoff/predictions.json), [`handoff/results.json`](handoff/results.json), [`handoff/cases.json`](handoff/cases.json), [`handoff/annotations.json`](handoff/annotations.json) | `python3 bench/ai-opportunities/handoff/dataset.py`; `python3 bench/ai-opportunities/handoff/run.py`; recompute from saved predictions with `python3 bench/ai-opportunities/handoff/run.py --metrics-only`. The [handoff report](handoff/report.md) records the inference setup. |

Dataset generation may preserve or refresh real-sanitized samples as each
experiment documents. The saved predictions and results identify the exact
measured dataset. Do not treat refreshing a sample as reproducing the same
measurement. The invalid handoff pilot is retained under
[`handoff/pilot-invalidated/`](handoff/pilot-invalidated/); do not use it as
evidence.

## Implementation candidates

These are proposals. They are not measured production features. The user should
select which, if any, becomes executable work.

### A. Advisory failure diagnosis

- **User-visible path:** An author sees a useful failure category in task
  attention details; the existing task reason remains visible. Automatic model
  switching stays behind both current exhaustion paths: explicit protocol
  signals and configured text patterns.
- **Sources and scope:** Normalize failed-task reason and available ACP fields
  near `crates/ariadne-daemon/src/acp.rs::exhausted_reason`; attach advisory
  classification near the existing session error and
  `crates/ariadne-daemon/src/scheduler/auto_switch.rs` seam. Do not change core
  transition rules.
- **Fallback:** Preserve explicit protocol signals (`usageLimitExceeded` and
  the JetBrains `limit` category) and configured text patterns. Store model
  diagnoses as advisory metadata only. Do not let the model change either
  existing path unless the user approves a policy change.
- **Acceptance criteria:** Tests prove explicit protocol signals and configured
  text patterns still reach their current paths; malformed, timeout, and
  missing answers use current behavior; model advice cannot change either
  path. Validate
  against a larger, human-reviewed, sanitized held-out error set before any
  automatic recovery policy.
- **Evidence needed first:** Real error payloads, bounded false-switch rates,
  production device startup/latency/memory, and integration tests. The
  synthetic pilot alone does not justify implementation of automatic recovery.

### B. Advisory progress signal

- **User-visible path:** The session attention view explains a repeated-failure
  signal and may surface a suggested human check; it does not terminate or
  relaunch the session from this experiment.
- **Sources and scope:** Build a bounded recent-event representation from
  `crates/ariadne-store/src/events.rs::Store::list_events`, then integrate an
  advisory in `crates/ariadne-daemon/src/scheduler/quiet.rs::check_session_quiet`.
  Keep the event window and decision trace inspectable.
- **Fallback:** On model failure or insufficient evidence, use current quiet
  thresholds. Preserve explicit waiting states. Do not allow a repetition-only
  rule to trigger a restart.
- **Acceptance criteria:** Tests cover explicit waits, long-running tools,
  improving test loops, repeated identical failures, missing events, and model
  failure. A human-reviewed real-session evaluation must lower missed loops
  without raising the false-escalation rate over an agreed bound. Measure
  production startup, p95 latency, and memory before rollout.
- **Evidence needed first:** Resolve ambiguous real labels and test a policy
  that handles long-tool false positives. The combined policy in this pilot
  worsened false escalation from 0.08 to 0.16.

### C. Deterministic handoff relevance selection

- **User-visible path:** A resumed session receives a bounded handoff that
  preserves constraints, corrections, blockers, decisions, changed files, and
  decisive tool results.
- **Sources and scope:** In `crates/ariadne-daemon/src/agents/handoff.rs`,
  rank rendered entries before `apply_budget` using observable entry kind and
  text. Retain event IDs and user-prompt provenance. Never read annotations or
  rewrite selected entry text.
- **Fallback:** Use current recency selection when rules cannot rank an entry.
  Preserve source order in the rendered handoff and keep user prompts eligible
  for selection.
- **Acceptance criteria:** Tests prove exact character-budget compliance,
  source-order restoration, label independence, and recency fallback. Evaluate
  mandatory-fact recall against recency on human-reviewed real sanitized
  histories. Compare output and omissions at the production budget.
- **Evidence:** On this synthetic evaluation, rules outperform Kev at both
  binding budgets and match the oracle upper bound at 430 characters. The
  results favor rules on this dataset, but real-session evidence is still
  required before rollout. Do not select Kev without evidence that it beats
  rules on representative histories.

## Further evidence candidates

The survey identifies other possible uses, but this experiment set does not
measure them:

- **Attention ordering:** Sort already attention-flagged sessions by urgency in
  the attention list (`crates/ariadne-daemon/src/attention.rs`); fall back to
  current recency order. First analyze `session_ended` durations by reason and
  measure whether the order improves human response time.
- **Review triage:** Order pending reviews by diff risk in
  `Scheduler::rouse_reviewer_for` under
  `crates/ariadne-daemon/src/scheduler/tasks.rs`; fall back to current FIFO
  order. First validate against existing reviewer verdict and requested-change
  statistics. Never change unanimous approval.
- **Model suggestion:** Suggest a model pin before first launch through
  `crates/ariadne-daemon/src/http/pins.rs`; leave the user's pin unchanged on
  invalid output. First collect task outcomes by pin and test whether scored
  classification can predict useful cost savings.
- **Model sharing:** Measure concurrent `kev.serve` load and permission latency
  before sharing an instance across uses. No experiment here measures
  contention or production serving.

The inventory in [`survey.md`](survey.md) records current behavior, possible
inputs, decision seams, and risks for these candidates. The progress detector
may also need a sequence-aware model rather than Kev's single-state scored
question.

## Decision checkpoint

The user selected optional advisory failure diagnosis and deterministic
handoff ranking with: “Continue with your recomendation”. The orchestrator
created these implementation tasks:

- `01m49pr97j46eazg2zjhmd6x11` — Show optional AI failure diagnoses without
  changing recovery.
- `01m49pshxt2eaebnr9gwbnm832` — Preserve important session history with
  deterministic handoff ranking.

Progress detection remains in evaluation. These selected scopes are
implementation work in progress. The measured findings above describe only
their experiments; they do not claim that either implementation has landed.
