---
id: stats
status: current
updated: 2026-10-06
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
  - crates/ariadne-daemon/src/http/stats/mod.rs
  - crates/ariadne-daemon/src/acp.rs
  - crates/ariadne-cli/src/commands/stats/mod.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - ui/src/routes/stats.test.tsx
  - ui/src/components/stats/work-section.test.tsx
  - ui/src/components/stats/time-section.test.tsx
  - ui/src/components/stats/spend-section.test.tsx
  - ui/src/components/stats/stat-time-chart.test.tsx
  - ui/src/components/stats/models-section.test.tsx
  - ui/src/components/stats/attention-section.test.tsx
  - ui/src/components/stats/status-colors.test.ts
  - ui/src/components/stats/stat-explain.test.tsx
  - ui/src/components/stats/stat-chart-legend.test.tsx
  - ui/src/events/dispatch.test.ts
  - ui/src/components/app-shell.test.tsx
---

# Stats

What the work did. The daemon writes one fact to a ledger when a thing that
tells performance happens, and every stat is an aggregate of that ledger. The
stats are five families, each the answer to one question a user asks, read
with `GET /v1/stats/<family>`, `ariadne stats <family>` and the Stats screen
of the desktop app.

## Scope

In: the ledger, the rules a fact obeys, every fact kind (`attention`, `session_ended`,
`switch`, `message`, `verdict`, `permission`, `task_ended` and
`pick`), the filters every family takes, the frame the five families share —
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

### The permission facts

21. A `permission` fact is written after each ACP permission reply. Its
    `data` is `tool_name`, `decided_by`, `answer` (`allow`, `deny`, or
    `cancelled`), `console_option_id`, and `wait_ms`.

### The `task_ended` fact

22. A task writes one `task_ended` fact each time it reaches `finished`,
    `cancelled` or `failed`. It is written in the same transaction as the
    status write, inside `Store::transition_task`'s shared body
    (`transition_in_tx`) — the one place every status change passes — rather
    than from the daemon once the call returns: that way no caller of
    `transition_task` can reach one of the three endings without the fact
    being written alongside it. A task retried and failed again writes one
    more fact.
23. Its `data` holds `status` (`finished`, `cancelled` or `failed`),
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
24. The model, the effort and the skills are the picked author's where the
    task staffed several authors, the one author's where it staffed a
    single one, and none of the three where it staffed several and none
    was picked yet — a task cancelled or failed before its pick settled.

### The `pick` fact

