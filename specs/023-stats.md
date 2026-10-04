---
id: stats
status: current
updated: 2026-10-04
areas: [api, store, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/stats/mod.rs
  - crates/ariadne-daemon/tests/it/stats.rs
  - crates/ariadne-daemon/tests/it/switch_stats.rs
  - crates/ariadne-daemon/tests/it/review_stats.rs
  - crates/ariadne-daemon/tests/it/outcome_stats.rs
  - crates/ariadne-daemon/tests/it/stats_work.rs
  - crates/ariadne-daemon/tests/it/stats_time.rs
  - crates/ariadne-daemon/tests/it/stats_spend.rs
  - crates/ariadne-daemon/tests/it/stats_models.rs
  - crates/ariadne-daemon/tests/it/stats_attention.rs
  - crates/ariadne-daemon/tests/it/stats_tools.rs
  - crates/ariadne-daemon/src/http/stats/mod.rs
  - crates/ariadne-daemon/src/acp.rs
  - crates/ariadne-cli/src/commands/stats/mod.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - ui/src/routes/stats.test.tsx
  - ui/src/components/stats/work-section.test.tsx
  - ui/src/components/stats/time-section.test.tsx
  - ui/src/components/stats/spend-section.test.tsx
  - ui/src/components/stats/models-section.test.tsx
  - ui/src/components/stats/attention-section.test.tsx
  - ui/src/components/stats/tools-section.test.tsx
  - ui/src/components/stats/status-colors.test.ts
  - ui/src/events/dispatch.test.ts
  - ui/src/components/app-shell.test.tsx
---

# Stats

What the work did. The daemon writes one fact to a ledger when a thing that
tells performance happens, and every stat is an aggregate of that ledger. The
stats are six families, each the answer to one question a user asks, read
with `GET /v1/stats/<family>`, `ariadne stats <family>` and the Stats screen
of the desktop app.

## Scope

In: the ledger, the rules a fact obeys, every fact kind (`attention`, `session_ended`,
`switch`, `message`, `verdict`, `tool_call`, `permission`, `task_ended` and
`pick`), the filters every family takes, the frame the six families share —
their routes, the command, the screen and its shared pieces, the query keys
and the bucket rule — and each family's own rules.

Out: token accounting itself (012, rules 13 to 16), what a session is and how
it ends (008), what a skill is (017), and the task state machine and the pick
itself, `task_picks` included (004) — the fact is a record of what they
decided, not a change to how they decide it.

## Behavior

### The ledger

1. The ledger is the table `stat_facts`. One row is one fact. The columns are
   `id` (a ULID), `kind`, `created_at`, `repo_id`, `goal_id`, `task_id`,
   `session_id`, `launch_id`, `seat`, `model`, `effort`, `skills` (a JSON
   array of skill names) and `data` (a JSON object). The keys of `data` are
   the kind's own.
2. A fact is appended and never changed. The store gives it its `id` and its
   `created_at` (`Store::record_fact`).
3. A fact has no foreign key. It keeps the ids it was written with after its
   goal, task or session is deleted, so a stat still counts work that is gone.
4. A stats read reads `stat_facts` only. It never scans `agent_events`, which
   holds most of a database that has run for months (012).
5. The table has one index, on `(kind, created_at)`: every aggregate reads one
   kind over a span of time.
6. A fact about a session is filled from that session by one daemon helper,
   `stats::session_fact`. The repository is the task's, or the goal's where
   the goal works in one repository alone; a loose session and an
   orchestrator over several repositories name none. The goal, task, session,
   launch, seat, model and effort are the session's own. The skills are the
   ones its staffed agent loads, in order (`task_agent_skills`); a session
   with no staffed agent has none.

### The `session_ended` fact

7. A session writes one `session_ended` fact each time its row moves to
   `exited` or `failed` and gets a new `ended_at`. Every place in the daemon
   that ends a session calls `stats::record_session_end` after the status
   write. A kill is the one exception: the runtime is still cancelling the
   turn, and the cancelled turn reports its stop and its usage after the kill
   returns. So the fact of a killed run is written once the agent is reaped,
   off the kill's own path. Its row is the one the kill's status write
   answers, read in the same transaction (`Store::set_session_status`), so a
   resume that writes the row next cannot change it. The session's next
   launch waits until that fact is written, the way it waits for an agent
   still ending, so a resumed run adds no turn and no token to it. The
   runtime's own `session_end` comes before that reap, and its fact counts
   the same last turn.
8. The fact is keyed by the launch: the ledger holds at most one
   `session_ended` fact per session and launch. A session restarted under its
   own id runs a new launch, so each run writes its own fact. The same end
   reported twice writes one fact.
9. Its `data` holds `status` (`exited` or `failed`), `attention_reason` (the
   reason the row carries as it ends, or null), `lifetime_secs` (from the
   session's `created_at` to its `ended_at`), `turns` (the count of the
   session's `stop` events), and `input_tokens`, `cached_input_tokens` and
   `output_tokens` (the session's usage, 012 rule 13). The lifetime, the
   turns and the tokens are the session's, so the fact of a later run of a
   restarted session counts the earlier runs too.
10. A fact that cannot be written is logged and goes no further. The session
    still ends.
11. A fact about a session announces that session again (`session_updated`)
    once the fact is committed. The status write that ended the session was
    announced before the fact existed, so stats refetched on that first
    event can miss it; stats refetched on the second hold it. A fact refused
    as a second one for its run announces nothing.

### Filters

12. Every family takes the same two filters: `since` and `repo`. Neither set
    is every fact there is.
13. `since` keeps the facts written at or after a moment. It is an RFC 3339
    moment, or a span back from now: a whole number and a unit, `m`
    (minutes), `h` (hours), `d` (days) or `w` (weeks), such as `24h`, `7d`
    and `30d`. Anything else is refused with `400 invalid_request`, and the
    message names what was sent. One daemon function reads it for every
    family.
14. `repo` keeps the facts whose `repo_id` is that repository id. A
    repository with no facts answers no rows.

### The `switch` fact

15. A session writes one `switch` fact as `Launcher::switch_session` moves it
    off a model, next to the `session.switched` event it already writes
    there, whichever of its two paths — same-row or new-row — carries the
    switch. The fact is of the session that leaves, so its `model` is the
    model left. A same-agent switch writes the fact before the row's pin
    moves, so `model` still names what the session left rather than what
    `set_session_pin` just gave the row.
16. Its `data` holds `to_model` and `to_effort` (the model and effort
    entered), `reason` (the event's own reason: `requested` for a switch
    asked by hand, `exhausted` for one the daemon made of its own accord, or
    whatever else a caller of `switch_session` writes), `automatic` (`true`
    where `reason` is `exhausted`, the one reason the daemon switches a
    session itself), and `same_agent` (whether the conversation carried
    over).
17. The exhaustion sweep's own bookkeeping event, `session.auto_switch` —
    written only for the in-place case, so a later sweep can tell a model
    already tried in the session's chain (009) — writes no fact of its own:
    the switch it marks already has one, from the `switch_session` call that
    wrote it.
18. A fact that cannot be written is logged and goes no further: by the time
    `switch_session` writes it, the pin has moved or the successor and its
    event already exist, and a ledger failure does not undo or refuse a
    switch already under way (rule 10).

### The review facts

19. Every stored task message writes a `message` fact. Its `data` holds
    `kind`, `from_actor` and `to_actor`, and the sender session supplies its
    model and seat.
20. An accepted verdict also writes a `verdict` fact from the reviewer
    session. Its `data` holds `verdict`, `author_model`, `author_session_id`,
    `round` (the author's count of review requests on the task) and
    `latency_secs` (from the latest request to the verdict).

### The tool facts

21. A `tool_call` fact is written when an ACP tool call ends. Its `data` is
    `tool_name` (the agent's own name for the tool: `/name`,
    `/_meta/claudeCode/toolName`, `/title`, `/toolCallId`, in order — the same
    lookup and the same order the learned-permission key names the tool by),
    `kind` (the ACP kind of the call — `read`, `edit`, `delete`, `move`,
    `search`, `execute`, `think`, `fetch`, `switch_mode` or `other` — `other`
    where the call names none), `duration_ms`, and `ok`; a failed ACP status
    writes `ok = false`. The runtime keeps the opening instant with each open
    call and spawns the `session_fact` and ledger write, so neither a tool
    update nor a turn waits for SQLite. The `tool_name` the `pre_tool_use` and
    `post_tool_use` event payloads carry is unchanged by this: it stays the
    call's title, the console's and the transcript's own name for it.
22. A `permission` fact is written after each ACP permission reply. Its
    `data` is `tool_name`, `decided_by`, `answer` (`allow`, `deny`, or
    `cancelled`), `console_option_id`, and `wait_ms`.

### The `task_ended` fact

23. A task writes one `task_ended` fact each time it reaches `finished`,
    `cancelled` or `failed`. It is written in the same transaction as the
    status write, inside `Store::transition_task`'s shared body
    (`transition_in_tx`) — the one place every status change passes — rather
    than from the daemon once the call returns: that way no caller of
    `transition_task` can reach one of the three endings without the fact
    being written alongside it. A task retried and failed again writes one
    more fact.
24. Its `data` holds `status` (`finished`, `cancelled` or `failed`),
    `reason` (the transition's own reason), `landing` (the task's own, as
    005 spells it), `lead_time_secs` (from the task's `created_at` to the
    transition), `review_requests` (the count of `review_request` messages
    on the task) and `authors` (the count of author agents staffed), and
    `picked` (true where `picked_agent_id` is set), and `status_secs` (an
    object with the seconds in `pending`, `ready`, `in_progress`,
    `under_review`, `changes_requested` and `approved`, summed across the
    task's whole life from its transitions). The ending status has no time.
    The repository, the
    goal and the task are the task's own; there is no session to one.
25. The model, the effort and the skills are the picked author's where the
    task staffed several authors, the one author's where it staffed a
    single one, and none of the three where it staffed several and none
    was picked yet — a task cancelled or failed before its pick settled.

### The `pick` fact

26. A contested task's settled pick writes one `pick` fact, inside
    `Store::set_task_picked` itself (004 rule 14) rather than from the
    daemon once that call returns: the winner's column and this fact are
    one transaction, so there is no moment where a daemon could die with
    the winner written and the fact not. Its `data` holds `winner_model`,
    `loser_models` (the losing authors' models, as an array) and
    `reviewers` (the count of reviewers staffed on the task).
27. The repository and the goal are the task's own, and the model, the
    effort and the skills are the winning `task_agents` row's own
    (`task_agent_skills`) rather than a session's: a session can end, be
    resumed under another one, or never have existed, none of which should
    stand between the winner being on the task and the fact that says who
    lost to it. Two authors of one contest sharing a model are still one
    row each, since the fact names the model, not the agent. `session_id`
    and `launch_id` are the winner's live session where `run_the_pick`
    found one, carried straight through to the fact; absent where it found
    none.
28. Each settled contest calls `set_task_picked` once, so each one writes
    one fact of its own: a task retried clears the winner
    (`Store::clear_task_picks`) and runs its review again, and the next
    settlement's fact is its own rather than a second one of a task
    already named.

### The frame

29. The stats are six families. Each answers one question a user asks, and
    the screen, the command and the docs show them in this order: `work`
    (what got done?), `time` (how long does it take?), `spend` (what did it
    spend?), `models` (which model does the job?), `attention` (how much did
    it need me?) and `tools` (what do the agents do?).
30. `GET /v1/stats/<family>` answers the family's own DTO: `WorkStatsDto`,
    `TimeStatsDto`, `SpendStatsDto`, `ModelStatsDto`, `AttentionStatsDto` and
    `ToolStatsDto`. Every route takes the filters of rules 12 to 14 and sits
    under the `stats` tag of the API document. No other route is under
    `/v1/stats/`.
31. Each family is one file in each layer: `stats/<family>.rs` in the
    store (`Store::<family>_stats`), the API and the daemon, where the file
    owns its handler and its part of the API document;
    `commands/stats/<family>.rs` in the CLI; and `<family>-section.tsx` in
    the desktop app. The `mod.rs` beside each registers the six once and
    holds what they share.
32. A median averages the two middle values of an even count, and a p90 is
    the nearest-rank value. Both are 0 over no values.
33. A family that draws a time axis buckets its facts by day where `since`
    is 31 days or less back from now, and by week where it is further back
    or absent (`Bucket::for_span`). A fact falls in the bucket that starts at
    midnight UTC of its day, or of the Monday of its week
    (`Bucket::start_of`), written as an RFC 3339 moment.
34. `ariadne stats [work|time|spend|models|attention|tools] [--since
    <duration|date>] [--repo <id>]` prints a family. `ariadne stats` alone
    prints `work`. `--repo` takes an id or a unique prefix of one (014).
    `--since` is sent to the daemon as it was written. Each of the six is a
    listing: it takes the table flags (014), and `--format json` prints the
    DTO whole. The table of a family that holds nothing to show prints one
    muted sentence.
35. The desktop app has a Stats screen at `#/stats`, titled `Stats`, last in
    the sidebar. Its header holds a `since` selector (all time, 24 hours, 7
    days, 30 days) and a repository selector. Both live in the URL
    (`?since=7d&repo=<id>`), and the screen hands them to every section as
    one `{ since, repo }`.
36. The screen renders six sections in the order of rule 29: `WorkSection`,
    `TimeSection`, `SpendSection`, `ModelsSection`, `AttentionSection` and
    `ToolsSection`. Each draws through the shared `StatSection`: its heading
    (`text-sm font-medium`), one sentence of the question it answers, the
    read's error or skeleton, and the one muted sentence of an empty family.
    The other shared pieces are `StatTiles` and `StatTile` (a label, a value
    and an optional hint), `StatTable` (a compact comparison table, numbers
    right-aligned, columns given by the caller), `StatTimeChart` (stacked
    bars per bucket on a date axis) and `StatBarChart` (a horizontal bar per
    row). Every chart keeps an `sr-only` table of the same numbers under it,
    for a screen reader and for a test. A colour carries one meaning, off the
    status ramp in `index.css` through the shared `STATUS_COLORS` module —
    finished or ended on `status-done`, failed on `status-danger`, stalled on
    `status-warn`, cancelled on `status-pending`, a neutral count on
    `status-active` — and each chart's series config lives in
    `chart-configs.ts`.
37. Each section reads its family under `qk.stats.<family>(filter)`, in
    the query-key group `stats`. Every `task_updated`, `session_updated` and
    `goal_updated` event invalidates the whole group (`qk.stats.all()`),
    since any of the three may be a fact.

### Work

What got done: goals completed, tasks finished, failed and cancelled, and
changes landed, over time.

38. A goal writes one `goal_ended` fact each time it moves to `completed` or
    `cancelled`. It is written inside `Store::set_goal_status`, in the same
    transaction as the status write, the way `task_ended` is written inside
    `transition_in_tx` (rule 23) — the one place every goal status change
    passes. A move to the same status writes no fact.
39. Its `data` holds `status` (`completed` or `cancelled`), `lead_time_secs`
    (from the goal's `created_at` to the move), `tasks` (the count of its
    tasks), `tasks_finished` (those `finished`) and `landing` (the goal's
    own). `goal_id` is the goal's; `repo_id` is the goal's one repository, or
    null where it works in several; `model` and `effort` are the goal's
    orchestrator pin.
40. `work_stats(filter) -> WorkStats { totals, bucket, buckets }` answers what
    got done. `totals` holds `goals_completed`, `goals_cancelled`,
    `median_goal_lead_time_secs` (over completed goals), `tasks_finished`,
    `tasks_failed`, `tasks_cancelled`, `finish_rate` (`tasks_finished` over
    the three, 0 where there are none) and `landed` (`task_ended` facts
    `finished` whose `landing` is `merge` or `pull_request`). `bucket` is
    `Bucket::for_span(since)` (rule 33); `buckets` is one row per bucket from
    the first fact to the last, zeros included, each with `start`,
    `tasks_finished`, `tasks_failed`, `tasks_cancelled`, `goals_completed`
    and `landed`. `since` and `repo_id` narrow every count, as rule 12 says.
41. `GET /v1/stats/work` answers `WorkStatsDto`, `work_stats`'s own shape.
    `ariadne stats work` prints the totals as `label: value` lines, then a
    table of the buckets, `FROM`, `FINISHED`, `FAILED`, `CANCELLED`, `GOALS`
    and `LANDED`; `--format json` prints the DTO whole. `WorkSection` draws
    `StatTiles` of tasks finished, goals completed, changes landed, finish
    rate and median goal lead time, and one `StatTimeChart` of tasks per
    bucket, stacked `finished`, `failed` and `cancelled` on `STATUS_COLORS`,
    with goals completed and landed carried in its tooltip and its `sr-only`
    table alongside the stacked counts.

### Time

38. `time_stats` reads `task_ended` facts whose status is `finished`. It
    answers their count, median, nearest-rank p90 and mean lead time; a
    missing value is zero. Its status rows follow the lifecycle order:
    `pending`, `ready`, `in_progress`, `under_review`, `changes_requested`
    and `approved`. Each gives total time, median time and its share of the
    status total. A fact without `status_secs` contributes no status time.
39. The family also counts `permission` facts where `decided_by` is `console`:
    the value the daemon writes for a person who answered. It answers their
    prompt count, total seconds and median seconds from `wait_ms`. Both the
    task and permission reads honour `since` and `repo`.
40. `ariadne stats time` prints the lead-time and person-waiting figures as
    label and value lines, then `STATUS`, `TOTAL`, `MEDIAN` and `SHARE` rows.
    The desktop section shows median and p90 lead time, person wait time and
    prompt count, then a "Where the time goes" status bar chart with each
    row's median and share in its tooltip.

### Spend

Built by its task.

### Models

`Store::model_stats` reads only filtered `stat_facts` and returns `ModelStats { items }`.
`GET /v1/stats/models` returns the same fields through `ModelStatsDto` and the shared `StatsQuery`.
Each named model gets one row per seat, including models named only inside verdicts or picks.
Rows sort by orchestrator, author, reviewer, then no seat, and by model within each seat.
A fact without a model contributes no session row.
Both filters apply before any aggregation or task matching.

Each row counts ended sessions, failed sessions, sessions whose attention reason is `stalled`, and switches with reason `exhausted`.
Switches count against the model and seat left.
Usage sums input, cached input, and output tokens from ended sessions.
Cached share is cached input tokens divided by input tokens.
Mean lifetime is summed session lifetime divided by ended sessions.

Author rows carry an `author` object; other rows carry null.
Finished, failed, and cancelled task counts count `task_ended` facts, including repeated endings.
Finish rate is finished endings divided by all three ending counts.
First-pass rate divides tasks with a round-one approval by tasks with any verdict, grouped by author model.
An approval from any reviewer qualifies; repeated verdicts count the task once.
Mean review rounds is the mean `review_requests` across this model's task endings.
Each pick counts one contest entered per distinct model named among its winner and losers.
Each pick counts one win for its winner model, even when that model also appears among losers.
Win rate is contests won divided by contests entered.
Tokens per finished task averages matching author-session input plus output over distinct finished tasks attributed to this model.
Matching requires the same model and task, and finished tasks without sessions contribute zero tokens.
Cached tokens are already part of input and are never added again.

Reviewer rows carry a `reviewer` object; other rows carry null.
Verdicts count each verdict fact from that reviewer model.
Approve share is approval verdicts divided by all verdicts.
Mean latency is summed verdict latency divided by verdicts.
Every share and mean with a zero denominator is zero.

The CLI prints one comparison table per seat, headed by the seat.
`--seat orchestrator|author|reviewer` belongs only to `stats models` and limits table output.
JSON output always contains the complete DTO, regardless of `--seat`.
A seatless row uses session columns under `NONE` in the CLI and remains available in JSON.
The desktop section draws Authors, Reviewers, and Orchestrators through `StatTable`, omitting empty seats.
Responses without rows for the three displayed seats use the shared empty state, including responses with only seatless rows.
Each desktop table sorts by its first count descending, then by model.

| Seat | Columns |
| --- | --- |
| Author | MODEL, TASKS, FINISH_RATE, FIRST_PASS, ROUNDS, WIN_RATE, TOKENS/TASK, FAILED, EXHAUSTED |
| Reviewer | MODEL, VERDICTS, APPROVE, LATENCY, FAILED, EXHAUSTED |
| Orchestrator | MODEL, SESSIONS, TOKENS, LIFETIME, FAILED, EXHAUSTED |

TASKS is the sum of finished, failed, and cancelled endings; FAILED always means failed sessions.
Rates print as percentages with one decimal, durations use the existing duration formatter, and tokens use compact notation.

### Attention

An `attention` fact is written by the store whenever a clear takes a session
flag down. Its data holds the reason taken down and the whole seconds since
`attention_since`. A clear that changes no row writes no fact. A flag still up
when a session ends writes no attention fact; the `session_ended` fact carries
its reason.

The family groups permission facts by `decided_by` and answer, reports the
share answered by `console`, and reports each supported flag reason from
clears plus ending sessions that carried it. It also counts failed sessions,
stalled ending sessions and exhausted switches. Every count and mean obeys the
shared `since` and `repo` filter.

### Tools

38. `Store::tools_stats(filter, limit)` answers `ToolStats`: `calls` and
    `errors`, the totals over the facts the filter keeps; `tools`, the count
    of distinct tool names; `by_kind`, one row per kind with `calls`,
    `errors`, `median_duration_ms` and `p90_duration_ms`, the most calls
    first; `top`, the `limit` tools with the most calls, each the same four
    figures plus its `tool_name` and `kind`; and `other`, the rest of the
    tools summed into `tools`, `calls` and `errors`. A tool is grouped by its
    `tool_name` alone, never by `(tool_name, kind)`: `tools` and `top` count
    and rank distinct names, so a stable name whose calls carry more than
    one kind is still one tool, and its row's own `kind` is whichever one
    most of its calls carry, the lowest name breaking a tie. A `tool_call`
    fact with no `kind` predates rule 21's stable name and is not counted.
    The median and the p90 are rule 32's, read over each group's own
    `duration_ms`.
39. `ToolStatsDto` mirrors the aggregate. `GET /v1/stats/tools` takes
    `since` and `repo` (rules 12 to 14) and `limit` — 1 to 100, 10 where it
    is absent, and `400 invalid_request` for anything else — in its own
    query type, `ToolsStatsQuery`, rather than the shared `StatsQuery` every
    other family's route takes.
40. `ariadne stats tools [--limit <n>]` prints the totals as `calls`,
    `errors` and `tools` lines, a table of `by_kind` (`KIND`, `CALLS`,
    `ERRORS`, `MEDIAN`, `P90`, most calls first), and a table of `top`
    (`TOOL`, `KIND`, `CALLS`, `ERRORS`, `MEDIAN`, `P90`) with an `other` row
    last where there is one. `--limit` is the `tools` subcommand's own flag,
    not one of rule 34's shared filters. `--format json` prints the DTO
    whole.
41. `ToolsSection` draws `StatTiles` of `calls`, `errors`, the error rate and
    the distinct tool count, then one `StatBarChart` of `by_kind` and one of
    `top` with its `other` row last, each stacking a call's `ok` and
    `errors` count, the median and the p90 in the tooltip. Neither chart
    draws more than the section's own limit plus one rows, whatever the
    answer holds.

## Acceptance criteria

#### The ledger

- Deleting the goal keeps the fact
  (`store.rs::a_fact_outlives_the_goal_it_is_about`), and a `task_ended`
  fact outlives the goal it is about
  (`store.rs::an_outcome_fact_outlives_the_goal_it_is_about`).
- A fact is written once per launch
  (`store.rs::a_fact_is_recorded_once_per_launch`).
- A fact announces its session once it is readable, and a refused one
  announces nothing
  (`store.rs::a_fact_announces_its_session_once_it_is_readable`).
- A session that ends writes one `session_ended` fact with its launch, seat,
  model, skills, status, turns and tokens, and an end reported twice writes
  no second one (`stats.rs::a_session_that_ends_writes_one_session_ended_fact`).
- A restarted session writes a fact for each run
  (`stats.rs::a_restarted_session_writes_a_fact_for_each_run`).
- A turn killed while it runs counts in its run's fact: its stop and its
  usage (`stats.rs::a_killed_turn_counts_in_its_fact`).
- A resume right after a kill leaves the killed launch one fact with only
  its own turn and tokens
  (`stats.rs::a_resume_after_a_kill_leaves_the_killed_run_its_own_fact`); the
  next launch waits for the kill's gate
  (`acp.rs::tests::a_launch_waits_for_the_gate_of_a_kill`); and a session's
  end answers the row its write left
  (`store.rs::ending_a_session_answers_the_row_its_write_left`).
- A manual switch writes one `switch` fact naming the model left and the
  model entered, `reason=requested` and `automatic=false`
  (`switch_stats.rs::a_manual_switch_writes_one_switch_fact_with_the_model_left_and_entered`),
  and a same-agent switch writes its fact before the pin moves, so the model
  named is the one left
  (`switch_stats.rs::a_same_agent_switch_writes_the_fact_before_the_pin_moves`).
- An exhausted session that auto-switches writes one `switch` fact,
  `reason=exhausted` and `automatic=true`
  (`switch_stats.rs::an_exhausted_session_that_auto_switches_writes_one_switch_fact`).
- Verdict facts count review requests as rounds, and message facts record
  each stored message
  (`review_stats.rs::verdicts_record_rounds_and_messages_record_each_message`).
- A tool completion and a console permission answer write their facts
  (`stats.rs::a_tool_call_and_console_permission_write_their_facts`).
- A task that finishes writes one `task_ended` fact with its status, its
  author's model and a lead time
  (`outcome_stats.rs::a_finished_task_writes_one_task_ended_fact`), and a task
  retried after it fails, and that fails again, writes a fact for each ending
  (`outcome_stats.rs::a_retried_task_that_fails_again_writes_two_facts`).
- A two-author task writes one `pick` fact with the winner's and the loser's
  models (`outcome_stats.rs::a_contested_pick_writes_one_pick_fact`), and a
  task retried writes a `pick` fact for each settled cycle, not just the
  first
  (`store.rs::a_retried_contest_writes_a_pick_fact_for_each_settled_cycle`).

#### The frame

- `since` and `repo_id` each add a clause, their values bound in order
  (`stats/mod.rs::tests::the_filter_narrows_by_moment_and_repository`).
- A span back from now is a moment, a moment is taken as written, and
  anything else is refused
  (`http/stats/mod.rs::tests::a_span_counts_back_from_now`,
  `::a_moment_is_taken_as_written`, `::anything_else_is_refused`); a bad
  `since` is a 400 naming it (`stats.rs::a_bad_since_is_refused`).
- The median averages the two middle values, and the p90 is the
  nearest-rank value
  (`stats/mod.rs::tests::the_median_averages_the_two_middle_values`,
  `::the_p90_is_the_nearest_rank_value`).
- A span of 31 days or less buckets by day, a longer one by week, and a fact
  falls in its day or in its week from Monday
  (`stats/mod.rs::tests::a_short_span_is_bucketed_by_day_and_a_long_one_by_week`,
  `::a_fact_falls_in_its_day_or_its_week_from_monday`).
- Each `GET /v1/stats/<family>` answers 200 with its DTO, and is in the API
  document under the `stats` tag
  (`stats_<family>.rs::the_<family>_stat_answers_and_is_in_the_api_document`,
  one per family); the reviews, switches and outcomes routes answer 404
  (`stats.rs::the_old_families_are_gone`).
- Each `ariadne stats <family> --format json` reads its own route with the
  filters given
  (`commands/stats/mod.rs::tests::each_family_reads_its_route_with_the_filters_given`),
  and `ariadne stats` alone runs `work`
  (`commands/stats/mod.rs::tests::stats_alone_runs_work`,
  `cli/tests.rs::stats_alone_runs_work_and_takes_the_filters_either_side`).
- The six are classified and take the table flags of a listing
  (`cli/tests.rs::every_command_in_the_tree_is_classified`,
  `::the_listing_flags_are_advertised_exactly_where_they_are_honored`).
- The Stats screen renders the six sections in order under one heading
  style (`stats.test.tsx` "renders the six sections in order, under one
  heading style"), asks every family with the filters in its URL under the
  key `qk` names (`stats.test.tsx` "asks every family with the filters in
  its URL, under the key qk names"), and renders an error rather than the
  empty sentence when a read fails (`stats.test.tsx` "renders an error, not
  the empty sentence, when the daemon refuses a read").
- Each section asks for its family with the filter under
  `qk.stats.<family>(filter)`, and says its empty sentence under its heading
  and its question (`<family>-section.test.tsx` "asks for its family with
  the filter, and says its empty sentence", one per family).
- A task or a session update invalidates the stats
  (`dispatch.test.ts` "refetches every stat when a task or a session moves").
- Stats is the last entry of the sidebar
  (`app-shell.test.tsx` "ends the navigation with stats, right after
  repositories and permissions").
- `STATUS_COLORS` maps each meaning to the status ramp's own CSS variable
  (`status-colors.test.ts` "maps each meaning to the status ramp's own CSS
  variable, so every panel's chart draws it the same").

#### Work

- A goal completed writes one `goal_ended` fact with its status, lead time,
  task counts and landing; a goal cancelled writes one too, and a second
  move to the same status writes no second one
  (`stats_work.rs::a_completed_goal_writes_one_goal_ended_fact`,
  `::a_cancelled_goal_writes_one_goal_ended_fact`).
- `work_stats` counts tasks finished, failed and cancelled, goals completed
  and changes landed, buckets them by day under a short `since` and by week
  with none, zeros included, and honours `since` and `repo_id`
  (`stats/work.rs::tests::work_stats_counts_tasks_finished_failed_cancelled_and_goals_and_landed`,
  `::work_stats_buckets_by_day_under_a_short_since_and_by_week_with_none`,
  `::work_stats_honours_since_and_repo_id`).
- `GET /v1/stats/work` answers the totals and the buckets
  (`stats_work.rs::the_work_stat_answers_the_totals_and_the_buckets`).
- `ariadne stats work --format json` prints the DTO, and the table prints the
  totals and a row per bucket
  (`commands/stats/work.rs::tests::the_totals_print_as_label_value_lines_in_field_order`,
  `::a_bucket_row_carries_every_count_in_column_order`).
- `WorkSection` draws the tiles and the chart from a mocked response under
  `qk.stats.work`, and says its empty sentence where there is nothing to show
  (`work-section.test.tsx` "asks for its family with the filter, and says its
  empty sentence", "draws the tiles and the chart from what the daemon
  answers").
- A `goal_updated` event invalidates the stats too (`dispatch.test.ts`
  "refetches every stat when a task, a session or a goal moves, since any
  may be a fact").

#### Time

- A finished task records time in every status it passed
  (`outcome_stats.rs::a_finished_task_writes_one_task_ended_fact`).
- The aggregate reports lead time, status shares and person waits and filters
  by `since` and repository
  (`stats/time.rs::tests::time_stats_measures_finished_tasks_statuses_and_person_waits`).
- The route reads a finished task into the time DTO
  (`stats_time.rs::the_time_stat_answers_and_is_in_the_api_document`).
- The desktop section renders its tiles and status chart from `qk.stats.time`
  (`time-section.test.tsx::asks_for_its_family_with_the_filter_and_draws_its_time_figures`).

#### Spend

Built by its task.

#### Models

- Author sessions, endings, verdicts, and picks produce every author figure, with duplicate contest models counted once
  (`stats/models.rs::tests::author_figures_combine_sessions_outcomes_reviews_and_contests`).
- Each verdict contributes to reviewer count, approval share, and mean latency
  (`stats/models.rs::tests::reviewer_figures_count_each_verdict_and_its_latency`).
- Every fact and the session-to-task match obey both filters, including the exact time boundary
  (`stats/models.rs::tests::every_fact_is_filtered_by_time_and_repository`).
- Payload-only models get rows; seat ordering, absent seats, and empty denominators hold
  (`stats/models.rs::tests::rows_include_models_named_only_in_payloads_and_sort_by_seat_then_model`).
- Repeated verdicts and finished endings preserve task-based denominators
  (`stats/models.rs::tests::first_pass_counts_reviewed_tasks_once`,
  `::finished_task_tokens_count_each_task_once_and_exclude_other_seats`).
- The route returns author and reviewer figures for a finished task with an approval
  (`stats_models.rs::a_finished_task_and_approval_answer_author_and_reviewer_rows`).
- CLI tables group each seat, and only the models command accepts `--seat`
  (`commands/stats/models.rs::tests::tables_group_models_by_seat_and_format_the_figures`,
  `::the_seat_option_belongs_only_to_models_and_prints_one_table`).
- JSON preserves the complete DTO with a seat selected
  (`commands/stats/models.rs::tests::json_reads_the_complete_dto_even_with_a_seat_selected`).
- The desktop reads under `qk.stats.models`, draws three sorted and formatted tables, and omits empty seats
  (`models-section.test.tsx`: "draws three seat tables with formatted figures and count ordering",
  "omits a table when its seat has no rows", and the section's empty-state test).

- A response containing only seatless rows shows the shared empty state and no table
  (`models-section.test.tsx`: "shows the empty state when the response contains only seatless rows").

#### Attention

- Agent-event, idle-report and explicit clears write one attention fact only
  when they remove a flag (`store.rs::attention_clears_write_facts_only_when_a_flag_falls`).
- The aggregate groups permission answers, flag reasons, failures, stalls and
  exhausted switches under both filters (`stats/attention.rs`).
- The route, command and screen expose the family, including its decider and
  flag charts (`stats_attention.rs`, `commands/stats/attention.rs`,
  `attention-section.test.tsx`).

#### Tools

- A tool call named by `/name` or by `/_meta/claudeCode/toolName` writes its
  `tool_call` fact under that name, with its ACP `kind`; a call with a title
  alone is named by the title
  (`stats_tools.rs::a_tool_call_fact_is_named_by_its_stable_name_and_kind`).
- `tools_stats` groups by kind and by tool, keeps the top `limit` tools and
  sums the rest into `other`, skips a fact with no `kind`, measures the
  median and the p90, and keeps only the facts `since` and `repo_id` select
  (`store.rs::tool_stats_group_by_kind_and_tool_and_measure_percentiles`).
- A stable name whose calls carry more than one kind is still one tool, its
  row summing every kind's calls under the one most of them carry
  (`store.rs::a_tool_named_once_under_more_than_one_kind_is_still_one_tool`).
- `GET /v1/stats/tools?limit=1` answers one top tool and the rest in
  `other`, and a `limit` outside 1 to 100 is refused with a 400 naming it
  (`stats_tools.rs::limit_keeps_the_top_tools_and_sums_the_rest_into_other`,
  `::a_limit_outside_1_to_100_is_refused`).
- `ariadne stats tools --format json` reads the DTO off the route, and the
  table prints a row per kind and the top tools with an `other` row
  (`commands/stats/tools.rs::tests::json_prints_the_dto_and_the_table_groups_its_rows`).
- `ToolsSection` draws the tiles and both charts from an answer with more
  tools than the limit it draws, and never draws more than the limit plus
  one rows (`tools-section.test.tsx` "draws the tiles and both charts, never
  more than the limit plus one rows").

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/src/stats/`,
`crates/ariadne-store/src/tasks.rs`,
`crates/ariadne-store/src/picks.rs`,
`crates/ariadne-store/src/events.rs`,
`crates/ariadne-api/src/stats/`,
`crates/ariadne-daemon/src/stats.rs`,
`crates/ariadne-daemon/src/http/stats/`,
`crates/ariadne-daemon/src/http/landing.rs`,
`crates/ariadne-daemon/src/http/events.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/scheduler/goals.rs`,
`crates/ariadne-daemon/src/scheduler/sweeps.rs`,
`crates/ariadne-daemon/src/scheduler/auto_switch.rs`,
`crates/ariadne-daemon/src/scheduler/tasks.rs`,
`crates/ariadne-cli/src/commands/stats/`,
`ui/src/routes/stats.tsx`,
`ui/src/components/stats/`,
`ui/src/api/query-keys.ts`,
`ui/src/events/dispatch.ts`.
