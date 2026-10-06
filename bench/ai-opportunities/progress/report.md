# Report: measuring Kev for stalled progress and user attention

## Question

Can an offline read of a session's recent events - the same evidence the daemon's quiet
watchdog has (specs 009, 008, 018) - tell useful progress apart from a repeating failure, a
correctly idle wait on a person, or a window too thin to judge, better than the watchdog's own
silence-only clock, and better than a naive repetition count?

## Method, in one paragraph

`bench_progress/synth.py` builds 122 synthetic windows across ten scenario families (a healthy
session, a single long-running tool call, legitimate retries, a healthy test-rerun loop that a
frequency-only rule would flag, a finished task, an identical command failing identically five
times running, four kinds of explicit wait, and three kinds of under-evidenced window), each
family wholly inside one split. `bench_progress/real_export.py` adds 20 sanitized windows read
read-only from the daemon's own `~/.ariadne/ariadne.db` (sessions from this very goal's own
history). Every field a policy sees for a real window - `status`, `attention_reason`, `now` - is
derived from the window's own last event alone, never from the live session row, which can
already describe the session after events the window does not show; `task_status` and
`goal_status` are left `None` for real windows, since the daemon keeps no point-in-time history
of either (`real_export.py::_derive_state`). 89 windows went to dev, 53 to eval - never mixed by
family. `python3 -m bench_progress.generate` rebuilds the 122 synthetic windows deterministically
every time and carries over whatever real-sanitized windows are already committed; only
`--refresh-real` re-pulls a sample from the live database, as a deliberate, logged, separate step.

Three policies were ported or built: `policy_baseline.py` (a pure-Python port of
`check_session_quiet`, pinned against the Rust thresholds by an `assert` and a test),
`policy_repetition.py` (escalates when one tool name is called three or more times in the last
six tool calls - nothing about its arguments or outcome), and Kev-4B
(`jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101`, run locally under the shared
advisory lock through `~/.ariadne/ai-permissions/venv/bin/python3` - the interpreter the
orchestrator confirmed already carries `kev` pinned to the required revision
`f1535963cea021439370c23127bc970b6788e730`, reused read-only), asked a four-way choice question
over the same four labels the dataset uses. Two prompt designs were tried on dev alone; the
better one (`prompts/v2.json`, 0.753 dev accuracy against 0.742) was selected before
`cases/eval.jsonl` was ever scored, then run on eval exactly once (0.792 accuracy there).
`policy_combine.py` is the integration point for a later rollout: the baseline's two explicit
waits always win, and an advisory may only *add* `escalate_unproductive` where the baseline would
otherwise do nothing, never remove an escalation the baseline already made.

## Headline numbers (eval, 53 windows, the reserved split)

| policy | coverage (loops caught) | false escalation rate (useful progress wrongly flagged) | missed user questions | detection delay (loop) | model calls |
| --- | --- | --- | --- | --- | --- |
| baseline (current watchdog) | 0.0 | 0.08 | 0 | never | 0 |
| repetition (naive) | 0.6 | 0.16 | 0 | 165s / 3 repeats | 0 |
| combined\_repetition | 0.6 | 0.24 | 0 | 165s / 3 repeats | 0 |
| **kev** | **0.8** | **0.08** | 0 | **120s / 2 repeats** | 53 |
| combined\_kev | 0.8 | 0.16 | 0 | 120s / 2 repeats | 53 (shared with `kev`) |

(`results.json`'s `metrics.eval` has the full set: coverage, both escalation rates, missed user
questions, insufficient-evidence escalations, latency and the real per-policy model-call count,
for every policy; `dev` holds the same for the tuning split. A model-free policy's `model_calls`
is 0, and `combined_kev` spends no call beyond the one `kev`'s own prediction already made for
the same window.)

## What each number means

