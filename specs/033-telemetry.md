---
id: telemetry
status: current
updated: 2026-10-19
areas: [store, daemon, cli, ui, scripts]
commits: []
tests:
  - crates/ariadne-store/src/telemetry/labels.rs
  - crates/ariadne-store/src/telemetry/logs.rs
  - crates/ariadne-store/src/telemetry/metrics.rs
  - crates/ariadne-store/src/telemetry/retention.rs
  - crates/ariadne-daemon/src/telemetry/retention.rs
  - crates/ariadne-daemon/src/telemetry/metrics_exporter.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-daemon/tests/it/logs.rs
  - scripts/tests/check-migrations.sh
  - ui/src/features/system/daemon-log-stream.test.ts
  - ui/src/features/system/daemon-logs-drawer.test.tsx
---

# Telemetry

One telemetry store, apart from `ariadne.db`, holds every log record and
metric sample the daemon's `tracing` events and recorded metrics produce,
through an OpenTelemetry pipeline, with its own retention and its own
daemon-log read routes.

## Scope

In: the telemetry database and its schema, the write and read API over it,
the OpenTelemetry SDK pipeline that feeds it from the daemon's own
`tracing` events and metrics, hourly rollup and retention, and
`GET /v1/logs` / `GET /v1/logs/stream` read from it.

Out: OTLP ingest from another process (a later task writes through the API
here); PromQL evaluation and a `/api/v1/*` or `/loki/api/v1/*` query surface
(a later task reads through the API here); any metric instrument beyond
`ariadne.telemetry.records`; any change to the `ariadne.db` schema.

## Behavior

### The store

1. `ariadne-store`'s `telemetry` module opens a second SQLite database,
   `<home>/telemetry.db` (`telemetry_db_path`), under the same pragmas as
   `ariadne.db`: WAL journaling, one single-connection write pool, one
   multi-connection read pool, and an independent `checkpoint` that folds
   its write-ahead log back in off the commit path, the same reason
   `Store::checkpoint` does for `ariadne.db`. Its migrations live in
   `crates/ariadne-store/telemetry-migrations/`, numbered from their own
   `0001`, independent of `crates/ariadne-store/migrations/`'s own numbers.
   `scripts/check-migrations.sh` guards both directories the same way: a
   migration already on `main` must not be edited, deleted or retyped, and
   a new one must be a correctly numbered regular file — each directory's
   ceiling is its own.
