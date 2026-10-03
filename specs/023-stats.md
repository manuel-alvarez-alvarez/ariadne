---
id: stats
status: current
updated: 2026-10-03
areas: [api, store, daemon, cli, ui]
commits: []
tests:
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/it/stats.rs
  - crates/ariadne-daemon/src/http/stats.rs
  - crates/ariadne-daemon/src/acp.rs
  - crates/ariadne-cli/src/commands/stats.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - ui/src/routes/stats.test.tsx
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
`models` stat family, the filters every family takes, and the route, the
command and the screen that show it.

Out: token accounting itself (012, rules 13 to 16), what a session is and how
it ends (008), and what a skill is (017). Each other stat family adds its
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

### The command

17. The `reviews` family records a `message` fact for every stored task
    message, with `kind`, `from_actor` and `to_actor`; the sender session
    supplies its model and seat. An accepted verdict also records a `verdict`
    fact from the reviewer session: verdict, author model and session, its
    author's request count as the derived round, and latency from the latest
    request. Its aggregate returns author approval counts, mean and median
    rounds and first-pass rate; reviewer verdict count, approve share and mean
    latency; and messages by kind and sender with total and mean per task.
    `since` and `repo` filter its facts.

18. `ariadne stats <family> [--since <duration|date>] [--repo <id>]` prints a
### The `tools` family

17. A `tool_call` fact is written when an ACP tool call ends. Its `data` is
    `tool_name`, `duration_ms`, and `ok`; a failed ACP status writes `ok = false`.
    The runtime keeps the opening instant with each open call and spawns the
    `session_fact` and ledger write, so neither a tool update nor a turn waits
    for SQLite.
18. A `permission` fact is written after each ACP permission reply. Its
    `data` is `tool_name`, `decided_by`, `answer` (`allow`, `deny`, or
    `cancelled`), `console_option_id`, and `wait_ms`.
19. `GET /v1/stats/tools` answers `ToolStatsDto`: tool rows carry calls,
    errors, median and p90 duration; model rows carry calls and mean duration;
    permission rows group total and mean wait by `decided_by` and `answer`.
    The median averages the two middle durations and p90 uses nearest rank.

17. `ariadne stats <family> [--since <duration|date>] [--repo <id>]` prints a
    family. `ariadne stats` alone prints `models`. `--repo` takes an id or a
    unique prefix of one (014). `--since` is sent to the daemon as it was
    written.
19. `ariadne stats models` prints a table of `MODEL`, `SEAT`, `SESSIONS`,
    `FAILED`, `STALLED`, `TOKENS`, `LIFETIME` and `SKILLS`. `TOKENS` is the
    usage cell every other table prints (`↑1.2M 89.1% ↓45k`), `LIFETIME` is
    the mean lifetime, and `SKILLS` is `name count` per skill.
    `--format json` prints the rows the daemon answered. It takes the table
    flags of a listing (014).

### The screen

20. The desktop app has a Stats screen at `#/stats`, titled `Stats`, last in
    the sidebar. Its header holds a `since` selector (all time, 24 hours, 7
    days, 30 days) and a repository selector. Both live in the URL
    (`?since=7d&repo=<id>`), and the screen hands them to every panel as one
    `{ since, repo }`.
21. Each family is a panel of its own. The models panel shows one row per
    model and seat, its tokens as the app's token figure.
22. The stats queries sit under the query-key group `stats`
    (`qk.stats.models(filter)`). Every `task_updated` and `session_updated`
    event invalidates the whole group, since either may be a fact.

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
  (`stats.test.tsx` "renders the models panel from the daemon's rows").
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
  (`stats.test.tsx` "renders the tools panel from the daemon's rows").
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
- `stats models` takes the table flags of a listing
  (`cli/tests.rs::the_listing_flags_are_advertised_exactly_where_they_are_honored`),
  and `stats` alone parses as `models` with the filters on either side of the
  family (`cli/tests.rs::stats_alone_runs_models_and_takes_the_filters_either_side`).
- The Stats screen renders the models panel from the daemon's rows
  (`stats.test.tsx` "renders the models panel from the daemon's rows"), and
  asks with the filters in its URL under the key `qk` names
  (`stats.test.tsx` "asks with the filters in its URL, under the key qk names").
- A task or a session update invalidates the stats
  (`dispatch.test.ts` "refetches every stat when a task or a session moves").
- Stats is the last entry of the sidebar
  (`app-shell.test.tsx` "ends the navigation with stats, right after
  repositories and permissions").

## Sources

`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-store/src/stats.rs`,
`crates/ariadne-store/src/events.rs`,
`crates/ariadne-api/src/stats.rs`,
`crates/ariadne-daemon/src/stats.rs`,
`crates/ariadne-daemon/src/http/stats.rs`,
`crates/ariadne-daemon/src/http/events.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/scheduler/goals.rs`,
`crates/ariadne-daemon/src/scheduler/sweeps.rs`,
`crates/ariadne-cli/src/commands/stats.rs`,
`ui/src/routes/stats.tsx`,
`ui/src/components/stats/models-panel.tsx`,
`ui/src/api/query-keys.ts`,
`ui/src/events/dispatch.ts`.
