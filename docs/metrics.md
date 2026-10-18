# Metrics

The daemon records what the work did, and how the daemon itself is doing, as
OpenTelemetry metrics. It keeps them in the telemetry database
([Telemetry](telemetry.md)), and they are the one source of stats. Read them
with `ariadne query`, or in the dashboard of the desktop app.

No metric names a goal, a task, a session, a launch or a path. A repository
is its name, the last part of its checkout path. A model is its catalog id,
`<agent>:<model>`. A value that is absent reads `none`.

## Names

Each metric is recorded under a dotted name, such as `ariadne.tasks.ended`.
A query uses its Prometheus name:

- A dot becomes an underscore.
- A counter ends in `_total`.
- A metric in seconds ends in `_seconds`, and one in bytes ends in `_bytes`.
- A histogram is three series: `_bucket` (with an `le` label), `_sum` and
  `_count`.

So `ariadne.tasks.ended` is `ariadne_tasks_ended_total`, and
`ariadne.task.lead_time` is `ariadne_task_lead_time_seconds_bucket`,
`ariadne_task_lead_time_seconds_sum` and `ariadne_task_lead_time_seconds_count`.

## The catalog

### What the work did

| Metric | Type | Labels | What it counts |
| --- | --- | --- | --- |
| `ariadne_tasks_ended_total` | counter | `status`, `step`, `landed`, `workflow`, `repo`, `model` | tasks that finished, were cancelled or failed |
| `ariadne_task_lead_time_seconds` | histogram | `status`, `step`, `workflow`, `repo` | from a task's creation to its ending |
| `ariadne_task_status_time_seconds` | histogram | `status` | the time a task spent in the status it left |
| `ariadne_goals_ended_total` | counter | `status`, `workflow`, `repo` | goals completed or cancelled |
| `ariadne_goal_lead_time_seconds` | histogram | `status`, `workflow` | from a goal's creation to its ending |
| `ariadne_sessions_ended_total` | counter | `status`, `seat`, `model`, `attention_reason` | agent runs that ended |
| `ariadne_session_lifetime_seconds` | histogram | `seat`, `model` | how long a run lived |
| `ariadne_tokens_total` | counter | `kind`, `model`, `seat`, `repo` | tokens spent: `input`, `cached_input` (part of `input`) and `output` |
| `ariadne_switches_total` | counter | `reason`, `automatic`, `same_agent`, `from_model`, `to_model` | model switches |
| `ariadne_messages_total` | counter | `kind`, `from_seat`, `to_seat` | agent messages |
| `ariadne_permissions_total` | counter | `decided_by`, `answer`, `tool_name` | permission answers |
| `ariadne_permission_wait_seconds` | histogram | `decided_by` | how long an agent waited for an answer |
| `ariadne_attention_raised_total` | counter | `reason` | attention flags raised |
| `ariadne_attention_cleared_total` | counter | `reason` | attention flags cleared |
| `ariadne_attention_wait_seconds` | histogram | `reason` | how long a flag stayed up |

The token counter moves while a turn runs, so spend shows before the turn
ends.

### Live state

| Metric | Labels | What it shows |
| --- | --- | --- |
| `ariadne_tasks_active` | `status` | unfinished tasks |
| `ariadne_goals_active` | `status` | unfinished goals |
| `ariadne_sessions_live` | `status`, `seat` | sessions starting, running or idle |
| `ariadne_attention_pending` | `reason` | flags up now |

These are read off the database each time the metrics are collected, so a
change shows in the next read.

### The daemon

| Metric | Type | Labels | What it shows |
| --- | --- | --- | --- |
| `http_server_request_duration_seconds` | histogram | `http.request.method`, `http.route`, `http.response.status_code` | API request time, by route template; streams and telemetry routes are left out |
| `ariadne_scheduler_tick_duration_seconds` | histogram | | one full scheduler pass |
| `ariadne_acp_launches_total` | counter | `outcome` | agent launches: `ready`, `failed`, `spawn_failed` |
| `ariadne_acp_launch_duration_seconds` | histogram | `outcome` | from an agent's spawn to its session ready |
| `ariadne_forge_poll_duration_seconds` | histogram | `repo`, `outcome` | one fetch of a repository's pull requests |
| `ariadne_forge_webhooks_total` | counter | `outcome` | webhook deliveries: `accepted`, `refused`, `failed` |
| `ariadne_events_stream_clients` | gauge | | clients following the event stream |
| `ariadne_events_stream_resyncs_total` | counter | | clients that fell behind and resynced |
| `ariadne_db_size_bytes` | gauge | `file` | the size of `ariadne.db` and `telemetry.db` |
| `ariadne_telemetry_deleted_total` | counter | `table` | rows the retention pass deleted |
| `process_cpu_time_seconds_total` | counter | | the daemon's CPU time |
| `process_memory_usage_bytes` | gauge | | the daemon's resident memory |

### Agent activity

| Metric | Type | Labels | What it counts |
| --- | --- | --- | --- |
| `ariadne_turns_total` | counter | `seat`, `model` | agent turns |
| `ariadne_turn_duration_seconds` | histogram | `seat`, `model` | from a prompt to its stop |
| `ariadne_tool_calls_total` | counter | `tool_name`, `model` | tool calls |
| `ariadne_ai_permissions_decisions_total` | counter | `verdict` | answers of the AI permission model |
| `ariadne_ai_permissions_latency_seconds` | histogram | | how long the model took |

## Reading them with `ariadne query`

`ariadne query EXPR` reads a series selector now. Add `--since SPAN --step
SPAN` for a range, and `--format json` for the Prometheus response. A counter
reads its total since the daemon started; the subset the query routes take
is in [Telemetry](telemetry.md#query-apis).

```sh
# Tasks that ended, by every label set
ariadne query 'ariadne_tasks_ended_total'

# Finished tasks of one repository
ariadne query 'ariadne_tasks_ended_total{status="finished",repo="ariadne"}'

# Output tokens of one model, over the last day
ariadne query 'ariadne_tokens_total{kind="output",model="claude:opus"}' --since 1d --step 1h

# Unfinished tasks now
ariadne query 'ariadne_tasks_active'
```

A Prometheus data source pointed at the daemon, such as the dashboard's,
reads the same names.

## The dashboard

The desktop app shows the shipped `stats` dashboard over these metrics
([spec 036](../specs/036-dashboards.md)). Its panels are queries like the
ones above, and a dashboard you edit is kept as your own.

The full catalog, with the place each metric is recorded, is in
[spec 035](../specs/035-metrics.md).