**The baseline never catches the loop it cannot see.** `loop-identical-command`'s five synthetic
snapshots repeat an identical failing `cargo test` every 45 seconds; each repeat refreshes
`last_activity_at`, so `quiet_seconds` never crosses even the 180-second nudge threshold. This is
the gap task item 13 asked to separate from ordinary silence: the watchdog's one clock answers
"has it been quiet", and a session that keeps reporting failure after identical failure is never
quiet by that clock, however little it is accomplishing. Coverage 0.0 is not a bug in the port -
it is the watchdog correctly doing the one thing it was built to do, and nothing past that.

**The baseline's own false escalations come from long tool calls, not loops.** Its two eval false
escalations (and three on dev) are all `progress-long-tool` cases: a single `cargo build
--release` still running, silent in the console because it has not finished, misread as
`stalled` once the silence crosses 600 or 1800 seconds.

**Kev's false escalations on eval come from two real-sanitized sessions, not the synthetic
foils.** All of Kev's 53 eval calls on the synthetic half of the dataset came back correct on
`useful_progress`; its 2 false escalations are both real-sanitized sessions this bench's own
after-the-fact heuristic (`real_export.py::_infer_label`) called `useful_progress` because no
run of 3+ identical-input-hash tool calls showed in their tail, where Kev read enough repetition
in the same tail to call it `repetitive_failure` instead. Neither label is a confirmed ground
truth for those two sessions (`label_source="inferred"`, `ambiguous=true` in the case file) - this
is exactly the kind of disagreement the real-sanitized subset is for surfacing, not a clean miss
to score Kev down for without a human check against the original transcript.

**The naive repetition rule trades one blind spot for another.** It catches 3 of the 5 loop
snapshots (coverage 0.6) by counting tool names alone, but it also fires on every
`progress-testloop-freq` case - a real edit before every rerun, the failing count falling each
time - because `pytest` is still the same tool name four times running. That is exactly the
"healthy session a simplistic detector would flag" the acceptance criteria asked this comparison
to include, reproduced for real: false escalation rate 0.16, double the baseline's 0.08.

**Combining naively adds blind spots, it does not fix them.** `combined_repetition`'s false
escalation rate (0.24) is *higher* than the repetition rule alone (0.16), because
`policy_combine`'s integration rule never softens a baseline escalation - so it keeps the
baseline's long-tool false positives *and* adds the repetition rule's test-loop false positives
on top. The same shows up for Kev: `combined_kev`'s false escalation rate (0.16) is higher than
Kev alone (0.08), for the identical reason - the baseline's two long-tool escalations survive the
combination even though Kev itself judged both of those particular cases correctly as progress. A
rollout that wants Kev's full benefit on long-running calls would need the integration rule to
let a confident advisory answer soften a stalled flag too, not only add an unproductive one; this
bench does not make that change, since task item 15 asked only for the integration point to be
named, not decided.

**Kev detects the loop one repeat sooner than the naive rule, and the baseline never does.**
Of the loop family's five time-ordered snapshots, Kev's first escalation lands at the second
repeat (120 elapsed seconds); the repetition rule needs its third (165s); the baseline never
crosses its own clock at all for this family. This is the detection-delay comparison task item
12 asked for "where timestamps permit" - computed only for the two `multi_snapshot`
`loop-identical-eval-*` families, since they are the only ones with several real decision points
over time for the same loop.

