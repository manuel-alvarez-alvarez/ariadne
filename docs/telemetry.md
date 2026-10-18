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
