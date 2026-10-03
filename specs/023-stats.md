---
id: stats
status: current
updated: 2026-10-03
areas: [api, store, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/it/stats.rs
  - crates/ariadne-daemon/tests/it/switch_stats.rs
  - crates/ariadne-daemon/tests/it/outcome_stats.rs
  - crates/ariadne-daemon/src/http/stats.rs
  - crates/ariadne-daemon/src/acp.rs
  - crates/ariadne-cli/src/commands/stats.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - ui/src/routes/stats.test.tsx
  - ui/src/components/stats/status-colors.test.ts
  - ui/src/components/stats/chart-configs.test.ts
  - ui/src/events/dispatch.test.ts
  - ui/src/components/app-shell.test.tsx
---

# Stats

How the tool and the models perform. The daemon writes one fact to a ledger
when a thing that tells performance happens, and every stat is an aggregate
of that ledger. The stats are read with `GET /v1/stats/<family>`,
`ariadne stats <family>` and the Stats screen of the desktop app.

## Scope

In: the ledger, the rules a fact obeys, the `session_ended` fact, the
`switch` fact, the `task_ended` and `pick` facts, the `models`, `switches`
and `outcomes` stat families, the filters every family takes, and the
route, the command and the screen that show it.

Out: token accounting itself (012, rules 13 to 16), what a session is and how
it ends (008), what a skill is (017), and the task state machine and the pick
itself, `task_picks` included (004) — the fact is a record of what they
decided, not a change to how they decide it. Each other stat family adds its
fact and its rules here when it is built.

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

### The `models` family

15. `GET /v1/stats/models` answers `{items: [ModelStatDto]}`: one row per
    `(model, seat)` that a `session_ended` fact names, ordered by model and
    then by seat. A loose session's row has a null seat.
16. A row carries `sessions` (the facts), `failed` (those whose status is
    `failed`), `stalled` (those whose attention reason is `stalled`),
    `usage` (the three token counts, summed), `cached_share` (cached input
    over input, 0 to 1, and 0 where no input went in), `mean_lifetime_secs`,
    and `skills`: each skill those runs loaded, with how many runs loaded it,
    the most loaded first and then by name.

### The `switch` fact

17. A session writes one `switch` fact as `Launcher::switch_session` moves it
    off a model, next to the `session.switched` event it already writes
    there, whichever of its two paths — same-row or new-row — carries the
    switch. The fact is of the session that leaves, so its `model` is the
    model left. A same-agent switch writes the fact before the row's pin
    moves, so `model` still names what the session left rather than what
    `set_session_pin` just gave the row.
18. Its `data` holds `to_model` and `to_effort` (the model and effort
    entered), `reason` (the event's own reason: `requested` for a switch
    asked by hand, `exhausted` for one the daemon made of its own accord, or
    whatever else a caller of `switch_session` writes), `automatic` (`true`
    where `reason` is `exhausted`, the one reason the daemon switches a
    session itself), and `same_agent` (whether the conversation carried
    over).
19. The exhaustion sweep's own bookkeeping event, `session.auto_switch` —
    written only for the in-place case, so a later sweep can tell a model
    already tried in the session's chain (009) — writes no fact of its own:
    the switch it marks already has one, from the `switch_session` call that
    wrote it.
20. A fact that cannot be written is logged and goes no further: by the time
    `switch_session` writes it, the pin has moved or the successor and its
    event already exist, and a ledger failure does not undo or refuse a
    switch already under way (rule 10).

### The `switches` family

21. `GET /v1/stats/switches` answers `{items: [SwitchStatDto], switches,
    exhaustions}`: one row per model a `switch` fact names, as the model it
    left, the model it arrived on, or both, ordered by model.
22. A row carries `switches` (facts that left this model), `by_reason` (each
    reason a switch left it, with how many gave it, the most common first
    and then by name), `exhaustions` (of `switches`, those reasoned
    `exhausted`), `automatic_share` (the automatic share of `switches`, 0 to
    1, and 0 where it had none), and `arrivals` (facts whose `to_model` is
    this one).
23. `switches` and `exhaustions` on the response are those same two counts,
    summed over every row.

### The `reviews` family

24. The `reviews` family records a `message` fact for every stored task
    message, with `kind`, `from_actor` and `to_actor`; the sender session
    supplies its model and seat. An accepted verdict also records a `verdict`
    fact from the reviewer session: verdict, author model and session, its
    author's request count as the derived round, and latency from the latest
    request. Its aggregate returns author approval counts, mean and median
    rounds and first-pass rate; reviewer verdict count, approve share and mean
    latency; and messages by kind and sender with total and mean per task.
    `since` and `repo` filter its facts.

### The `tools` family

25. A `tool_call` fact is written when an ACP tool call ends. Its `data` is
    `tool_name`, `duration_ms`, and `ok`; a failed ACP status writes `ok = false`.
    The runtime keeps the opening instant with each open call and spawns the
    `session_fact` and ledger write, so neither a tool update nor a turn waits
    for SQLite.
26. A `permission` fact is written after each ACP permission reply. Its
    `data` is `tool_name`, `decided_by`, `answer` (`allow`, `deny`, or
    `cancelled`), `console_option_id`, and `wait_ms`.
27. `GET /v1/stats/tools` answers `ToolStatsDto`: tool rows carry calls,
    errors, median and p90 duration; model rows carry calls and mean duration;
    permission rows group total and mean wait by `decided_by` and `answer`.
    The median averages the two middle durations and p90 uses nearest rank.

### The command

28. `ariadne stats <family> [--since <duration|date>] [--repo <id>]` prints a
    family. `ariadne stats` alone prints `models`. `--repo` takes an id or a
    unique prefix of one (014). `--since` is sent to the daemon as it was
    written.
29. `ariadne stats models` prints a table of `MODEL`, `SEAT`, `SESSIONS`,
    `FAILED`, `STALLED`, `TOKENS`, `LIFETIME` and `SKILLS`. `TOKENS` is the
    usage cell every other table prints (`↑1.2M 89.1% ↓45k`), `LIFETIME` is
    the mean lifetime, and `SKILLS` is `name count` per skill.
    `--format json` prints the rows the daemon answered. It takes the table
    flags of a listing (014).
30. `ariadne stats switches` prints a table of `MODEL`, `SWITCHES`,
    `EXHAUSTED`, `AUTOMATIC` and `ARRIVALS`. `AUTOMATIC` is `automatic_share`
    as a percentage to one decimal place. `--format json` prints the rows
    the daemon answered. It takes the table flags of a listing (014).

### The screen

31. The desktop app has a Stats screen at `#/stats`, titled `Stats`, last in
    the sidebar. Its header holds a `since` selector (all time, 24 hours, 7
    days, 30 days) and a repository selector. Both live in the URL
    (`?since=7d&repo=<id>`), and the screen hands them to every panel as one
    `{ since, repo }`.
32. Each family is a panel of its own, and every panel draws the one shared
    heading, `StatSectionHeading` (`text-sm font-medium`), so the five read
    as one section style. A row is a horizontal bar, not a table: one bar per
    model, seat or tool, built by the one shared `StatBarChart`, sorted by
    its first series descending so a long model id still reads as a label at
    the left. The models panel draws sessions per model and seat, stacked
    bars of ended and failed with stalled as a second series beside them —
    `stalled` can overlap `failed`, so stacking it in would draw a bar past
    `sessions` — and a second chart of tokens per model and seat; the
    switches panel draws switches per model, split into exhausted,
    automatic and other, with arrivals as a second series beside them. Every
    chart keeps an `sr-only` table of the same numbers under it, for a screen
    reader and for a test. A family with no rows renders its heading and the
    one muted sentence its empty state always said, and no chart. The bars'
    colours carry one meaning each, off the status ramp in `index.css`
    through the shared `STATUS_COLORS` module — finished or ended on
    `status-done`, failed on `status-danger`, stalled on `status-warn`,
    cancelled on `status-pending`, a neutral count on `status-active` — so
    the same meaning is the same colour in every panel.
33. The stats queries sit under the query-key group `stats`
    (`qk.stats.models(filter)`, `qk.stats.switches(filter)`). Every
    `task_updated` and `session_updated` event invalidates the whole group,
    since either may be a fact.

### The `task_ended` fact

34. A task writes one `task_ended` fact each time it reaches `finished`,
    `cancelled` or `failed`. It is written in the same transaction as the
    status write, inside `Store::transition_task`'s shared body
    (`transition_in_tx`) — the one place every status change passes — rather
    than from the daemon once the call returns: that way no caller of
    `transition_task` can reach one of the three endings without the fact
    being written alongside it. A task retried and failed again writes one
    more fact.
35. Its `data` holds `status` (`finished`, `cancelled` or `failed`),
    `reason` (the transition's own reason), `landing` (the task's own, as
    005 spells it), `lead_time_secs` (from the task's `created_at` to the
    transition), `review_requests` (the count of `review_request` messages
    on the task) and `authors` (the count of author agents staffed), and
    `picked` (true where `picked_agent_id` is set). The repository, the
    goal and the task are the task's own; there is no session to one.
36. The model, the effort and the skills are the picked author's where the
    task staffed several authors, the one author's where it staffed a
    single one, and none of the three where it staffed several and none
    was picked yet — a task cancelled or failed before its pick settled.

### The `pick` fact

37. A contested task's settled pick writes one `pick` fact, inside
    `Store::set_task_picked` itself (004 rule 14) rather than from the
    daemon once that call returns: the winner's column and this fact are
    one transaction, so there is no moment where a daemon could die with
    the winner written and the fact not. Its `data` holds `winner_model`,
    `loser_models` (the losing authors' models, as an array) and
    `reviewers` (the count of reviewers staffed on the task).
38. The repository and the goal are the task's own, and the model, the
    effort and the skills are the winning `task_agents` row's own
    (`task_agent_skills`) rather than a session's: a session can end, be
    resumed under another one, or never have existed, none of which should
    stand between the winner being on the task and the fact that says who
    lost to it. Two authors of one contest sharing a model are still one
    row each, since the fact names the model, not the agent. `session_id`
    and `launch_id` are the winner's live session where `run_the_pick`
    found one, carried straight through to the fact; absent where it found
    none.
39. Each settled contest calls `set_task_picked` once, so each one writes
    one fact of its own: a task retried clears the winner
    (`Store::clear_task_picks`) and runs its review again, and the next
    settlement's fact is its own rather than a second one of a task
    already named.

### The `outcomes` family

40. `GET /v1/stats/outcomes` answers `{items: [OutcomeStatDto], totals:
    OutcomeTotalsDto}`: one row per author model a `task_ended` or a `pick`
    fact names, ordered by model.
41. A row carries `finished`, `failed` and `cancelled` (the `task_ended`
    facts of that status for that model), `finish_rate` (`finished` over
    the three), `median_lead_time_secs` and `mean_lead_time_secs`,
    `mean_review_requests`, `contests_entered` (the `pick` facts naming
    that model as the winner or among the losers), `contests_won` (the
    ones naming it the winner), and `win_rate` (`contests_won` over
    `contests_entered`). A rate with nothing to take a share of is 0. Two
    authors of one contest on the same model are one entry for it, not two.
42. The totals are the sum of the rows above them, field for field,
    lead times and review requests pooled into one mean and one median
    rather than averaged again: a `task_ended` fact with no author to name
    answers for no row and so for none of the totals either.
43. `ariadne stats outcomes` prints a table of `MODEL`, `FINISHED`,
    `FAILED`, `CANCELLED`, `FINISH_RATE`, `LEAD_TIME`, `REVIEWS`,
    `CONTESTS` and `WIN_RATE`, one row per model and a totals row after
    them. `--format json` prints the DTO whole, totals included. It takes
    the table flags of a listing (014).
44. The desktop app's Outcomes panel draws the per-model rows of
    `ariadne stats outcomes` as a chart, stacked bars of tasks per model
    split into finished, failed and cancelled, and a second chart of median
    lead time per model; finish rate, win rate, contests and mean reviews
    go in the first chart's tooltip. The totals row is the CLI's own and is
    not drawn: a chart compares models against each other, which a totals
    bar is not one of. It reads under the same `since` and `repo` the
    screen's header sets, and under the same query-key group
    (`qk.stats.outcomes(filter)`).

## Acceptance criteria

- A fact recorded is read back by `model_stats`, summed into one row with its
  counts, tokens, cached share, mean lifetime and skills
  (`store.rs::a_recorded_fact_is_read_back_by_model_stats`).
- `since` and `repo_id` narrow the facts
  (`store.rs::model_stats_keep_the_facts_since_the_filter_and_of_its_repository`).
- Review aggregates drop facts older than `since`
  (`store.rs::review_stats_drop_facts_older_than_since`).
- Verdict facts count review requests as rounds, message facts record each
  stored message, and the review endpoint returns author metrics
  (`review_stats.rs::verdicts_record_rounds_messages_and_review_stats`).
- The reviews command serializes its DTO and names its three table groups
  (`commands/stats.rs::tests::reviews_json_has_the_dto_and_the_table_has_three_groups`).
- The Stats screen asks for the reviews panel alongside model stats
  (`stats.test.tsx` "draws the models panel's charts from the daemon's rows").
- Tool durations produce the documented median and nearest-rank p90, and
  `since` excludes later facts
  (`store.rs::tool_stats_keep_the_facts_since_the_filter_and_measure_percentiles`).
- A tool completion and a console permission answer write their facts, and
  `GET /v1/stats/tools` returns both
  (`stats.rs::a_tool_call_and_console_permission_are_reported_as_tool_stats`).
- `ariadne stats tools --format json` prints the tools DTO and the table
  prints its tool and permission groups
  (`commands/stats.rs::tools_json_prints_the_dto_and_the_table_groups_its_rows`).
- The tools panel renders the daemon response under `qk.stats.tools`
  (`stats.test.tsx` "draws the tools panel's charts from the daemon's rows").
- `switch_stats` honours `since`
  (`store.rs::switch_stats_keeps_the_facts_since_the_filter`).
- Deleting the goal keeps the fact
  (`store.rs::a_fact_outlives_the_goal_it_is_about`).
- A fact is written once per launch
  (`store.rs::a_fact_is_recorded_once_per_launch`).
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
- A fact announces its session once it is readable, and a refused one
  announces nothing
  (`store.rs::a_fact_announces_its_session_once_it_is_readable`).
- `GET /v1/stats/models` returns the ended session's row
  (`stats.rs::the_models_stat_returns_the_row_of_an_ended_session`).
- A manual switch writes one `switch` fact naming the model left and the
  model entered, `reason=requested` and `automatic=false`
  (`switch_stats.rs::a_manual_switch_writes_one_switch_fact_with_the_model_left_and_entered`),
  and a same-agent switch writes its fact before the pin moves, so the model
  named is the one left
  (`switch_stats.rs::a_same_agent_switch_writes_the_fact_before_the_pin_moves`).
- An exhausted session that auto-switches writes one `switch` fact,
  `reason=exhausted` and `automatic=true`
  (`switch_stats.rs::an_exhausted_session_that_auto_switches_writes_one_switch_fact`).
- `GET /v1/stats/switches` returns a manual switch and an exhausted
  auto-switch each in the row of the model it left, and the model the
  automatic switch arrived on counts the arrival
  (`switch_stats.rs::the_switches_stat_returns_both_in_the_row_of_the_model_left`).
- `since=1h` keeps a fresh fact, and a moment after now keeps none
  (`stats.rs::since_keeps_the_facts_written_since_then`); a bad `since` is a
  400 naming it (`stats.rs::a_bad_since_is_refused`).
- The route is in the API document under the `stats` tag
  (`stats.rs::the_models_stat_is_in_the_api_document`).
- A span counts back from now, a moment is taken as written, and anything
  else is refused
  (`http/stats.rs::tests::a_span_counts_back_from_now`,
  `::a_moment_is_taken_as_written`, `::anything_else_is_refused`).
- `ariadne stats models --format json` reads the rows with the filters given,
  and `ariadne stats` alone runs `models`
  (`commands/stats.rs::tests::stats_models_reads_the_rows_with_the_filters_given`);
  the table prints its headers and a token cell
  (`commands/stats.rs::tests::the_table_prints_headers_and_a_token_cell`).
- `ariadne stats switches --format json` prints the rows the daemon answered,
  with the filters given
  (`commands/stats.rs::tests::stats_switches_reads_the_rows_with_the_filters_given`);
  the table prints a row per model
  (`commands/stats.rs::tests::the_switches_table_prints_a_row_per_model`).
- `stats models` and `stats switches` take the table flags of a listing
  (`cli/tests.rs::the_listing_flags_are_advertised_exactly_where_they_are_honored`),
  and `stats` alone parses as `models` with the filters on either side of the
  family (`cli/tests.rs::stats_alone_runs_models_and_takes_the_filters_either_side`).
- The Stats screen draws the models panel's charts from the daemon's rows
  (`stats.test.tsx` "draws the models panel's charts from the daemon's rows"), and
  asks with the filters in its URL under the key `qk` names
  (`stats.test.tsx` "asks with the filters in its URL, under the key qk names").
- The Stats screen draws the switches panel's chart from the daemon's rows
  (`stats.test.tsx` "draws the switches panel's chart from the daemon's rows").
- A task or a session update invalidates the stats
  (`dispatch.test.ts` "refetches every stat when a task or a session moves").
- Stats is the last entry of the sidebar
  (`app-shell.test.tsx` "ends the navigation with stats, right after
  repositories and permissions").
- A task that finishes writes one `task_ended` fact with its status, its
  author's model and a lead time
  (`outcome_stats.rs::a_finished_task_writes_one_task_ended_fact`), and a task
  retried after it fails, and that fails again, writes a fact for each ending
  (`outcome_stats.rs::a_retried_task_that_fails_again_writes_two_facts`).
- A two-author task writes one `pick` fact with the winner's and the loser's
  models, and `GET /v1/stats/outcomes` then counts a contest entered for both
  and won for the winner alone
  (`outcome_stats.rs::a_contested_pick_writes_one_fact_the_outcomes_stat_counts`).
- `outcome_stats` honours `since` and `repo_id`
  (`store.rs::outcome_stats_keeps_the_facts_since_the_filter_and_of_its_repository`),
  and a `task_ended` fact outlives the goal it is about
  (`store.rs::an_outcome_fact_outlives_the_goal_it_is_about`).
- The totals equal the sum of the rows, so a fact with no model to name
  inflates neither
  (`store.rs::outcome_totals_equal_the_sum_of_the_rows`).
- Two authors on the same model in one contest are one entry for it
  (`store.rs::a_contest_names_a_shared_model_once`).
- A task retried writes a `pick` fact for each settled cycle, not just the
  first
  (`store.rs::a_retried_contest_writes_a_pick_fact_for_each_settled_cycle`).
- `ariadne stats outcomes --format json` prints the DTO whole, and the table
  prints a row per model and a totals row
  (`commands/stats.rs::tests::stats_outcomes_format_json_reads_the_dto_with_the_filters_given`,
  `::the_table_prints_headers_a_model_row_and_a_totals_row`).
- `stats outcomes` is classified and takes the table flags of a listing
  (`cli/tests.rs::every_command_in_the_tree_is_classified`,
  `::the_listing_flags_are_advertised_exactly_where_they_are_honored`).
- The outcomes panel draws its charts from a mocked response, under the key
  `qk.stats.outcomes` names
  (`stats.test.tsx` "draws the outcomes panel's charts from the daemon's
  rows", "asks with the filters in its URL, under the key qk names").
- The reviews panel's chart reads the author rows of a mocked response
  (`stats.test.tsx` "draws the reviews panel's chart from the daemon's
  rows").
- The five panels' headings share one class list
  (`stats.test.tsx` "gives the five panels the same heading style").
- An empty family renders its heading and one sentence, with no chart
  (`stats.test.tsx` "renders an empty family as its heading and one
  sentence, with no chart").
- `STATUS_COLORS` maps each meaning to the status ramp's own CSS variable
  (`status-colors.test.ts` "maps each meaning to the status ramp's own CSS
  variable, so every panel's chart draws it the same").
- Every chart's own series config names the right meaning for each key, and
  the same meaning is the same colour wherever it is drawn again
  (`chart-configs.test.ts`, one test per panel's config and one that compares
  them against each other).
- A session's `ended` count is the rest of `sessions` once `failed` is taken
  out, not `failed` and `stalled` both, so a run that is both does not count
  against it twice (`stats.test.tsx` "draws the models panel's charts from
  the daemon's rows"). `stalled` stands beside the `ended`/`failed` stack
  rather than inside it, so it draws no bar past `sessions`
  (`chart-configs.test.ts`).
- The tools panel draws the permission chart on its own facts: a tool with no
  completed call still charts the permission replies that named it
  (`stats.test.tsx` "draws the permission chart even where no tool call
  completed").

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/src/stats.rs`,
`crates/ariadne-store/src/tasks.rs`,
`crates/ariadne-store/src/picks.rs`,
`crates/ariadne-store/src/events.rs`,
`crates/ariadne-api/src/stats.rs`,
`crates/ariadne-daemon/src/stats.rs`,
`crates/ariadne-daemon/src/http/stats.rs`,
`crates/ariadne-daemon/src/http/events.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/scheduler/goals.rs`,
`crates/ariadne-daemon/src/scheduler/sweeps.rs`,
`crates/ariadne-daemon/src/scheduler/auto_switch.rs`,
`crates/ariadne-daemon/src/scheduler/tasks.rs`,
`crates/ariadne-cli/src/commands/stats.rs`,
`ui/src/routes/stats.tsx`,
`ui/src/components/stats/models-panel.tsx`,
`ui/src/components/stats/reviews-panel.tsx`,
`ui/src/components/stats/switches-panel.tsx`,
`ui/src/components/stats/tools-panel.tsx`,
`ui/src/components/stats/outcomes-panel.tsx`,
`ui/src/components/stats/stat-bar-chart.tsx`,
`ui/src/components/stats/stat-query-state.tsx`,
`ui/src/components/stats/chart-configs.ts`,
`ui/src/components/stats/status-colors.ts`,
`ui/src/components/stats/section-heading.tsx`,
`ui/src/api/query-keys.ts`,
`ui/src/events/dispatch.ts`.
