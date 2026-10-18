---
id: stats
status: current
updated: 2026-10-18
areas: [api, store, daemon]
commits: []
tests:
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/stats/mod.rs
  - crates/ariadne-daemon/tests/it/stats.rs
  - crates/ariadne-daemon/tests/it/switch_stats.rs
  - crates/ariadne-daemon/tests/it/outcome_stats.rs
  - crates/ariadne-daemon/tests/it/stats_work.rs
  - crates/ariadne-daemon/tests/it/stats_time.rs
  - crates/ariadne-daemon/tests/it/stats_spend.rs
  - crates/ariadne-daemon/tests/it/stats_models.rs
  - crates/ariadne-daemon/tests/it/stats_attention.rs
  - crates/ariadne-daemon/src/http/stats/mod.rs
  - crates/ariadne-daemon/src/acp.rs
---

# Stats

What the work did. The daemon writes one fact to a ledger when a thing that
tells performance happens, and every stat is an aggregate of that ledger. The
stats are five families, each the answer to one question a user asks, read
with `GET /v1/stats/<family>`. The CLI no longer reads them: `ariadne stats`
is gone, and `ariadne query` reads the telemetry query API instead (034).
The desktop app's
Stats screen reads this ledger too, through a Perses dashboard rather than a
family section of its own — see spec 015.

## Scope

In: the ledger, the rules a fact obeys, every fact kind (`attention`, `session_ended`,
`switch`, `message`, `permission`, `task_ended` and `goal_ended`), the
filters every family takes, the frame the five families share —
their routes, the command, the screen and its shared pieces, the query keys
and the bucket rule — and each family's own rules.

Out: token accounting itself (012, rules 13 to 16), what a session is and how
it ends (008), what a skill is (017), and the task state machine and the
columns a task walks (001, 030) — the fact is a record of what they decided,
not a change to how they decide it.

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

### The message fact

19. Every stored task message writes a `message` fact. Its `data` holds
    `kind`, `from_actor` and `to_actor`, and the sender session supplies its
    model and seat.
20. There is no verdict fact: a review's outcome is a step call, and the
    `task_ended` fact names the column a task ended in (rule 23).

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
    `reason` (the transition's own reason), `lead_time_secs` (from the
    task's `created_at` to the transition), `landed` (true where the last
    column reported the commit the task landed as), `step` (the column the
    task ended in, absent for a task that never entered one) and
    `status_secs` (an object with the seconds in `pending`, `ready` and
    `in_progress`, summed across the task's whole life from its
    transitions). The ending status has no time. The repository, the goal
    and the task are the task's own; there is no session to one.
24. The seat is `agent`, and the model, the effort and the skills are those
    of the agent of the column the task ended in (030); a task that ended
    before its first column names none of the three.

### The frame

28. The stats are five families. Each answers one question a user asks, and
    the command and the docs show them in this order: `work` (what got
    done?), `time` (how long does it take?), `spend` (what did it spend?),
    `models` (which model does the job?) and `attention` (how much did it
    need me?).
29. `GET /v1/stats/<family>` answers the family's own DTO: `WorkStatsDto`,
    `TimeStatsDto`, `SpendStatsDto`, `ModelStatsDto` and `AttentionStatsDto`.
    Every route takes the filters of rules 12 to 14 and sits
    under the `stats` tag of the API document. No other route is under
    `/v1/stats/`.
30. Each family is one file in each layer: `stats/<family>.rs` in the
    store (`Store::<family>_stats`), the API and the daemon, where the file
    owns its handler and its part of the API document. The `mod.rs` beside
    each registers the five once and holds what they share.
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
33. *(retired with spec 034 — `ariadne stats` is removed, and the CLI no
    longer reads `/v1/stats/*`; `ariadne query` reads the telemetry query API
    instead. The daemon still serves every route of rule 29.)*
34. The desktop app's Stats screen (`#/stats`) no longer renders this ledger
    through family sections of its own: it opens a Perses dashboard, over
    `GET /v1/dashboards` and the daemon's own Prometheus and Loki APIs, whose
    rules live in spec 015. This ledger — the facts, the five families, their
    aggregates and `GET /v1/stats/<family>` — is unchanged;
    only what read it inside the desktop app changed, and nothing in this
    spec about the API describes the Stats screen any more.
35. *(retired with rule 34 — the family order, card grid and chart
    primitives it named belonged to the desktop screen alone)*.
36. *(retired with rule 34 — the query key and event invalidation it named
    belonged to the desktop screen alone; this ledger's own freshness rules
    are 10 and 18)*.
37. *(retired with rule 34 — the on-hover explanation it named belonged to
    the desktop screen alone)*.

### Work

What got done: goals completed, tasks finished, failed and cancelled, and
changes landed, over time.