25. A contested task's settled pick writes one `pick` fact, inside
    `Store::set_task_picked` itself (004 rule 14) rather than from the
    daemon once that call returns: the winner's column and this fact are
    one transaction, so there is no moment where a daemon could die with
    the winner written and the fact not. Its `data` holds `winner_model`,
    `loser_models` (the losing authors' models, as an array) and
    `reviewers` (the count of reviewers staffed on the task).
26. The repository and the goal are the task's own, and the model, the
    effort and the skills are the winning `task_agents` row's own
    (`task_agent_skills`) rather than a session's: a session can end, be
    resumed under another one, or never have existed, none of which should
    stand between the winner being on the task and the fact that says who
    lost to it. Two authors of one contest sharing a model are still one
    row each, since the fact names the model, not the agent. `session_id`
    and `launch_id` are the winner's live session where `run_the_pick`
    found one, carried straight through to the fact; absent where it found
    none.
27. Each settled contest calls `set_task_picked` once, so each one writes
    one fact of its own: a task retried clears the winner
    (`Store::clear_task_picks`) and runs its review again, and the next
    settlement's fact is its own rather than a second one of a task
    already named.

### The frame

28. The stats are five families. Each answers one question a user asks, and
    the command and the docs show them in this order: `work` (what got
    done?), `time` (how long does it take?), `spend` (what did it spend?),
    `models` (which model does the job?) and `attention` (how much did it
    need me?). The desktop screen leads with `models` instead, ahead of
    `work` (rule 35).
29. `GET /v1/stats/<family>` answers the family's own DTO: `WorkStatsDto`,
    `TimeStatsDto`, `SpendStatsDto`, `ModelStatsDto` and `AttentionStatsDto`.
    Every route takes the filters of rules 12 to 14 and sits
    under the `stats` tag of the API document. No other route is under
    `/v1/stats/`.
30. Each family is one file in each layer: `stats/<family>.rs` in the
    store (`Store::<family>_stats`), the API and the daemon, where the file
    owns its handler and its part of the API document;
    `commands/stats/<family>.rs` in the CLI; and `<family>-section.tsx` in
    the desktop app. The `mod.rs` beside each registers the five once and
    holds what they share.
31. A median averages the two middle values of an even count, and a p90 is
    the nearest-rank value. Both are 0 over no values.
32. A family that draws a time axis buckets its facts over the span `since`
    to now where the filter gives one, or the first kept fact to now where
    it does not: by the hour where that span is 2 days or less, by the day
    where it is 31 days or less, and by the week where it is longer
    (`Bucket::for_span`). A fact falls in the bucket that starts at the top
    of its hour, at midnight UTC of its day, or at midnight UTC of the
    Monday of its week (`Bucket::start_of`), written as an RFC 3339 moment.
    The family's own buckets run that whole span, zeros included for a
    bucket with no fact in it.
33. `ariadne stats [work|time|spend|models|attention] [--since
    <duration|date>] [--repo <id>]` prints a family. `ariadne stats` alone
    prints `work`. `--repo` takes an id or a unique prefix of one (014).
    `--since` is sent to the daemon as it was written. Each of the five is a
    listing: it takes the table flags (014), and `--format json` prints the
    DTO whole. The table of a family that holds nothing to show prints one
    muted sentence.
34. The desktop app has a Stats screen at `#/stats`, titled `Stats`, last in
    the sidebar. Its header holds a `since` selector (all time, 24 hours, 7
    days, 30 days) and a repository selector. Both live in the URL
    (`?since=7d&repo=<id>`), and the screen hands them to every section as
    one `{ since, repo }`. The screen is a dashboard: it leads with a `Key
    figures` row — tasks finished, finish rate and median goal lead time off
    the work family, total tokens (input and output) off the spend family,
    and interventions and person time off the attention family's own
    `interventions.total` and `interventions.person_secs` — each figure
    reading `—` until its answer is in.
35. Under the key figures, the screen renders five sections in this order:
    `ModelsSection`, `WorkSection`, `TimeSection`, `SpendSection` and
    `AttentionSection` — `ModelsSection` ahead of rule 28's own order — laid
    out as a card grid: one column, and two from `xl` up, where `ModelsSection`
    spans both. Each draws through the shared `StatSection`: its heading
    (`text-sm font-medium`), one sentence of the question it answers, the
    read's error or skeleton, and the one muted sentence of an empty family.
    The other shared pieces are `StatTiles` and `StatTile` (a label, a value
    and an optional hint), `StatTable` (a compact comparison table, numbers
    right-aligned, columns given by the caller), `StatTimeChart` (stacked
    bars per bucket on a time axis, an hour bucket labelled as a time and
    any other as a date, the date added beside an hour's own tooltip and
    `sr-only` row) and `StatBarChart` (a horizontal bar per row). Every
    chart keeps an `sr-only` table of the same numbers under it,
    for a screen reader and for a test, in a `relative` wrapper: `sr-only` is
    `position: absolute`, and with no ancestor positioned the table would lay
    out against the document and draw a second scrollbar. A colour carries
    one meaning, off the
    status ramp in `index.css` through the shared `STATUS_COLORS` module —
    finished or ended on `status-done`, failed on `status-danger`, stalled on
    `status-warn`, cancelled on `status-pending`, a neutral count on
    `status-active` — and each chart's series config lives in
    `chart-configs.ts`.
36. Each section reads its family under `qk.stats.<family>(filter)`, in
    the query-key group `stats`. Every `task_updated`, `session_updated` and
    `goal_updated` event invalidates the whole group (`qk.stats.all()`),
    since any of the three may be a fact.
37. Every figure on the screen explains itself: a short plain sentence of
    what it is, shown on hover and on keyboard focus through the app's
    `Tooltip`, and wired as the figure's accessible description rather than a
    sighted-only hint (`StatExplain`). `StatTile` and each `StatTable` column
    take an `explain` string; a chart's figures carry theirs under
    `ChartConfig`'s `explain` field, drawn by `StatChartLegend` — a legend
    kept outside recharts' own measured area (`ResponsiveContainer`), so it
    renders, and is explained, with no dependency on whatever size recharts
    gives the chart itself. The legend carries every figure a chart draws
    through its `config`, a bar of its own or not: `StatTimeChart`'s `keys`
    and `extra`, and `StatBarChart`'s `bars`, each add their figures to it,
    and both also take a `legendExtra` of config keys with neither a bar nor
    a stacked slot — a figure the caller's own tooltip already carries its
    own way, such as a bucket's cached share or a status row's median and
    share, that still wants a hoverable, focusable place for its meaning.

### Work

What got done: goals completed, tasks finished, failed and cancelled, and
changes landed, over time.

37. A goal writes one `goal_ended` fact each time it moves to `completed` or
    `cancelled`. It is written inside `Store::set_goal_status`, in the same
    transaction as the status write, the way `task_ended` is written inside
    `transition_in_tx` (rule 22) — the one place every goal status change
    passes. A move to the same status writes no fact.
38. Its `data` holds `status` (`completed` or `cancelled`), `lead_time_secs`
    (from the goal's `created_at` to the move), `tasks` (the count of its
    tasks), `tasks_finished` (those `finished`) and `landing` (the goal's
    own). `goal_id` is the goal's; `repo_id` is the goal's one repository, or
    null where it works in several; `model` and `effort` are the goal's
    orchestrator pin.
39. `work_stats(filter) -> WorkStats { totals, bucket, buckets }` answers what
    got done. `totals` holds `goals_completed`, `goals_cancelled`,
    `median_goal_lead_time_secs` (over completed goals), `tasks_finished`,
    `tasks_failed`, `tasks_cancelled`, `finish_rate` (`tasks_finished` over
    the three, 0 where there are none) and `landed` (`task_ended` facts
    `finished` whose `landing` is `merge` or `pull_request`). `bucket` is
    `Bucket::for_span` of the span rule 32 names (`"hour"`, `"day"` or
    `"week"`); `buckets` is one row per bucket of that whole span, zeros
    included, each with `start`, `tasks_finished`, `tasks_failed`,
    `tasks_cancelled`, `goals_completed` and `landed`. `since` and
    `repo_id` narrow every count, as rule 12 says.
40. `GET /v1/stats/work` answers `WorkStatsDto`, `work_stats`'s own shape.
    `ariadne stats work` prints the totals as `label: value` lines, then a
    table of the buckets, `FROM` (the hour too where the bucket is an hour),
    `FINISHED`, `FAILED`, `CANCELLED`, `GOALS` and `LANDED`; `--format json`
    prints the DTO whole. `WorkSection` draws
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

What did it spend? Tokens, never a cost: the ledger holds no price, and
nothing here converts one.

38. `Store::spend_stats` reads every `session_ended` fact the filter keeps and
    answers `SpendStats { totals, per_finished_task, bucket, buckets,
    by_model }`.
39. `totals` sums every kept fact's `input_tokens`, `cached_input_tokens` and
    `output_tokens`, counts the facts as `sessions`, and carries
    `cached_share`: `cached_input_tokens` over `input_tokens`, 0 where
    `input_tokens` is 0.
40. `per_finished_task` is the tokens of every kept `session_ended` fact whose
    `task_id` names a task with a kept `task_ended` fact of status
    `finished`, divided by how many such tasks there are — `tasks`,
    `input_tokens` and `output_tokens`. 0 for all three where no task
    finished.
41. `bucket` is `Bucket::for_span` of the span rule 32 names. `buckets` is
    one row per bucket of that whole span, zeros where a bucket in it holds
    no fact, each with `start`, `input_tokens`, `cached_input_tokens` and
    `output_tokens`.
42. `by_model` is one row per model named by a kept fact, every seat pooled
    into it, with `input_tokens`, `cached_input_tokens`, `output_tokens` and
    `share` — `input_tokens + output_tokens` over the same total of every
    model — the heaviest model first. A fact naming no model answers no row.
43. `GET /v1/stats/spend` answers `SpendStatsDto`, the aggregate's own shape.
44. `ariadne stats spend` prints the totals and the per-task figures as
    `label: value` lines, then a table of `by_model` (`MODEL`, `TOKENS` — the
    usage cell every table prints — `SHARE`), then a table of `buckets`
    (`FROM` — the hour too where the bucket is an hour — `INPUT`, `CACHED`,
    `OUTPUT`). `--format json` prints the DTO whole.
45. `SpendSection` draws four tiles with `StatTiles`: input tokens, cache
    share, output tokens, and tokens per finished task (`input_tokens +
    output_tokens` of `per_finished_task`, hinted with the task count). Under
    them, one `StatTimeChart` stacks `input_tokens` and `output_tokens` per
    bucket, the bucket's own cached share added to its tooltip. It draws no
    chart of `by_model`: `ModelsSection` is where a model is compared. A
    family with no ended session is empty.

### Models

`Store::model_stats` reads only filtered `stat_facts` and returns `ModelStats { items }`.
`GET /v1/stats/models` returns the same fields through `ModelStatsDto` and the shared `StatsQuery`.
Each named model gets one row per seat, including models named only inside verdicts or picks.
A verdict names its `author_model` as an author, and a pick names its winner and losers as authors.
A model named only by a `switch` fact, a `permission` fact with `decided_by = console`, or an
`attention` fact with reason `waiting_input`, `waiting_user`, `stalled` or `agent_error` gets a row too.
Such a row carries zero figures.
Rows sort by orchestrator, author, reviewer, then no seat, and by model within each seat.
A fact without a model contributes to no figure.
Both filters apply before any aggregation.

Each row carries `model`, `seat`, `tasks`, `goals`, `tokens`, `time_secs`, `messages`,
`rounds_per_task` and `changes_per_task`.
`tasks` counts the distinct tasks of this model's `session_ended` facts in this seat.
`goals` counts the distinct goals of the same facts.
`tokens` sums `input_tokens` and `output_tokens` of those facts.
Cached tokens are already part of input and are never added again.
`time_secs` sums their `lifetime_secs`.
`messages` counts the `message` facts this model sent in this seat.

`rounds_per_task` is null except for authors.
For an author it is the mean `review_requests` over this model's `task_ended` facts with status `finished`.
`changes_per_task` is null except for reviewers.
For a reviewer it divides this model's `verdict` facts with verdict `request_changes`
by the distinct tasks this model gave a verdict on.
Each of the two is zero without a denominator.

The CLI prints one comparison table per seat, headed by the seat.
`--seat orchestrator|author|reviewer` belongs only to `stats models` and limits table output.
JSON output always contains the complete DTO, regardless of `--seat`.
A seatless row uses the TASKS columns under `NONE` in the CLI and remains available in JSON.
The desktop section draws Authors, Reviewers, and Orchestrators through `StatTable`, omitting empty seats.
Responses without rows for the three displayed seats use the shared empty state, including responses with only seatless rows.
Each desktop table sorts by its first count descending, then by model.

| Seat | Columns |
| --- | --- |
| Author | MODEL, TASKS, TOKENS, TIME, MESSAGES, ROUNDS/TASK |
| Reviewer | MODEL, TASKS, TOKENS, TIME, MESSAGES, CHANGES/TASK |
| Orchestrator | MODEL, GOALS, TOKENS, TIME, MESSAGES |

TOKENS uses compact notation and TIME the existing duration formatter.
ROUNDS/TASK and CHANGES/TASK print with one decimal.

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

The response also carries `interventions`, the times a person stepped in,
by these rules:
`permissions` counts `permission` facts with `decided_by = console`;
`questions` counts `attention` facts with reason `waiting_input` or
`waiting_user`; `stalls` counts `attention` facts with reason `stalled` or
`agent_error`; an `attention` fact with reason `waiting_permission` counts
nowhere; `total` is the sum of the three counts; `person_secs` sums
`wait_ms / 1000` of those permissions and `wait_secs` of those attention
facts. The time and repository filters apply before the counts.
`ariadne stats attention` prints the interventions total and the person time
alongside the family's other totals, and the Stats screen's key figures (rule
34) read the two straight off this object.

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
- A console permission answer writes a fact
  (`stats.rs::a_console_permission_answer_writes_a_fact`).
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
- A span of 2 days or less buckets by the hour, one of 31 days or less by
  the day, and a longer one by the week, and a fact falls in its hour, its
  day, or in its week from Monday
  (`stats/mod.rs::tests::a_short_span_is_bucketed_by_hour_a_medium_one_by_day_and_a_long_one_by_week`,
  `::a_fact_falls_in_its_hour_its_day_or_its_week_from_monday`).
- The axis runs bucket by bucket from the one `start` falls in to the one
  `end` falls in, and `filled_buckets` zero-fills every one of it a fact
  does not reach
  (`stats/mod.rs::tests::bucket_starts_runs_from_the_first_bucket_to_the_last_inclusive`,
  `::filled_buckets_zero_fills_every_bucket_the_facts_do_not_reach`).
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
- The five are classified and take the table flags of a listing
  (`cli/tests.rs::every_command_in_the_tree_is_classified`,
  `::the_listing_flags_are_advertised_exactly_where_they_are_honored`).
- The Stats screen renders the five sections in order under one heading
  style (`stats.test.tsx` "renders the five sections in order, under one
  heading style"), asks every family with the filters in its URL under the
  key `qk` names (`stats.test.tsx` "asks every family with the filters in
  its URL, under the key qk names"), and renders an error rather than the
  empty sentence when a read fails (`stats.test.tsx` "renders an error, not
  the empty sentence, when the daemon refuses a read"). It leads with the
  key figures, read off the work, spend and models answers, and lays Models
  across the full width with the other four as a two-column card grid
  (`stats.test.tsx` "leads with the key figures, read off the work, spend
  and models answers", "lays Models across the full width, and the other
  four out as a two-column card grid").
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
- `StatExplain` shows its sentence on hover and on keyboard focus, and names
  it as the trigger's accessible description
  (`stat-explain.test.tsx` "shows the explanation on hover, and names it as
  the trigger's accessible description", "shows the explanation on keyboard
  focus too, not only on hover").
- `StatChartLegend` explains an entry whose series carries one, and renders
  one with none plainly, with no trigger to open
  (`stat-chart-legend.test.tsx` "explains an entry with an explanation, on
  hover", "renders an entry with no explanation plainly, with no trigger to
  open").
- `StatTimeChart` labels an hour bucket as a time, with the date added
  rather than dropped
  (`stat-time-chart.test.tsx` "labels an hour bucket as a time, with the
  date added, not a bare date").
- Every key figure, and every tile, column and chart series of the Work,
  Time, Spend, Attention and Models sections, explains itself on hover
  (`stats.test.tsx` "explains every key figure, on hover";
  `work-section.test.tsx`, `time-section.test.tsx`, `spend-section.test.tsx`
  and `attention-section.test.tsx`, each "explains every tile and every
  chart series, on hover"; `models-section.test.tsx` "explains every column
  of every seat table, on hover").

#### Work

- A goal completed writes one `goal_ended` fact with its status, lead time,
  task counts and landing; a goal cancelled writes one too, and a second
  move to the same status writes no second one
  (`stats_work.rs::a_completed_goal_writes_one_goal_ended_fact`,
  `::a_cancelled_goal_writes_one_goal_ended_fact`).
- `work_stats` counts tasks finished, failed and cancelled, goals completed
  and changes landed, and honours `since` and `repo_id`
  (`stats/work.rs::tests::work_stats_counts_tasks_finished_failed_cancelled_and_goals_and_landed`,
  `::work_stats_honours_since_and_repo_id`).
- A `since` of a day or less buckets by the hour and a `since` of 7 days by
  the day, each filled with zeros from `since` to now even where no fact
  falls in it; with no `since`, facts within the last 10 days bucket by the
  day and facts over 60 days by the week
  (`stats/work.rs::tests::work_stats_buckets_by_hour_under_a_since_of_a_day_or_less`,
  `::work_stats_buckets_by_day_under_a_since_of_seven_days`,
  `::work_stats_fills_buckets_from_since_to_now_with_no_kept_fact`,
  `::work_stats_buckets_by_day_with_no_since_and_facts_within_ten_days`,
  `::work_stats_buckets_by_week_with_no_since_and_facts_over_sixty_days`).
- `GET /v1/stats/work` answers the totals and the buckets
  (`stats_work.rs::the_work_stat_answers_the_totals_and_the_buckets`).
- `ariadne stats work --format json` prints the DTO, and the table prints the
  totals and a row per bucket, an hour bucket's row showing the time of day
  beside its date
  (`commands/stats/work.rs::tests::the_totals_print_as_label_value_lines_in_field_order`,
  `::a_bucket_row_carries_every_count_in_column_order`,
  `::a_bucket_row_shows_the_time_of_day_for_an_hour_bucket`).
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

- The totals sum ended sessions and compute the cached share
  (`spend.rs::tests::the_totals_sum_ended_sessions_and_compute_the_cached_share`),
  pool every seat per model, heaviest first
  (`spend.rs::tests::models_are_pooled_across_seats_and_ranked_heaviest_first`),
  divide by finished tasks
  (`spend.rs::tests::per_finished_task_averages_the_tokens_of_tasks_that_finished`),
  and honour `since` and `repo_id`
  (`spend.rs::tests::since_and_repo_id_narrow_every_fact`).
- A `since` of a day or less buckets by the hour and a `since` of 7 days by
  the day, each filled with zeros from `since` to now even where no fact
  falls in it; with no `since`, facts within the last 10 days bucket by the
  day and facts over 60 days by the week
  (`spend.rs::tests::spend_stats_buckets_by_hour_under_a_since_of_a_day_or_less`,
  `::spend_stats_buckets_by_day_under_a_since_of_seven_days`,
  `::spend_stats_fills_buckets_from_since_to_now_with_no_kept_fact`,
  `::spend_stats_buckets_by_day_with_no_since_and_facts_within_ten_days`,
  `::spend_stats_buckets_by_week_with_no_since_and_facts_over_sixty_days`).
- `GET /v1/stats/spend` answers the totals, the buckets and the models for an
  ended session
  (`stats_spend.rs::the_spend_stat_answers_the_totals_the_buckets_and_the_models_for_an_ended_session`).
- The table prints the totals and the per-task figures as `label: value`
  lines
  (`commands/stats/spend.rs::tests::the_totals_and_the_per_task_figures_print_as_label_value_lines`),
  then a model row and a bucket row
  (`commands/stats/spend.rs::tests::the_table_prints_a_model_row_and_a_bucket_row`),
  an hour bucket's date carrying its time of day too
  (`commands/stats/spend.rs::tests::bucket_date_carries_the_time_of_day_for_an_hour_bucket`),
  and a family with no ended session is empty
  (`commands/stats/spend.rs::tests::a_family_with_no_ended_session_is_empty`).
- `SpendSection` draws the tiles and the time chart from a mocked response,
  with no by-model chart
  (`spend-section.test.tsx` "draws the tiles and the time chart from a
  mocked response, with no by-model chart").

#### Models

- Each row counts the distinct tasks and goals, the tokens without cached tokens again,
  the time and the messages of its model and seat
  (`stats/models.rs::tests::each_figure_counts_the_sessions_and_messages_of_its_model_and_seat`).
- Rounds per task is the mean over finished tasks, for authors only
  (`stats/models.rs::tests::rounds_per_task_is_the_mean_review_requests_of_finished_tasks_for_authors_only`).
- Changes per task divides changes requested by the tasks reviewed, for reviewers only
  (`stats/models.rs::tests::changes_per_task_divides_changes_requested_by_the_tasks_reviewed_for_reviewers_only`).
- A real `request_changes` verdict sent through the API lifts the reviewer's changes per task
  above zero
  (`stats_models.rs::a_real_request_changes_verdict_lifts_the_reviewers_changes_per_task`).
- Both review figures are zero without a denominator
  (`stats/models.rs::tests::the_review_figures_are_zero_without_a_denominator`).
- Every fact obeys both filters, including the exact time boundary
  (`stats/models.rs::tests::every_fact_is_filtered_by_time_and_repository`).
- Rows sort by seat, then by model
  (`stats/models.rs::tests::rows_sort_by_seat_then_model`).
- Models named only in verdict or pick payloads, or only by switch, console permission and
  attention facts, get rows under both filters
  (`stats/models.rs::tests::models_named_only_in_payloads_and_other_facts_get_rows_under_both_filters`).
- The route answers the new shape per model and seat under both filters
  (`stats_models.rs::each_model_and_seat_answers_its_figures_under_both_filters`).
- CLI tables print each seat's columns and figures in seat order, and only the models command accepts `--seat`
  (`commands/stats/models.rs::tests::each_seat_prints_its_own_columns_and_figures`,
  `::every_seat_prints_a_table_in_seat_order`,
  `::the_seat_option_belongs_only_to_models_and_prints_one_table`).
- JSON preserves the complete DTO with a seat selected
  (`commands/stats/models.rs::tests::json_reads_the_complete_dto_even_with_a_seat_selected`).
- The desktop reads under `qk.stats.models`, draws three sorted tables with their columns and formats, and omits empty seats
  (`models-section.test.tsx`: "draws three seat tables with their columns, formats and count ordering",
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
- Interventions count permissions, questions and stalls, exclude
  `waiting_permission`, sum `person_secs`, and obey both filters
  (`stats/attention.rs::tests::interventions_count_permissions_questions_and_stalls_and_exclude_waiting_permission`,
  `::interventions_honour_since_and_repo_id`).
- `GET /v1/stats/attention` answers the `interventions` object
  (`stats_attention.rs::the_attention_stat_answers_the_interventions_object`).
- `ariadne stats attention` prints the interventions total and the person
  time (`commands/stats/attention.rs::tests::the_kv_lines_carry_the_interventions_total_and_the_person_time`).
- The Stats screen's Interventions and Person time tiles read off the
  attention family, not the models family
  (`stats.test.tsx` "leads with the key figures, read off the work, spend and
  attention answers").

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
