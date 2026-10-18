# Telemetry

Ariadne keeps the daemon's own observability data — its logs and its
metrics — apart from the orchestration database. `ariadne.db` holds goals,
tasks, sessions and everything else a goal moves through; `telemetry.db`
holds what the daemon itself said and measured while it did that work. The
two are separate SQLite files so that a chatty daemon never grows the file
every task, goal and session query reads.

## What is stored

Every `tracing` event the daemon emits — the same lines that used to live
only in a bounded in-memory ring — becomes a log record: a timestamp, a
level, the module it came from, its message, and the service that wrote it
(`ariadned` today; `ariadne-desktop` and `ariadne-cli` once they report
through the same pipeline). Nothing is dropped to make room for a newer
line, the way the old ring buffer dropped its oldest; retention (below)
is what eventually removes a record, on its own schedule.

Every metric the daemon records — starting with `ariadne.telemetry.records`,
a counter of how many log records and metric points the pipeline has
written, labelled by `signal` (`log` or `metric`) and `service` — becomes a
time series: a name, a kind (gauge, counter or histogram), a unit, a set of
labels, and a value at a point in time. A histogram is stored the
Prometheus way, as three ordinary series sharing a name stem: `<name>_bucket`
(one series per upper bound, labelled `le`), `<name>_sum` and `<name>_count`.

