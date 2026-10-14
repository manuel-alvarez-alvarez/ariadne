# Request-level transcript follow-up

## Question and answer

**Question:** What avoidable token use and elapsed time can saved transcripts establish before new production instrumentation?

Saved Claude transcripts establish per-request usage for mapped sessions and native request, response, turn, and tool IDs. Saved Codex `token_usage_record` rows establish one observed `usage` field per native response ID; cumulative counters and overlapping turn/thread totals remain excluded. They establish observed request spans and tool-result sizes. They do not establish billed prompt composition, active execution time, delivery causes, rendered handoff selection, or causal savings. The baseline remains the canonical cumulative total.

## Reproducible evidence

Cutoff: `2026-10-14T22:17:05.521Z`. Source: `SQLite backup created 2026-10-15` at `/tmp/token-followup.db`. Source SHA-256: `adfae5cd052310fedd19d3f7b2aa4742c1a6f2a60f546a65de32f7894bc2a06a`. Source revision: `32d004ef0d543ed82b7ef4447f3b577568b76f9f`. Analyzer SHA-256: `b7236ab119135b961da3fbb2f6bd6208e40bc5944c9c35691097f731939f8f61`. `followup-transcript-manifest.csv` has a per-source retained-content hash; it does not hash records after the cutoff.

The follow-up found 48,769 unique request records, of which 48,769 map through native session IDs. It discarded 81,531 repeated Claude records and excluded 0 conflicting duplicate native IDs. The CSV artifacts retain no raw transcript text.

## Cohorts, accounting, and timing

`cohort-comparison.csv` uses the supplied frozen SQLite snapshot to intersect native `agent_sessions.id` values. It separates those shared identities, new sessions, and this goal by explicit goal ID. These transcript cohorts are not a before/after experiment.

Request input includes cached input. Uncached input equals input minus cached input. `request-reconciliation.csv` compares each mapped session with its retained session counters and records residuals. `followup-reconciliation.json` names the frozen baseline canonical total and fallback rule separately from the current snapshot; neither total replaces the other.

`session-timeline.csv` measures only first-to-last timestamped model request in a session. It never sums per-request intervals, so parallel requests and waits do not inflate elapsed time. It does not measure model inference, tool runtime, queueing, or human wait.

## Wakeups, handoffs, and tool results

`wakeup-analysis.csv` joins a daemon prompt to subsequent mapped requests in the same session. It records a temporal candidate and not a causal delivery join: events retain no vendor request ID, delivery cause, or outcome correlation. Session/task status and ending facts remain lifecycle outcomes, not request-start timestamps.

`handoff-analysis.csv` inventories each native `switched_from` predecessor-successor pair. Stored events can support a renderer inspection, but the deployed renderer revision and selected-entry set are not retained together; each row therefore states that uncertainty separately. This study does not claim omitted context or later rereading.

`tool-output.csv` links Claude and Codex tool calls/results by native tool-call ID. It separates mapped requests, unmatched calls, and unavailable request associations. Result characters describe retained text only; arrays of text blocks are measured and unsupported shapes are labelled. They are not billed-token measurements or proof of later prompt inclusion.

## Recommendation

Keep TU-03 unchanged. Rank TU-01 first: record delivery IDs, causes, and resulting request/outcome links. Rank TU-02 second: record deployed renderer revision, candidates, selected entries, and prompt inclusion. Rank TU-04 third: record prompt components and tool-result inclusion. The evidence identifies these residual gaps, but it does not show that suppressing any item saves tokens or time. A controlled comparison must keep checks and review columns unchanged and compare tokens, completion lead time, retry/review outcomes, and failure diagnosis quality.

The strongest counter-evidence is the existing review history: repeated work may be a distinct revision or correction. No saved transcript demonstrates token savings or faster completion from suppressing a wakeup, reducing a handoff, or truncating a tool result. Preserve required checks and review columns. Stop here for the user decision before any production change.