38. A goal writes one `goal_ended` fact each time it moves to `completed` or
    `cancelled`. It is written inside `Store::set_goal_status`, in the same
    transaction as the status write, the way `task_ended` is written inside
    `transition_in_tx` (rule 22) — the one place every goal status change
    passes. A move to the same status writes no fact.
39. Its `data` holds `status` (`completed` or `cancelled`), `lead_time_secs`
    (from the goal's `created_at` to the move), `tasks` (the count of its
    tasks), `tasks_finished` (those `finished`) and `workflow` (the goal's
    own). `goal_id` is the goal's; `repo_id` is the goal's one repository, or
    null where it works in several; `model` and `effort` are the goal's
    orchestrator pin.
40. `work_stats(filter) -> WorkStats { totals, bucket, buckets }` answers what
    got done. `totals` holds `goals_completed`, `goals_cancelled`,
    `median_goal_lead_time_secs` (over completed goals), `tasks_finished`,
    `tasks_failed`, `tasks_cancelled`, `finish_rate` (`tasks_finished` over
    the three, 0 where there are none) and `landed` (`task_ended` facts
    `finished` whose `landed` is true). `bucket` is
    `Bucket::for_span` of the span rule 32 names (`"hour"`, `"day"` or
    `"week"`); `buckets` is one row per bucket of that whole span, zeros
    included, each with `start`, `tasks_finished`, `tasks_failed`,
    `tasks_cancelled`, `goals_completed` and `landed`. `since` and
    `repo_id` narrow every count, as rule 12 says.
41. `GET /v1/stats/work` answers `WorkStatsDto`, `work_stats`'s own shape.

### Time

42. `time_stats` reads `task_ended` facts whose status is `finished`. It
    answers their count, median, nearest-rank p90 and mean lead time; a
    missing value is zero. Its status rows follow the lifecycle order:
    `pending`, `ready`, `in_progress`, `under_review`, `changes_requested`
    and `approved`. Each gives total time, median time and its share of the
    status total. A fact without `status_secs` contributes no status time.
43. The family also counts `permission` facts where `decided_by` is `console`:
    the value the daemon writes for a person who answered. It answers their
    prompt count, total seconds and median seconds from `wait_ms`. Both the
    task and permission reads honour `since` and `repo`.
44. *(retired with rule 33 — it named the `ariadne stats time` table)*.

### Spend

What did it spend? Tokens, never a cost: the ledger holds no price, and
nothing here converts one.

45. `Store::spend_stats` reads every `session_ended` fact the filter keeps and
    answers `SpendStats { totals, per_finished_task, bucket, buckets,
    by_model }`.
46. `totals` sums every kept fact's `input_tokens`, `cached_input_tokens` and
    `output_tokens`, counts the facts as `sessions`, and carries
    `cached_share`: `cached_input_tokens` over `input_tokens`, 0 where
    `input_tokens` is 0.
47. `per_finished_task` is the tokens of every kept `session_ended` fact whose
    `task_id` names a task with a kept `task_ended` fact of status
    `finished`, divided by how many such tasks there are — `tasks`,
    `input_tokens` and `output_tokens`. 0 for all three where no task
    finished.
48. `bucket` is `Bucket::for_span` of the span rule 32 names. `buckets` is
    one row per bucket of that whole span, zeros where a bucket in it holds
    no fact, each with `start`, `input_tokens`, `cached_input_tokens` and
    `output_tokens`.
49. `by_model` is one row per model named by a kept fact, every seat pooled
    into it, with `input_tokens`, `cached_input_tokens`, `output_tokens` and
    `share` — `input_tokens + output_tokens` over the same total of every
    model — the heaviest model first. A fact naming no model answers no row.
50. `GET /v1/stats/spend` answers `SpendStatsDto`, the aggregate's own shape.
51. *(retired with rule 33 — it named the `ariadne stats spend` tables)*.
52. A family with no ended session is empty.

### Models

`Store::model_stats` reads only filtered `stat_facts` and returns `ModelStats { items }`.
`GET /v1/stats/models` returns the same fields through `ModelStatsDto` and the shared `StatsQuery`.
Each named model gets one row per seat.
A model named only by a `switch` fact, a `permission` fact with `decided_by = console`, or an
`attention` fact with reason `waiting_input`, `waiting_user`, `stalled` or `agent_error` gets a row too.
Such a row carries zero figures.
Rows sort by orchestrator, agent, then no seat, and by model within each seat.
A fact without a model contributes to no figure.
Both filters apply before any aggregation.

Each row carries `model`, `seat`, `tasks`, `goals`, `tokens`, `time_secs`, `messages`
and `tasks_finished`.
`tasks` counts the distinct tasks of this model's `session_ended` facts in this seat.
`goals` counts the distinct goals of the same facts.
`tokens` sums `input_tokens` and `output_tokens` of those facts.
Cached tokens are already part of input and are never added again.
`time_secs` sums their `lifetime_secs`.
`messages` counts the `message` facts this model sent in this seat.

