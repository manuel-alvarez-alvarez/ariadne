---
id: telemetry-query
status: current
updated: 2026-10-19
areas: [daemon, cli, docs, specs]
commits: []
tests:
  - crates/ariadne-daemon/src/http/query/mod.rs
  - crates/ariadne-daemon/src/http/query/promql.rs
  - crates/ariadne-daemon/src/http/query/logql.rs
  - crates/ariadne-daemon/tests/it/telemetry_query.rs
  - crates/ariadne-cli/src/commands/query.rs
  - crates/ariadne-cli/src/cli/tests.rs
---

# Telemetry query APIs

Perses's and Grafana's standard Prometheus and Loki plugins read the
daemon's metrics and logs through two query APIs over the telemetry store
(033). `ariadne query <expr>` prints the answer of a PromQL expression.

## Scope

In: the Prometheus and Loki routes, the Prometheus name translation, the
supported PromQL and LogQL, the rollup reads, and `ariadne query`.

Out: recording metrics (the exporter is 033's), the stats ledger and
`/v1/stats/*` (023), and the dashboards that call these routes (015, 036).

## Behavior

### Routes

1. The Prometheus routes are `GET` and `POST /api/v1/query` (`query`,
   `time`), `GET` and `POST /api/v1/parse_query` (`query`), `GET` and `POST
   /api/v1/query_range` (`query`, `start`, `end`,
   `step`), `GET` and `POST /api/v1/labels` (`match[]`, `start`, `end`),
   `GET /api/v1/label/{name}/values` (`match[]`, `start`, `end`), `GET` and
   `POST /api/v1/series` (`match[]`, `start`, `end`), `GET /api/v1/metadata`
   (`metric`, `limit`) and `GET /-/healthy`. A `POST` body is form-encoded, and its
   parameters join those of the query string. A repeated `match[]` keeps
   every value. A `start` or `end` on the three discovery routes narrows
   them to the series with a raw sample, or the rollup of an hour, between
   the two; a missing one is open.
2. Each Prometheus answer is the standard envelope:
   `{"status": "success", "data": …}`, or a `400` with
   `{"status": "error", "errorType": "bad_data", "error": …}`. The `error`
   names what was wrong: the unsupported function, aggregation, operator,
   modifier or stage, or the bad parameter.
3. A time is Unix seconds or RFC 3339. A step is a number of seconds or a
   Prometheus duration (`1m30s`). A range query with no `step` steps by the
   span over 250, rounded up to a whole second. A range query refuses an
   end before its start, a step of zero or less, and more than 11,000
   points.
4. The Loki routes are `GET /loki/api/v1/query_range` (`query`, `start`,
   `end`, `limit`, `direction`, `step`), `GET /loki/api/v1/query` (`query`,
   `time`, `limit`, `direction`), `GET /loki/api/v1/labels`,
   `GET /loki/api/v1/label/{name}/values` and `GET /loki/api/v1/series`
   (`match[]`), each with `start` and `end`. `GET /loki/api/v1/index/stats`
   answers `{"streams": 0, "chunks": 0, "entries": 0, "bytes": 0}` and
   `GET /loki/api/v1/index/volume` an empty vector. A Loki time is
   nanoseconds, Unix seconds or RFC 3339, kept to the nanosecond: a line is
   in when `start <= ts < end`, exactly, though the store holds
   milliseconds. A log query reads the last hour
   by default, newest first, at most 100 lines.
5. Both routers serve every route. The TCP router adds CORS for the
   origins it allows (012).

### Names

6. A metric name and an attribute name have each character outside
   `[a-zA-Z0-9_]` replaced by `_`. A unit adds its suffix (`s` →
   `_seconds`, `ms` → `_milliseconds`, `By` → `_bytes`, and the other
   UCUM time and byte units); a unit in braces adds nothing. A counter adds
   `_total` after the unit. `ariadne.tokens` reads `ariadne_tokens_total`.
7. A histogram is stored as `_bucket` series with an `le` label beside
   `_sum` and `_count` (033). The unit goes before that suffix, and the
   suffix is kept: `ariadne.task.lead_time` in seconds reads
   `ariadne_task_lead_time_seconds_bucket`. A `_sum` or `_count` is a
   histogram component only beside such buckets. `/api/v1/metadata` lists
   the histogram under its family name with type `histogram`.
8. Every series carries `service` and `job`, both its resource's
   `service.name`. The metric exporter stores that name on each point under
   the reserved label key `__resource_service_name__` (033, rule 6), and
   the query routes read it from that key alone; the key is not shown. An
   attribute named `service` or `job` shows as `exported_service` or
   `exported_job` (another `exported_` is added while that key is taken), so
   no attribute sets either label. A series stored before the exporter
   added the key shows `ariadned`.

### PromQL

9. A query is parsed with `promql-parser` 0.11 and checked before any read.
   The subset is: instant and range selectors with `=`, `!=`, `=~` and
   `!~` (the regular expressions anchored); `offset`; the `@` modifier
   with a timestamp, `start()` or `end()`, which pins a selector to that
   time at every step (in a `match[]` selector, `@` is refused); `rate`, `irate`,
   `increase`, `delta`, `idelta`; `sum`, `avg`, `min`, `max`, `count`,
   `topk`, `bottomk`, `quantile`, with `by` and `without`;
   `avg_over_time`, `sum_over_time`, `min_over_time`, `max_over_time`,
   `count_over_time`, `last_over_time`, `quantile_over_time`, `changes`;
   `histogram_quantile`; `+`, `-`, `*`, `/`, `%`, `^` and the comparison
   operators with or without `bool`, between scalars, between a vector
   and a scalar, and between vectors with `on`, `ignoring`, `group_left`
   and `group_right`; `and`, `or`, `unless`; `abs`, `ceil`, `floor`,
   `round`, `clamp`, `clamp_min`, `clamp_max`, `absent`, `time`, `vector`,
   `scalar`, `label_replace`, `label_join`, `sort`, `sort_desc`. A
   subquery, `fill`, `or` inside a selector, and every
   other function, aggregation and operator answer `bad_data` naming it.
10. The lookback delta is 5 minutes: an instant selector takes a series'
    latest sample in the 5 minutes up to its time.
11. `rate`, `increase` and `delta` extrapolate to the window's edges the
    way Prometheus does, and `rate` and `increase` add a counter reset back.
    Arithmetic and functions drop `__name__`; a comparison that filters
    keeps it, and `last_over_time`, `sort`, `sort_desc`, `label_replace`
    and `label_join` keep it. One-to-one matching keeps only the `on`
    labels, or drops the `ignoring` ones; `group_left` and `group_right`
    keep the many side's labels and copy the named labels from the one
    side. A one-to-one match that finds two series on one side refuses, and
    so does any match whose results land on one label set twice.
    `clamp`, `clamp_min` and `clamp_max` keep a `NaN` value or bound as
    `NaN`, as Go's `math.Min` and `math.Max` do.
12. The evaluator reads through the telemetry store's read API only:
    `list_series` once, then `read_samples` and `read_rollups` of each
    series a selector names, over the whole span the query needs. Each step
    is evaluated over that one read.
13. Beyond the raw retention a series has only hourly rollups. An hour
    with no raw sample reads its rollup: an instant selector sees the
    rollup's `last` through the hour and the lookback after it, and
    `min_over_time`, `max_over_time`, `avg_over_time`, `sum_over_time` and
    `count_over_time` fold the rollup's `min`, `max`, `sum` and `count`
    in whole. Other range functions see a rollup as its `first` and `last`.
14. An instant query answers `vector`, `scalar`, `string` or, for a bare
    range selector, `matrix`. A range query answers `matrix`, one series
    per label set across the steps; an expression that is neither a scalar
    nor an instant vector refuses. A value is a string, with `NaN`, `+Inf`
    and `-Inf` spelled the Prometheus way.

### LogQL

15. A Loki stream has the labels `service`, `level` (the severity text in
    lower case), `scope` and `target`; an empty one is left out. A
    `streams` answer carries each line's nanosecond timestamp as a string
    and the record's body as the line.
16. The subset is: a stream selector over those labels with `=`, `!=`,
    `=~` and `!~` (anchored); the line filters `|=`, `!=`, `|~` and `!~`
    (not anchored); a label filter such as `| level="error"`; and
    `count_over_time` and `rate` over a log selector with a range, alone or
    under `sum` or `sum by (...)`, the `by` before or after. A parser stage
    (`json`, `logfmt` and the rest), `without`, a numeric label filter and
    every other function answer `400` naming it.
17. A metric query answers `matrix` from `query_range` and `vector` from
    `query`; a step with no line in its window has no point.

### The command

18. `ariadne query <expr>` runs an instant query at now. `--at <rfc3339 |
    span>` moves the instant. `--since <span | rfc3339>` runs a range query
    from then to now, one point per `--step <span>`; the default step is
    the span over 250, rounded up to a whole second. `--at` and `--since`
    exclude each other, and `--step` needs `--since`.
19. The table has one column per label of the result, then `VALUE`; a
    range adds `TIME` first, and prints one row per point, by time. A
    scalar prints one `VALUE`. The table flags of 014 apply, and `-q`
    prints the first column. `--format json` prints the daemon's answer
    whole. A daemon error prints its `error` sentence.
20. `ariadne stats` is removed, and the CLI no longer reads
    `/v1/stats/*` (023, rule 33).

## Acceptance criteria

- An instant query answers a grouped vector under the Prometheus names,
  with `service` and `job`, and `histogram_quantile` over the buckets
  (`telemetry_query.rs::an_instant_query_answers_a_vector_under_prometheus_names`).
- A form-posted range query answers a matrix, one point per step, with the
  name dropped by arithmetic
  (`telemetry_query.rs::a_form_posted_range_query_answers_a_matrix_per_step`).
- An unsupported function answers `400` `bad_data` naming it
  (`telemetry_query.rs::an_unsupported_query_answers_bad_data_naming_it`),
  and so do an unsupported aggregation, operator, subquery and modifier
  (`promql.rs::tests::unsupported_functions_and_operators_are_named`).
- `parse_query` accepts both request forms and answers its Prometheus AST;
  malformed PromQL answers `400` `bad_data`
  (`telemetry_query.rs::the_parse_query_route_answers_an_ast_or_bad_data`).
- `@ start()`, `@ end()` and `@ <time>` pin a selector, `offset` included,
  and a `match[]` selector refuses `@`
  (`promql.rs::tests::at_pins_a_selector_to_the_query_start_end_or_a_time`,
  `promql.rs::tests::a_series_selector_refuses_at`).
- Labels, label values, series and metadata answer, the histogram under
  its family, and `/-/healthy` answers
  (`telemetry_query.rs::discovery_routes_list_labels_series_and_metadata`);
  `start` and `end` leave out a series with no sample between them
  (`telemetry_query.rs::discovery_bounds_list_only_series_with_a_sample_between_them`).
- The TCP router answers a query with CORS for an allowed origin
  (`telemetry_query.rs::the_tcp_router_serves_the_query_routes_with_cors`).
- Query string and form body merge, and a repeated `match[]` keeps every
  value (`mod.rs::tests::parameters_merge_the_query_string_and_a_form_body`).
- Times and durations take every spelling, Loki nanoseconds included
  (`mod.rs::tests::times_and_durations_take_every_spelling`); the default
  step is the span over 250 and a step past 11,000 points refuses
  (`mod.rs::tests::the_default_step_is_the_span_over_250_in_whole_seconds`);
  values are strings in the Prometheus spellings
  (`mod.rs::tests::values_are_strings_with_prometheus_spellings`).
- `service` and `job` come from the reserved key alone, and an attribute
  of either name moves to an `exported_` label
  (`promql.rs::tests::service_and_job_come_from_the_resource_alone`).
- Counters add `_total`, and units go before a histogram's suffix
  (`promql.rs::tests::counters_gain_total_and_units_precede_histogram_suffixes`);
  translation marks histogram components and adds `service` and `job`
  (`promql.rs::tests::translation_marks_histogram_components_and_adds_service_and_job`).
- Selectors apply the four matchers and the 5-minute lookback
  (`promql.rs::tests::selectors_apply_all_four_matchers_and_the_lookback`);
  `offset` reads an earlier sample
  (`promql.rs::tests::offset_reads_an_earlier_sample`).
- `rate`, `increase`, `irate`, `idelta` and `changes` follow Prometheus
  (`promql.rs::tests::rate_and_increase_extrapolate_like_prometheus`); the
  `_over_time` functions reduce a window
  (`promql.rs::tests::over_time_functions_reduce_each_window`).
- Rollups answer beyond the raw retention, and a raw hour ignores its
  rollup (`promql.rs::tests::rollups_answer_beyond_the_raw_retention`).
- The eight aggregations group with `by` and `without`
  (`promql.rs::tests::aggregations_group_by_and_without`);
  `histogram_quantile` interpolates within a bucket
  (`promql.rs::tests::histogram_quantile_interpolates_within_a_bucket`).
- Arithmetic and comparisons with scalars, `bool` and unary minus
  (`promql.rs::tests::arithmetic_and_comparisons_with_scalars`); vector
  matching with `on`, `ignoring`, `group_left` and `group_right`
  (`promql.rs::tests::vector_matching_with_on_ignoring_and_groups`), a
  grouped match that collides on one label set refusing
  (`promql.rs::tests::a_grouped_match_that_collides_on_one_label_set_refuses`);
  the set operators (`promql.rs::tests::set_operators_match_on_labels`).
- The instant functions and label transforms
  (`promql.rs::tests::instant_functions_transform_each_sample`), the clamps
  keeping `NaN` (`promql.rs::tests::clamp_keeps_nan_in_values_and_bounds`),
  and
  `absent`, `time`, `vector`, `scalar`, `sort` and `sort_desc`
  (`promql.rs::tests::absent_time_vector_scalar_and_sort`).
- Loki streams carry nanosecond times and filtered lines; `sum by` over
  `count_over_time` answers a matrix; labels, values, series and the two
  index routes answer; a parser stage refuses
  (`telemetry_query.rs::loki_streams_carry_nanosecond_times_and_filtered_lines`).
  A bound 1ns past a stored line decides it exactly
  (`telemetry_query.rs::loki_bounds_keep_their_nanoseconds`).
- LogQL selectors take the four matchers, anchored
  (`logql.rs::tests::selectors_take_the_four_matchers`); line and label
  filters narrow the lines
  (`logql.rs::tests::line_and_label_filters_narrow_the_lines`); metric
  queries count and rate per stream or sum
  (`logql.rs::tests::metric_queries_count_and_rate_per_stream_or_sum`);
  anything else is refused by name
  (`logql.rs::tests::anything_else_is_refused_by_name`).
- `ariadne query` runs at now or at `--at`
  (`commands/query.rs::tests::an_instant_query_runs_at_now_or_at_the_given_moment`),
  a range defaults its step to the span over 250
  (`commands/query.rs::tests::a_range_query_defaults_its_step_to_the_span_over_250`),
  and spans parse (`commands/query.rs::tests::parses_cli_spans_for_query_boundaries`).
- The table has a column per label then `VALUE`
  (`commands/query.rs::tests::an_instant_table_has_a_column_per_label_then_value`),
  one row per time for a range
  (`commands/query.rs::tests::a_range_table_has_one_row_per_time`), one
  value for a scalar (`commands/query.rs::tests::a_scalar_is_one_value`),
  and a daemon error reads as its sentence
  (`commands/query.rs::tests::a_daemon_error_reads_as_its_sentence`).
- The flags parse, and `--at` with `--since` or `--step` without `--since`
  refuse (`cli/tests.rs::query_takes_instant_and_range_flags`,
  `::query_refuses_at_with_since_and_step_without_since`); `query` is a
  listing and takes the table flags
  (`cli/tests.rs::every_command_in_the_tree_is_classified`,
  `::the_listing_flags_are_advertised_exactly_where_they_are_honored`).



## Sources

- The Prometheus HTTP API, its OpenTelemetry name translation and its
  PromQL evaluation (`extrapolatedRate`, `bucketQuantile`).
- The Loki HTTP API and LogQL.
- `crates/ariadne-daemon/src/http/query/`,
  `crates/ariadne-cli/src/commands/query.rs`,
  `crates/ariadne-store/src/telemetry/`.
