# Request-level transcript follow-up

## Question and answer

**Question:** What avoidable token use and elapsed time can saved transcripts establish before new production instrumentation?

Saved Claude transcripts establish per-request usage for mapped sessions and native request, response, turn, and tool IDs. Saved Codex `token_usage_record` rows establish one observed `usage` field per native response ID; cumulative counters and overlapping turn/thread totals remain excluded. They establish observed request spans and tool-result sizes. They do not establish billed prompt composition, active execution time, delivery causes, rendered handoff selection, or causal savings. The baseline remains the canonical cumulative total.

## Reproducible evidence

Cutoff: `2026-10-14T22:17:05.521Z`. Source: `SQLite backup created 2026-10-15` at `/tmp/token-followup.db`. Source SHA-256: `adfae5cd052310fedd19d3f7b2aa4742c1a6f2a60f546a65de32f7894bc2a06a`. Source revision: `70df639377a3956f9e85991a08a35e40678d9194`. Analyzer SHA-256: `770a8bae817ca3ef8f8cefa478de561953959f82543c646e66810621f1befb5a`. `followup-transcript-manifest.csv` has a per-source retained-content hash; it does not hash records after the cutoff.

The follow-up found 48,769 retained request records, all mapped through native session IDs. It discarded 81,531 repeated Claude blocks and excluded 6,782 ambiguous-usage request identities (6,782 identities including missing usage) in `excluded-requests.csv`. The CSV artifacts retain no raw transcript text.

## Cohorts, accounting, and timing

`cohort-comparison.csv` uses the supplied frozen SQLite snapshot to intersect native `agent_sessions.id` values. It separates those shared identities, new sessions, and this goal by explicit goal ID. These transcript cohorts are not a before/after experiment.

Request input includes cached input. Uncached input equals input minus cached input. `request-reconciliation.csv` compares current retained requests with current session counters. `shared-cohort-reconciliation.csv` compares requests at or before the frozen cutoff with frozen per-session counters, and records post-cutoff requests in shared sessions separately. The baseline hash is recorded in `followup-snapshot.json`. Residuals label missing retained requests separately; all other residuals remain unresolved where counters, resets, and launch scope do not identify a unique cause. `followup-reconciliation.json` names the frozen baseline canonical total and fallback rule separately from the current snapshot; neither total replaces the other.

`manual-inspection.json` traces the three largest absolute input residuals. They have 11/12, 6/6, and 5/5 frozen counter-source/session-start counts. Two positive-residual cases have every frozen source triple matched to Codex cumulative snapshots. The recorded decompositions separate sum-minus-largest cumulative overlap from largest-minus-request scope: 60,822,618 plus -374,225 equals 60,448,393 in one case, and 59,488,061 plus -372,122 equals 59,115,939 in the other. Cached-input and output decompositions are recorded too. The negative residual case shows why exclusions cannot explain every mismatch.

`session-timeline.csv` measures only first-to-last timestamped model request in a session. It never sums per-request intervals, so parallel requests and waits do not inflate elapsed time. It does not measure model inference, tool runtime, queueing, or human wait.

## Wakeups, handoffs, and tool results

`wakeup-analysis.csv` joins a daemon prompt to subsequent mapped requests in the same session. It labels shared first-later-request matches as `ambiguous_competing_delivery`; a candidate is temporal, not causal. Its delay is event creation to recorded transcript timestamp, not request start or active execution. Events retain no vendor request ID, delivery cause, or outcome correlation. Session/task status and ending facts remain lifecycle outcomes.

`handoff-analysis.csv` inventories each native `switched_from` pair and measures the first saved successor prompt. It marks retained delivered handoff text separately from three missing successor-prompt candidates. The historical renderer revision and pre-budget selected-entry set remain unavailable, so delivered length does not prove which history caused an outcome.

`tool-output.csv` links Claude and Codex tool calls/results by native tool-call ID. It separates mapped requests, unmatched calls, and unavailable request associations. Result characters describe retained text only; arrays of text blocks are measured and unsupported shapes are labelled. They are not billed-token measurements or proof of later prompt inclusion.

## Manual record inspection

`manual-inspection.json` selects the three largest mapped-session input totals and one median mapped reviewer session. Each selected daemon event has a later native request and a finished task outcome; one major session has a correction-marker delivery among thirteen. Those are supported sequence facts, not evidence that the delivery caused the work. The first major transcript has 17 native error results. Two sampled error results have later request IDs and observed 2,408 and 1,737 millisecond record intervals. The first rejected a ScheduleWakeup call for a missing prompt; its subsequent assistant content chose automatic notification. It also has 69 exact repeated-command groups across 1,030 Bash calls. These facts establish retry candidates, but not whether a command was necessary or a correction succeeded.

The handoff sample includes the largest retained successor prompt (247,208 characters, cancelled) and a finished ordinary successor (50,949 characters). Both retain rendered session-history context and a persisted successor prompt. Stored renderer source provides `handoff_text`, `render_entry`, and `apply_budget` rules. The historical deployed revision, selected entries, and omitted candidates are absent, so the body cannot reconstruct the original selection.

The tool inventory has repeated and large retained operations. `manual-inspection.json` records two mapped `Read` results of 85,089 and 78,622 characters and mapped `Bash` groups of 1,030 and 617 calls. Its sampled repeated-command group has fingerprint `bf556d72887258b5`, native call IDs, retained results, and a later request ID. The records support unresolved repetition; they do not retain a stable revision or semantic-operation join. The tool table retains unmatched results and representation limits. Stored result characters are not token savings.

## Recommendation

Keep TU-03 unchanged. Rank TU-01 first: its bounded scope is delivery ID, cause, receiving request/turn, and lifecycle outcome; the ScheduleWakeup correction and competing temporal matches show the missing link. Rank TU-02 second: its scope is deployed renderer revision, candidates, selected entries, and prompt inclusion; delivered history bodies show what is available, while selection remains absent. Rank TU-04 third: its scope is request prompt components and tool-result inclusion; repeated native command records and error followups show retry candidates, while no semantic-operation or inclusion identifier remains. Confidence is moderate that each gap exists and low that reducing it saves tokens or time. Each comparison must keep checks and review columns unchanged and compare request categories, completion lead time, retry/review outcomes, and failure-diagnosis quality against matched task types.

The strongest counter-evidence is the existing review history: repeated work may be a distinct revision or correction. No saved transcript demonstrates token savings or faster completion from suppressing a wakeup, reducing a handoff, or truncating a tool result. Preserve required checks and review columns. Stop here for the user decision before any production change.
