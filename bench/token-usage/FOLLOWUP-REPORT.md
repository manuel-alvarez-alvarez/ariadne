# Request-level transcript follow-up

## Question and answer

**Question:** What avoidable token use and elapsed time can saved transcripts establish before new production instrumentation?

Saved Claude transcripts establish per-request usage for mapped sessions and native request, response, turn, and tool IDs. Saved Codex `token_usage_record` rows establish one observed `usage` field per native response ID; cumulative counters and overlapping turn/thread totals remain excluded. They establish observed request spans and tool-result sizes. They do not establish billed prompt composition, active execution time, delivery causes, rendered handoff selection, or causal savings. The baseline remains the canonical cumulative total.

## Reproducible evidence

Cutoff: `2026-10-14T22:17:05.521Z`. Source: `SQLite backup created 2026-10-15`. Source SHA-256: `adfae5cd052310fedd19d3f7b2aa4742c1a6f2a60f546a65de32f7894bc2a06a`. Analyzer revision: `21ae5e699c4bcde8cd772de832a3c13d18669a98`. Transcript manifest: `35718f855c8ba089d8a20bc95ba372b708f14d067016c75bcac44f1dad0c3ebe`. See `followup-snapshot.json`, `format-inventory.csv`, and `followup-reconciliation.json`.

The follow-up found 52,278 unique request records, of which 52,278 map through native session IDs. It discarded 81,531 repeated Claude records and excluded 0 conflicting duplicate native IDs. The CSV artifacts retain no raw transcript text.

## Cohorts, accounting, and timing

`cohort-comparison.csv` separates mapped requests shared with the frozen baseline cutoff from newer evidence and this goal's overhead. These are transcript cohorts, not a before/after experiment. They do not show a saving.

Request input includes cached input. Uncached input equals input minus cached input. `followup-reconciliation.json` compares the request sum with the baseline cumulative session counter without adding them. The difference is expected: formats and mappings are incomplete, while vendor and daemon records can have different scopes.

`session-timeline.csv` measures only first-to-last timestamped model request in a session. It never sums per-request intervals, so parallel requests and waits do not inflate elapsed time. It does not measure model inference, tool runtime, queueing, or human wait.

## Wakeups, handoffs, and tool results

`wakeup-analysis.csv` joins a daemon prompt to the first later retained request only when both share the native Ariadne session ID. This is temporal sequencing, not delivery causation. The database has no delivery ID, cause, outcome link, or rendered-prompt membership, so corrections cannot be separated reliably from repeated deliveries.

Saved handoff text can be inspected locally, but historical rendering revision, candidate set, selection, and prompt inclusion are not retained as structured records. This study therefore does not reconstruct omitted handoff context or claim that later rereading came from a handoff.

`tool-output.csv` links Claude tool calls and results by native tool-call ID. Result characters describe retained text only. They are not billed-token measurements and do not establish that a result was later included in context.

## Recommendation

Keep TU-03 unchanged. Rank TU-01 first: add delivery IDs, causes, and resulting request/outcome links because existing records cannot resolve repeated delivery versus correction. Rank TU-02 second: record handoff candidate and selected lengths, renderer revision, and prompt inclusion because saved text cannot reconstruct selection. Rank TU-04 third: record request prompt components and tool-result inclusion because result size alone does not show token contribution. These are evidence-gap instruments, not production optimizations.

The strongest counter-evidence is the existing review history: repeated work may be a distinct revision or correction. No saved transcript demonstrates token savings or faster completion from suppressing a wakeup, reducing a handoff, or truncating a tool result. Preserve required checks and review columns. Stop here for the user decision before any production change.