`tasks_finished` counts this model's `task_ended` facts with status `finished`: the tasks
whose last column this model's agent ended.

A seatless row stays in the answer.

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
- A console permission answer writes a fact
  (`stats.rs::a_console_permission_answer_writes_a_fact`).
- A task that finishes writes one `task_ended` fact with its status, the
  model of the column's agent, its column and a lead time
  (`outcome_stats.rs::a_finished_task_writes_one_task_ended_fact`), a task
  cancelled before its first column names no agent
  (`::a_task_cancelled_before_its_first_column_names_no_agent`), and a task
  retried after it fails, and that fails again, writes a fact for each ending
  (`outcome_stats.rs::a_retried_task_that_fails_again_writes_two_facts`).

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
  (`stats_time.rs::the_time_stat_answers_and_is_in_the_api_document`,
  `stats_spend.rs::the_spend_stat_answers_and_is_in_the_api_document`,
  `stats_models.rs::the_models_stat_answers_and_is_in_the_api_document`,
  `stats_attention.rs::the_attention_stat_answers_and_is_in_the_api_document`,
  `stats_work.rs::the_work_stat_answers_the_totals_and_the_buckets`); the
  reviews, switches and outcomes routes answer 404
  (`stats.rs::the_old_families_are_gone`).
- The Stats screen's own acceptance criteria — the dashboard picker, the
  Reset action, the iframe and its message protocol — are spec 015's; none
  of this ledger's own behavior changed underneath it.

#### Work

- A goal completed writes one `goal_ended` fact with its status, lead time,
  task counts and workflow; a goal cancelled writes one too, and a second
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
#### Time

- A finished task records time in every status it passed
  (`outcome_stats.rs::a_finished_task_writes_one_task_ended_fact`).
- The aggregate reports lead time, status shares and person waits and filters
  by `since` and repository
  (`stats/time.rs::tests::time_stats_measures_finished_tasks_statuses_and_person_waits`).
- The route reads a finished task into the time DTO
  (`stats_time.rs::the_time_stat_answers_and_is_in_the_api_document`).
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

#### Models

- Each row counts the distinct tasks and goals, the tokens without cached tokens again,
  the time and the messages of its model and seat
  (`stats/models.rs::tests::each_figure_counts_the_sessions_and_messages_of_its_model_and_seat`).
- Tasks finished counts the finished endings of a model's column
  (`stats/models.rs::tests::tasks_finished_counts_the_finished_endings_of_a_models_column`),
  and a task finished in its column through the API lifts its agent's count
  (`stats_models.rs::a_task_finished_in_its_column_lifts_its_agents_tasks_finished`).
- Every fact obeys both filters, including the exact time boundary
  (`stats/models.rs::tests::every_fact_is_filtered_by_time_and_repository`).
- Rows sort by seat, then by model
  (`stats/models.rs::tests::rows_sort_by_seat_then_model`).
- Models named only by switch, console permission and attention facts get
  rows under both filters
  (`stats/models.rs::tests::models_named_in_other_facts_get_rows_under_both_filters`).
- The route answers the new shape per model and seat under both filters
  (`stats_models.rs::each_model_and_seat_answers_its_figures_under_both_filters`).

#### Attention

- Agent-event, idle-report and explicit clears write one attention fact only
  when they remove a flag (`store.rs::attention_clears_write_facts_only_when_a_flag_falls`).
- The aggregate groups permission answers, flag reasons, failures, stalls and
  exhausted switches under both filters (`stats/attention.rs`).
- The route exposes the family, including its decider and flag breakdowns
  (`stats_attention.rs`).
- Interventions count permissions, questions and stalls, exclude
  `waiting_permission`, sum `person_secs`, and obey both filters
  (`stats/attention.rs::tests::interventions_count_permissions_questions_and_stalls_and_exclude_waiting_permission`,
  `::interventions_honour_since_and_repo_id`).
- `GET /v1/stats/attention` answers the `interventions` object
  (`stats_attention.rs::the_attention_stat_answers_the_interventions_object`).

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/src/stats/`,
`crates/ariadne-store/src/tasks.rs`,
`crates/ariadne-store/src/events.rs`,
`crates/ariadne-api/src/stats/`,
`crates/ariadne-daemon/src/stats.rs`,
`crates/ariadne-daemon/src/http/stats/`,
`crates/ariadne-daemon/src/http/channel.rs`,
`crates/ariadne-daemon/src/http/events.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/scheduler/goals.rs`,
`crates/ariadne-daemon/src/scheduler/sweeps.rs`,
`crates/ariadne-daemon/src/scheduler/auto_switch.rs`,
`crates/ariadne-daemon/src/scheduler/tasks.rs`.