Both flow through the [OpenTelemetry](https://opentelemetry.io) SDK rather
than being written straight to the store: the daemon bridges `tracing` into
an `SdkLoggerProvider`, and sets an `SdkMeterProvider` as the process's
global one, so a later task can record a metric through
`opentelemetry::global::meter` without knowing anything about SQLite. Each
provider's batch processor and periodic collector run on their own thread,
with no daemon work waiting on either.

## Where, and for how long

`telemetry_db_path` in `config.toml` sets where the database lives (default
`<home>/telemetry.db`); see [Configuration](configuration.md). An hourly job
rolls every raw metric sample older than an hour into an hourly summary
(min, max, sum, count, first, last). It keeps the raw samples too, and only
deletes each kind of record once its own cutoff has passed:

- `telemetry_samples_days` (default 30) — raw metric samples, including
  samples already represented in an hourly rollup.
- `telemetry_logs_days` (default 14) — log records.
- `telemetry_rollups_days` (default 365) — hourly metric rollups.

All three have to be a positive number of days; `ariadned --check-config`
reports a non-positive one by name, the same way it reports a bad
`tcp_listen`. The same job also folds the telemetry file's write-ahead log
back in afterward, the way `checkpoint.rs` does for `ariadne.db`.

## Reading the logs

`GET /v1/logs` and `GET /v1/logs/stream` read the telemetry store instead of
an in-memory buffer, so a restart loses no line. Both take `tail` (default
200), `service`, `level` (at or above), and `since` query parameters. The
stream opens with that same
snapshot, then follows new records as the log exporter writes them —
there is still no replay, so a follower that falls behind reconnects and
starts over from a fresh snapshot, exactly as before.

```sh
ariadne daemon logs                          # the last 200 lines
ariadne daemon logs -f                       # ...and keep printing as it happens
ariadne daemon logs --service ariadne-cli    # only that service
ariadne daemon logs --level warn             # only warnings and errors
```

See [Following what happens](following-events.md) for where this sits
beside `ariadne events` and the session/task transcript logs, and
[Configuration](configuration.md) for every telemetry key. The desktop
app's logs drawer carries the same two filters, as selects beside the
drawer's title.

## Querying metrics and logs

The daemon answers two standard query APIs over the telemetry store, so a
Prometheus or Loki client reads its metrics and logs as it would read a
Prometheus or a Loki server:

| API | Routes |
| --- | --- |
| Prometheus | `GET`/`POST /api/v1/query`, `GET`/`POST /api/v1/query_range`, `GET`/`POST /api/v1/labels`, `GET /api/v1/label/{name}/values`, `GET`/`POST /api/v1/series`, `GET /api/v1/metadata`, `GET /-/healthy` |
| Loki | `GET /loki/api/v1/query`, `GET /loki/api/v1/query_range`, `GET /loki/api/v1/labels`, `GET /loki/api/v1/label/{name}/values`, `GET /loki/api/v1/series`, `GET /loki/api/v1/index/stats`, `GET /loki/api/v1/index/volume` |

A `POST` takes a form-encoded body, as the Perses and Grafana clients send
it. Every answer uses the standard `{"status", "data"}` envelope. A query
outside the supported subset answers `400` with `"errorType": "bad_data"`
and an `error` that names the function, operator or stage. The two index
routes answer a valid, empty shape: the store keeps no index.

### Metric and label names

The Prometheus API shows the OpenTelemetry names under the Prometheus
translation rules:

- A dot becomes an underscore. An attribute becomes a label, under the same
  rule.
- A unit adds its suffix: `s` adds `_seconds`, `ms` adds `_milliseconds`,
  `By` adds `_bytes`. A unit in braces, such as `{token}`, adds nothing.
- A counter adds `_total` after the unit.
- A histogram shows `_bucket` series with an `le` label, then `_sum` and
  `_count`, with the unit before that suffix.

So `ariadne.tokens` reads `ariadne_tokens_total`, and
`ariadne.task.lead_time` in seconds reads
`ariadne_task_lead_time_seconds_bucket`. Every series carries `service` and
`job`, both the service name of the process that recorded it. An attribute
named `service` or `job` shows as `exported_service` or `exported_job`. For
example, the pipeline counter's own `service` attribute reads
`ariadne_telemetry_records_total{exported_service="ariadned"}`. A series
stored by an older daemon, before it kept the service name, shows
`ariadned`.

A Loki stream has four labels: `service`, `level` (the severity text in
lower case), `scope` and `target`. Each line is the record's body, with a
nanosecond timestamp.

### The supported PromQL

- Instant and range selectors with `=`, `!=`, `=~` and `!~`, and `offset`.
  An instant selector looks back 5 minutes for a sample.
- `rate`, `irate`, `increase`, `delta`, `idelta`.
- `sum`, `avg`, `min`, `max`, `count`, `topk`, `bottomk` and `quantile`,
  with `by` or `without`.
- `avg_over_time`, `sum_over_time`, `min_over_time`, `max_over_time`,
  `count_over_time`, `last_over_time`, `quantile_over_time`, `changes`.
- `histogram_quantile`.
- `+`, `-`, `*`, `/`, `%`, `^` and `==`, `!=`, `>`, `<`, `>=`, `<=` (with or
  without `bool`), between a vector and a scalar, and between two vectors
  with `on`, `ignoring`, `group_left` and `group_right`.
- `and`, `or`, `unless`.
- `abs`, `ceil`, `floor`, `round`, `clamp`, `clamp_min`, `clamp_max`,
  `absent`, `time`, `vector`, `scalar`, `label_replace`, `label_join`,
  `sort`, `sort_desc`.

Subqueries, the `@` modifier and every other function refuse with
`bad_data`.

Raw samples are kept for `telemetry_samples_days`. Past that, a series has
only its hourly rollups. An hour with no raw sample reads its rollup: an
instant selector sees the rollup's last value, and `min_over_time`,
`max_over_time`, `avg_over_time`, `sum_over_time` and `count_over_time`
read the rollup's columns.

### The supported LogQL

- A stream selector over the four stream labels, with `=`, `!=`, `=~` and
  `!~`: `{service="ariadned", level=~"warn|error"}`.
- The line filters `|=`, `!=`, `|~` and `!~`.
- A label filter on a stream label: `| level="error"`.
- `count_over_time` and `rate` over a log selector, and `sum` or
  `sum by (...)` over them:
  `sum by (level) (count_over_time({service="ariadned"}[5m]))`.

A parser stage such as `| json`, and every other function, refuse with
`bad_data`.

### From the CLI

`ariadne query` runs one PromQL expression and prints a table: one column
per label of the result, then `VALUE`.

```sh
ariadne query 'sum by (seat) (ariadne_tokens_total)'          # at now
ariadne query 'ariadne_tokens_total' --at 1h                  # an hour ago
ariadne query 'rate(ariadne_tokens_total[5m])' --since 6h     # a range
ariadne query 'up' --since 2026-01-01T00:00:00Z --step 1h --format json
```

A range query (`--since`) adds a `TIME` column and prints one row per point.
Its step defaults to the span over 250, rounded up to a whole second.
`--format json` prints the daemon's answer whole. A refused query prints the
daemon's `error` sentence.

### Point Grafana or Perses at the daemon

The query routes are on both listeners: the Unix socket and the TCP
listener. An external tool uses the TCP listener, so set `tcp_listen` in
[Configuration](configuration.md) first, for example
`tcp_listen = "127.0.0.1:7676"`.

- In Grafana, add a Prometheus data source with the URL
  `http://127.0.0.1:7676`, and a Loki data source with the same URL.
  Grafana's server sends the requests, so no origin needs to be allowed.
- In Perses, add a Prometheus datasource and a Loki datasource with the URL
  `http://127.0.0.1:7676`. If the browser calls the daemon directly, add
  the Perses page's origin to `tcp_allowed_origins`. The TCP listener sends
  the CORS headers for that origin.

The TCP listener serves only a loopback `Host`, so the tool must run on the
same machine as the daemon.