2. `metric_series` holds one row per time series: id, name, kind (`gauge`,
   `counter` or `histogram`), unit, description, labels as JSON, and a hash
   of the canonical (sorted-key) label JSON for the find-or-create lookup
   every write makes. `metric_samples` holds one row per raw point: series
   id, timestamp, value, indexed by series and time and by time alone. A
   histogram has no table of its own: it is stored the Prometheus way, as
   three ordinary series sharing a name stem — `<name>_bucket` (one series
   per bucket, labelled `le` with that bucket's upper bound or `+Inf`),
   `<name>_sum` and `<name>_count` — each with cumulative bucket counts.
   `metric_rollups_1h` holds one row per series per UTC hour: min, max, sum,
   count, first and last. `log_records` holds one row per log record: id,
   event timestamp, observed timestamp, severity number, severity text,
   body, service, scope, target, attributes as JSON, trace id, span id;
   indexed by time and by service and time. `service` is always the
   emitting resource's `service.name`.
3. The write API accepts a batch of log records (`TelemetryStore::write_logs`)
   and a batch of metric points — name, kind, unit, description, labels,
   timestamp, value (`TelemetryStore::write_metrics`) — each in one
   transaction. The read API is wide enough for a PromQL evaluator and a
   Loki-shaped log query to need no SQL of their own: log records by filter
   (tail, service, level at or above, since, before an id —
   `TelemetryStore::read_logs`); series by label matchers (`=`, `!=`, `=~`,
   `!~`, matched against a series' name under the reserved label
   `__name__` the same as any other — `TelemetryStore::list_series`);
   samples and rollups of one series over a time range
   (`TelemetryStore::read_samples`, `read_rollups`); every label name and
   every value of one label, each narrowed by matchers
   (`TelemetryStore::label_names`, `label_values`); and a metric's kind,
   unit and description by name (`TelemetryStore::metric_metadata`).

### Retention

4. A daemon job, `ariadne_daemon::telemetry::retention`, runs once an hour
   (`Timeouts::telemetry_retention`). Each run rolls every raw metric
   sample older than an hour into its `metric_rollups_1h` row and marks that
   raw sample as represented. A repeated rollup ignores represented samples,
   while a late sample merges with the preserved hourly summary. Raw samples
   remain until it deletes samples older than
   `telemetry_samples_days` (default 30), log records older than
   `telemetry_logs_days` (default 14), and rollups older than
   `telemetry_rollups_days` (default 365). It then checkpoints the
   telemetry file the way `crate::checkpoint` does for `ariadne.db`. All
   three retention keys must be a positive number of days; a non-positive
   one is refused at startup and by `ariadned --check-config`, by name.

### The OpenTelemetry pipeline

5. The daemon builds an `SdkLoggerProvider`, bridging `tracing` into it
   through `opentelemetry-appender-tracing`'s `OpenTelemetryTracingBridge`,
   and an `SdkMeterProvider` with a `PeriodicReader`, set as the process's
   global meter provider (`opentelemetry::global::set_meter_provider`), so
   a later task records through `opentelemetry::global::meter` with no
   knowledge of the store underneath. Both the logger provider's batch
   processor and the meter provider's `PeriodicReader` run on their own
   plain thread. The two custom exporters (`StoreLogExporter`,
   `StoreMetricExporter`) block that SDK worker thread on their SQLite write,
   run on one Tokio runtime they share for the life of the process
   (`telemetry::export`, one worker thread, `telemetry-export`). Not a
   runtime per export: a pool returns each connection on a task spawned on
   the runtime that dropped it, and a runtime dropped with that task still
   waiting kept the store's one write connection from coming back, so the
   next export waited out its timeout. The
   resource both providers share carries `service.name=ariadned` and
   `service.version` from the crate version; `LogExporter::set_resource`
   is how the log exporter learns the service name a batch's records carry,
   since a `LogBatch` carries no resource of its own. The daemon's stdout
   `fmt` layer is unaffected; the two run side by side. Both providers are
   shut down (flushing whatever they had queued) on daemon exit.
6. One metric proves the pipeline end to end: `ariadne.telemetry.records`,
   a `u64` counter with labels `signal` (`"log"` or `"metric"`) and
   `service`, incremented by each exporter with the number of rows it just
   wrote — so the counter's own write is itself collected and exported the
   same way on the next periodic collection.
   The metric exporter also puts the resource's `service.name` on every
   point it writes, under the reserved label key
   `__resource_service_name__`, after the point's attributes, so the
   resource always wins. An attribute that already uses the key moves to
   `exported___resource_service_name__` (another `exported_` is added
   while that key is taken), so no attribute value is lost. The query
   routes read the service from this key (034, rule 8).

### The log routes

7. `GET /v1/logs` and `GET /v1/logs/stream` read `TelemetryStore::read_logs`
   rather than an in-memory ring. Query parameters: `tail` (default 200),
   `service`, `level` (at or above; an unknown level is refused with 400),
   and `since`. The stream subscribes before its opening snapshot; the
   snapshot's persisted last ID drops only duplicate broadcasts, so a record
   committed during that handoff is still in either the snapshot or a delta.
   It then follows records through a broadcast
   (`ariadne_daemon::telemetry::LogStream`) the log exporter feeds as it
   writes each batch; there is still no replay, so a follower that falls
   behind reconnects and starts over from a fresh snapshot.
   `LogLineDto` carries `service` beside `ts`, `level`, `target` and
   `message`; its `message` is the record's body with every attribute
   appended as ` key=value` (`ariadne_api::logs::render_message`), the same
   shape the daemon's old in-process capture layer rendered live.
8. `ariadne daemon logs` gains `--service` and `--level`, each forwarded as
   the matching query parameter to both the snapshot and the stream route.
9. The desktop logs drawer gains a service filter (all, `ariadned`,
   `ariadne-desktop`, `ariadne-cli`) and a level filter (all, trace, debug,
   info, warn, error), as two selects beside the drawer's title; picking
   either reopens the stream on the new filter, replacing what was shown.

## Acceptance criteria

- Label matchers (`=`, `!=`, `=~`, `!~`), including against the reserved
  `__name__` label, and an unreadable regex refused rather than panicking:
  `ariadne-store::telemetry::labels::tests::eq_and_ne_match_the_label_value`,
  `::a_missing_label_matches_only_the_empty_string`,
  `::the_name_label_matches_the_series_name_rather_than_a_label`,
  `::regex_operators_match_and_negate`,
  `::an_invalid_regex_is_reported_rather_than_panicking`,
  `::an_empty_matcher_list_matches_everything`.
- Log records are written and read back oldest first, narrowed by tail,
  service and level, an unknown level refused, and retention deletes only
  what is older than its cutoff:
  `ariadne-store::telemetry::logs::tests::written_records_read_back_oldest_first`,
  `::tail_keeps_only_the_newest_n_records`,
  `::service_and_level_narrow_the_read`,
  `::an_unknown_level_is_refused_rather_than_silently_matching_nothing`,
  `::retention_deletes_only_records_older_than_the_cutoff`.
- A repeat metric point of the same name and labels shares one series;
  series listing, label names and values narrow by matcher; metadata
  answers a metric's kind and unit; a series' hour of raw samples rolls
  into one row while raw samples remain through their retention window;
  and retention deletes old samples and old rollups on their own cutoffs:
  `ariadne-store::telemetry::metrics::tests::repeat_points_of_the_same_name_and_labels_share_one_series`,
  `::list_series_is_narrowed_by_label_matchers`,
  `::label_names_and_values_are_read_across_matching_series`,
  `::metadata_reports_kind_and_unit`,
  `::rollup_keeps_raw_samples_until_their_retention_cutoff`,
  `::equivalent_timestamp_forms_share_inclusive_range_boundaries`,
  `::an_invalid_timestamp_is_refused_before_its_metric_is_written`,
  `::a_writer_waiting_after_rollup_selection_keeps_its_raw_sample`,
  `::delete_old_samples_removes_a_raw_sample_that_was_never_rolled_up`,
  `::delete_old_rollups_keeps_only_the_rollup_within_its_cutoff`.
- One retention pass rolls up before raw retention deletes, so a sample stays
  queryable as raw data until its configured cutoff:
  `ariadne-store::telemetry::retention::tests::one_pass_rolls_up_and_then_deletes_raw_samples_past_retention`,
  `::a_two_hour_old_sample_survives_a_thirty_day_raw_retention_window`,
  `::rollups_keep_expired_history_and_merge_late_samples`.
- The daemon's own hourly job rolls up metrics and deletes what is past its
  cutoff, logs and metrics alike, in one pass:
  `ariadne-daemon::telemetry::retention::tests::one_tick_rolls_up_metrics_and_deletes_what_is_past_its_cutoff`.
- The telemetry keys are read from `config.toml` with their defaults, and a
  non-positive retention window is refused at startup and by
  `--check-config`, by name:
  `config.rs::a_config_file_that_says_nothing_keeps_every_default`,
  `::telemetry_keys_are_read_from_config`,
  `::telemetry_retention_days_must_be_positive_at_start_and_check`.
- `GET /v1/logs` and `GET /v1/logs/stream` read the telemetry store: order,
  tail (explicit and the default), service and level filters, an unknown
  level refused with 400, the stream's opening snapshot and its deltas,
  hostile content surviving SSE framing, both routes in the OpenAPI
  document, and the pipeline's own counter moving on an exported record:
  `logs.rs::the_snapshot_returns_captured_lines_in_order`,
  `::tail_limits_the_snapshot_to_the_last_n_lines`,
  `::the_default_snapshot_is_bounded_even_without_an_explicit_tail`,
  `::service_narrows_the_snapshot_to_one_service`,
  `::level_narrows_the_snapshot_to_that_level_and_above`,
  `::an_unknown_level_is_a_bad_request`,
  `::the_stream_opens_with_a_snapshot_then_follows_with_deltas`,
  `ariadne_daemon::http::logs::tests::an_export_during_stream_initialization_is_in_the_snapshot`,
  `::a_live_since_filter_normalizes_fractional_precision_and_offsets`,
  `::hostile_log_content_survives_sse_framing`,
  `::both_endpoints_are_in_the_openapi_document`,
  `::the_pipeline_counts_exported_log_records_on_its_own_metric`.
- Log integration tests wait for stored records or the next stream message
  under `Timeouts::telemetry_pipeline_patience`. They report the first failed
  wait without retrying the test:
  `logs.rs::the_default_snapshot_is_bounded_even_without_an_explicit_tail`,
  `::service_narrows_the_snapshot_to_one_service`,
  `::level_narrows_the_stream_snapshot_and_its_deltas`.
- The metric exporter stores the resource's service name under the
  reserved key, and moves a colliding attribute aside without losing it
  (`telemetry/metrics_exporter.rs::tests::the_resource_service_wins_and_a_colliding_attribute_moves_aside`,
  `logs.rs::the_pipeline_counts_exported_log_records_on_its_own_metric`).
- `check-migrations.sh` guards `telemetry-migrations/` the same way it
  guards `migrations/`, each numbered from its own `0001`
  (`scripts/tests/check-migrations.sh`: `telemetry-reject-edit`,
  `telemetry-reject-low-number`,
  `telemetry-accept-add-independent-of-the-other-directory`).
- The desktop log stream carries a service and level filter as query
  parameters, or none where the filter picks everything, and the drawer
  reconnects with the picked filter:
  `daemon-log-stream.test.ts::daemonLogStreamUrl > carries a service and level filter as query parameters`,
  `::leaves the query string off when the filter picks everything`,
  `daemon-logs-drawer.test.tsx::reconnects with a service and level filter when one is picked`.

## Sources

`crates/ariadne-store/telemetry-migrations/`,
`crates/ariadne-store/src/telemetry/`,
`crates/ariadne-daemon/src/telemetry/`,
`crates/ariadne-daemon/src/http/logs.rs`,
`crates/ariadne-daemon/src/config.rs`,
`crates/ariadne-daemon/src/timeouts.rs`,
`crates/ariadne-api/src/logs.rs`,
`crates/ariadne-cli/src/commands/daemon.rs`,
`scripts/check-migrations.sh`,
`ui/src/features/system/daemon-log-stream.ts`,
`ui/src/features/system/daemon-logs-drawer.tsx`.