**No policy mishandled an explicit wait, on either split.** `missed_user_questions` is 0 for
every policy on both dev and eval: the four kinds of explicit wait this dataset builds
(`waiting_permission`, `waiting_input`, `agent_error`, an idle planning orchestrator, and an
already-raised `waiting_user` flag that must survive the 600-second mark without being
re-escalated to `stalled`) are all preserved, by the baseline's own guard and by every combined
policy built on top of it (task item 4). The one case constructed specifically to probe the edge
of rule 18 - `waiting_user_flag`'s quiet time sits inside `[600, 1800)` on purpose, since past
1800 seconds the real daemon *does* relaunch a `waiting_user` session (rule 18's other half) -
passes for exactly the reason the spec gives, not by accident; `test_baseline_policy.py` pins
both halves of that rule as separate cases.

**Insufficient evidence is a real category Kev reads well.** Prompt `v1` never once chose
`insufficient_evidence` on dev (0/9); `v2` named it correctly on every one of the 9 dev cases and
6 of 6 eval cases, which is most of why `v2` was selected
(`predictions/kev_v1_raw_dev.jsonl`, `kev_v2_raw_dev.jsonl`, `kev_raw_eval.jsonl`, scored by
`bench_progress/score_kev_raw.py`). None of the three under-evidenced synthetic scenarios (a
missing activity timestamp, a transcript truncated to one tool call's end with no start, and a
nine-thousand-second gap standing in for a dropped console event, spec 008 rule 15) are things
any policy should escalate on, and none of them did
(`insufficient_evidence_escalation_rate` is 0.0 for every policy, both splits).

## Cost

One Kev call per window: 53 calls to score the whole eval split (`model_calls` in
`results.json`'s `metrics.eval.kev`; `combined_kev` reuses the same 53 calls rather than spending
a second round). Warm p50 latency on eval was 585ms, warm p95 1.91s
(`results.json`'s `run.kev_inference.measured_runs.prompt_v2_eval_selected`); cold start (the
first call after loading the checkpoint) was 36.1 seconds. Peak process RSS was about 12.5GB,
loading the `Qwen/Qwen3.5-4B-Base` backbone plus the Kev LoRA adapter in bf16 on Apple Silicon
(`mps`, `sdpa` attention) through `~/.ariadne/ai-permissions/venv/bin/python3` - the interpreter
the orchestrator named as already carrying `kev` at the required revision, reused read-only -
against weights cached under the shared experimental `~/.ariadne/ai-opportunities/hf`, not the
production `ai-permissions` cache; not necessarily the shape or hardware Ariadne's production
inference would run under (see `results.json`'s `run.kev_inference.environment`). A
`multi_snapshot` loop family costs one call per snapshot (up to 9 for this dataset's largest
family); every other family costs exactly one call per replayed decision point
(`dataset.model_calls_per_family` in `results.json`: min 1, max 9, mean 2.96 across the 48
families).

## Supported vs. inferred vs. synthetic

- **Synthetic, by construction** (122 windows): the label is true by how the window was built,
  not observed. These carry the comparison's main weight and are fully deterministic to
  reproduce (`python3 -m bench_progress.generate` with no flag never touches the live database).
- **Real, observed** (the subset of the 20 real-sanitized windows whose last event directly names
  an explicit wait - `real_export.py::_derive_state` reading `permission_request` or
  `session.error` as the most recent event): the label follows directly from the window's own
  evidence, not a live column that could have moved on since.
- **Real, inferred** (the rest of the 20): a judgement call from the same tail-of-events heuristic
  every policy is graded by, flagged `ambiguous=true` in the case file, and never treated as
  ground truth strong enough to anchor a threshold decision on its own - see Kev's two eval false
  escalations above, both against this category.

## Recommendation

**Investigate further.** On this dataset, Kev clearly outperforms both the current watchdog
(which structurally cannot see activity-without-progress, by design, not by a bug) and a naive
repetition count (which structurally cannot see progress-without-novelty) on the one claim this
bench set out to test - 0.8 loop coverage against 0.0 and 0.6, at a lower false escalation rate
than the naive rule (0.08 vs 0.16) - at a cost of one call per decision point and sub-second
warm p50 latency. That is not evidence that any session should be killed, restarted, or have its
watchdog behavior changed from this measurement alone (per the task's own acceptance criteria):
142 windows, 20 of them real and inferred rather than observed, is a pilot, not a production
bound, and two concrete weak points surfaced here are themselves the next questions for whoever
designs the rollout - first, that bolting an advisory onto the current integration rule inherits
the baseline's long-tool false positives rather than fixing them, and second, that Kev's own
false escalations on this split trace to real sessions this bench's own labelling heuristic is
not fully confident about either, which argues for a human-reviewed real-data sample before any
threshold is picked from real-world evidence rather than synthetic constructions alone.
