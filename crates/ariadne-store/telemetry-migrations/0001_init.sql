-- Ariadne's telemetry schema: a second database, separate from ariadne.db,
-- so that the volume of logs and metrics a running daemon produces never
-- grows the file every other repository query reads. Ids are lowercase
-- ULIDs (TEXT, 26 chars); timestamps are ISO-8601 UTC TEXT.

-- One time series. `kind` is the OpenTelemetry/Prometheus instrument kind;
-- a histogram is stored as the `_bucket` (one row per `le` in `labels`),
-- `_sum` and `_count` series the Prometheus exposition format uses, each a
-- plain row here with its own name and labels — there is no fourth kind.
-- `label_hash` is a hash of `labels`' canonical (sorted-key) JSON text, so a
-- repeat point for the same name and labels finds its series with an
-- indexed equality lookup rather than a JSON comparison.
CREATE TABLE metric_series (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('gauge', 'counter', 'histogram')),
    unit        TEXT,
    description TEXT,
    labels      TEXT NOT NULL DEFAULT '{}',
    label_hash  TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (name, label_hash)
);

CREATE INDEX idx_metric_series_name ON metric_series (name);

-- One raw sample of a series, at full resolution. An hourly job marks each
-- sample after adding it to `metric_rollups_1h`; raw samples remain until
-- the configured telemetry_samples_days retention cutoff deletes them.
CREATE TABLE metric_samples (
    id        TEXT PRIMARY KEY,
    series_id TEXT NOT NULL REFERENCES metric_series (id),
    ts        TEXT NOT NULL,
    value     REAL NOT NULL,
    rolled_up INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_metric_samples_series_ts ON metric_samples (series_id, ts);
CREATE INDEX idx_metric_samples_ts ON metric_samples (ts);

-- One hour of a series. `first_ts` and `last_ts` retain the ordering needed
-- when a late raw sample merges after earlier raw rows have expired.
CREATE TABLE metric_rollups_1h (
    series_id TEXT NOT NULL REFERENCES metric_series (id),
    hour      TEXT NOT NULL,
    min       REAL NOT NULL,
    max       REAL NOT NULL,
    sum       REAL NOT NULL,
    count     INTEGER NOT NULL,
    first     REAL NOT NULL,
    last      REAL NOT NULL,
    first_ts  TEXT NOT NULL,
    last_ts   TEXT NOT NULL,
    PRIMARY KEY (series_id, hour)
);

CREATE INDEX idx_metric_rollups_1h_ts ON metric_rollups_1h (hour);

-- One log record. `service` is the emitting resource's `service.name`
-- (`ariadned`, `ariadne-desktop`, `ariadne-cli`); `scope` is the
-- instrumentation scope OpenTelemetry attaches (here, the `tracing` crate
-- name); `target` is the module path, kept separate from `scope` because a
-- Loki-shaped query filters on it directly. `attributes` is the event's
-- other fields, as JSON.
CREATE TABLE log_records (
    id               TEXT PRIMARY KEY,
    ts               TEXT NOT NULL,
    observed_ts      TEXT NOT NULL,
    severity_number  INTEGER,
    severity_text    TEXT,
    body             TEXT,
    service          TEXT,
    scope            TEXT,
    target           TEXT,
    attributes       TEXT NOT NULL DEFAULT '{}',
    trace_id         TEXT,
    span_id          TEXT
);

CREATE INDEX idx_log_records_ts ON log_records (ts);
CREATE INDEX idx_log_records_service_ts ON log_records (service, ts);
