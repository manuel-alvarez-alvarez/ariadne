# Kev failure-diagnosis measurement

## Question

Can Kev improve advisory diagnosis after the current structured signals and exhaustion patterns?

## Method

The baseline reproduces `exhausted_reason` in `crates/ariadne-daemon/src/acp.rs` and `default_exhausted_patterns` in `crates/ariadne-daemon/src/config.rs`. The scheduler switches only sessions already marked exhausted, as specified in `specs/009-scheduler-attention-and-watchdogs.md` rule 36. ACP rule 14 in `specs/021-acp-runtime.md` defines the structured fields and text patterns.

The frozen corpus contains 125 synthetic cases: 80 development and 45 reserved evaluation cases. It covers quota exhaustion, temporary failures, authentication or configuration failures, task errors, and insufficient evidence. It includes quoted errors, negation, unrelated quota mentions, unfamiliar exhaustion wording, and conflicting evidence. No private transcript appears in the repository.

Labels and recovery categories come from each case's stated evidence and rationale before scoring. They do not use Kev output. The selected one-prompt design is a five-way diagnosis choice. The evaluation runs it once on the reserved cases. A separate direct exhaustion decision maps every policy through `label == exhausted`; the finer labels remain separate because the current detector has no finer diagnostic output.

## Policies

| Policy | Decision rule |
| --- | --- |
| baseline | Current structured fields and configured default substring patterns. |
| kev | Kev-4B diagnosis alone. |
| rules_first_kev | Preserve protocol exhaustion, apply narrow contextual rules, use Kev, then use baseline after no Kev answer. |

The fallback retains current behavior only when Kev returns no usable probability vector. Explicit protocol exhaustion always remains exhausted in the combined policy.

## Measured result

`results.json` records the package revision, pinned checkpoint, device, precision, command, cold startup, warm p50 and p95, peak memory method, malformed-answer count, per-split confusion matrices, exhaustion precision and recall, wrong switch recommendations, missed exhaustion, and abstention coverage. `predictions.jsonl` records each probability distribution and latency.

On the 45 reserved cases, baseline exhaustion precision and recall were 0.667 and 1.000. It made five wrong switch recommendations. Kev and the combined policy both reached 1.000 precision and recall with zero wrong switch recommendations. This is pilot evidence because every case is synthetic.

Kev loaded in 445.3 seconds. Its warm latency was 172.7 ms p50 and 190.3 ms p95 across 125 cases. The macOS process peak was 10,257,727,488 bytes by `ru_maxrss`. No answer was malformed. The retained predictions show that Kev incorrectly proposed exhaustion for five development insufficient-evidence cases, so the development result does not support automatic switching.

The measured environment is a local macOS experiment with the isolated `/Users/malvarez/.ariadne/ai-opportunities/hf` cache. Production currently runs no failure-diagnosis model. Therefore, the result cannot estimate production latency or reliability.

## Recommendation

Investigate further. Enable no automatic recovery from this experiment. Automatic recovery needs held-out sanitized production errors, repeated latency and memory measurements on the production device, a bounded false-switch rate, a review of the fallback implementation, and scheduler integration tests proving that only an explicit exhausted diagnosis reaches the existing auto-switch seam.

The strongest counter-evidence is the fully synthetic corpus. Real failures can contain adapter-specific wording and correlated signals absent here. That evidence, or stable evaluation precision and recall on a larger real held-out set, would change the recommendation.

## Integration seam

An advisory classifier can run beside `acp.rs::exhausted_reason` after it builds the error message and data. Preserve `usageLimitExceeded` and the JetBrains `limit` category before model judgment. Send only an exhausted decision to the existing `session.error` fields that `scheduler/auto_switch.rs` consumes. Store any finer diagnosis as advisory metadata until the evidence above supports a policy change.
